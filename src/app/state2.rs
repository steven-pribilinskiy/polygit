use super::*;

impl AppState {
    pub fn visible_indices(&self) -> Vec<usize> {
        let filter = self.filter.as_ref().map(|filter| filter.to_lowercase());
        // A non-`@` name filter ranks results by fuzzy relevance (best first) like fzf; the `@`
        // status filter and the no-filter case keep the active column sort.
        let name_needle = filter
            .as_deref()
            .filter(|needle| !needle.is_empty() && !needle.starts_with('@'));
        // (index, fuzzy score) — score is 0 unless a name filter is ranking the results.
        let mut scored: Vec<(usize, u32)> = self
            .repos
            .iter()
            .enumerate()
            .filter_map(|(index, repo)| {
                let state = repo.lock().unwrap();
                if state.hidden || !self.status_filter.matches(&state.status) {
                    return None;
                }
                // The Favorites status-filter is repo-level (favorite_key by absolute path).
                if self.status_filter == StatusFilter::Favorites
                    && !self.favorites.contains(&favorite_key(&state.path))
                {
                    return None;
                }
                match filter.as_deref() {
                    None => Some((index, 0)),
                    Some(needle) => match needle.strip_prefix('@') {
                        Some(token) => Self::status_token_matches(&state, token).then_some((index, 0)),
                        None => tui_pick::finder::fuzzy_match(&state.rel_path, needle)
                            .map(|(score, _)| (index, score)),
                    },
                }
            })
            .collect();
        if name_needle.is_some() {
            // Rank by fuzzy score (best first), tie-break by name ascending.
            scored.sort_by(|&(left, left_score), &(right, right_score)| {
                right_score.cmp(&left_score).then_with(|| {
                    self.repos[left].lock().unwrap().rel_path.to_lowercase().cmp(
                        &self.repos[right].lock().unwrap().rel_path.to_lowercase(),
                    )
                })
            });
        } else {
            // The list is sorted by the active column (direction-aware), then ties break by name
            // (rel_path) ascending — always alphabetical, never discovery order, and independent of
            // the primary direction (so `branch ▼` lists branches Z→A but each branch's repos A→Z).
            scored.sort_by(|&(left, _), &(right, _)| {
                let primary = match self.sort_dir {
                    SortDir::Asc => self.compare_repos(left, right),
                    SortDir::Desc => self.compare_repos(left, right).reverse(),
                };
                primary.then_with(|| {
                    self.repos[left].lock().unwrap().rel_path.to_lowercase().cmp(
                        &self.repos[right].lock().unwrap().rel_path.to_lowercase(),
                    )
                })
            });
        }
        scored.into_iter().map(|(index, _)| index).collect()
    }

    /// The list rows in display order — the single source of truth for the list pane. With
    /// grouping inactive this is exactly `visible_indices()` as `Repo` rows; with grouping
    /// active, repos are partitioned into config-ordered group sections (each keeping the
    /// global sort/filter order), with an implicit "ungrouped" section last. Empty groups are
    /// hidden; collapsed groups keep their header but omit their members.
    pub fn visible_rows(&self) -> Vec<ListRow> {
        let visible = self.visible_indices();
        // Favorites-first: pin a "★ Favorites" section at the top (favorited repos in sort order),
        // then render the rest of the views below with favorites excluded from their normal place.
        let favorites = if self.favorites_first { self.favorite_visible(&visible) } else { Vec::new() };
        let mut rows = Vec::new();
        let body_visible: Vec<usize> = if favorites.is_empty() {
            visible
        } else {
            rows.push(ListRow::FavoritesHeader);
            rows.extend(favorites.iter().map(|&repo_idx| ListRow::Repo { repo_idx, depth: 0 }));
            rows.push(ListRow::Spacer);
            visible.into_iter().filter(|idx| !self.is_favorite(*idx)).collect()
        };
        // Tree view wins when active; groups subdivide repos inside each folder (tree+groups).
        let body = if self.tree_active() {
            self.visible_rows_tree(&body_visible)
        } else if !self.grouping_active() {
            body_visible.into_iter().map(ListRow::repo).collect()
        } else {
            self.grouped_rows(&body_visible, None, 0)
        };
        rows.extend(body);
        rows
    }

    /// Partition `visible` repos into config-ordered group sections (the grouped view, also
    /// reused inside each folder of the tree+groups view). `parent`/`base_depth` place the
    /// section within an enclosing folder (None / 0 at the top level). Empty groups are hidden;
    /// when nothing matches a named group the repos are returned flat (no lone "ungrouped"
    /// header). Spacers separate top-level sections only.
    fn grouped_rows(&self, visible: &[usize], parent: Option<usize>, base_depth: u16) -> Vec<ListRow> {
        let group_count = self.groups.len();
        // Collapse eligibility uses the TOTAL assigned membership (stable under filters).
        let mut totals = vec![0usize; group_count + 1];
        for assignment in &self.repo_group_map {
            totals[assignment.unwrap_or(group_count)] += 1;
        }
        let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); group_count + 1];
        for &repo_idx in visible {
            let bucket = self
                .repo_group_map
                .get(repo_idx)
                .copied()
                .flatten()
                .unwrap_or(group_count);
            buckets[bucket].push(repo_idx);
        }
        let repo_depth = base_depth;
        // Nothing matched any named group → plain flat list (no lone "ungrouped" header).
        if buckets[..group_count].iter().all(|bucket| bucket.is_empty()) {
            return buckets
                .swap_remove(group_count)
                .into_iter()
                .map(|repo_idx| ListRow::Repo { repo_idx, depth: repo_depth })
                .collect();
        }
        let mut rows = Vec::new();
        for (group_idx, bucket) in buckets.iter().enumerate() {
            if bucket.is_empty() {
                continue;
            }
            // Spacers separate top-level sections only (tree folders separate by indentation).
            if parent.is_none() && !rows.is_empty() {
                rows.push(ListRow::Spacer);
            }
            let collapsible = totals[group_idx] > self.collapse_threshold;
            rows.push(ListRow::GroupHeader { group_idx, parent, collapsible, depth: base_depth });
            let collapsed = collapsible
                && self.collapsed_groups.contains(&self.group_collapse_key(group_idx, parent));
            if !collapsed {
                // Repos sit at the same depth as their group header — the header is a divider,
                // not an extra indent level (matching the original flat-under-header look).
                rows.extend(
                    bucket
                        .iter()
                        .map(|&repo_idx| ListRow::Repo { repo_idx, depth: repo_depth }),
                );
            }
        }
        rows
    }

    /// The directory-tree rows: the root's own repos first, then a pre-order walk of the folder
    /// nodes (a folder is shown only when its subtree holds a visible repo; collapsed folders
    /// keep their header but omit descendants). When grouping is also on, each folder's repos
    /// are subdivided by group.
    fn visible_rows_tree(&self, visible: &[usize]) -> Vec<ListRow> {
        use std::collections::HashMap;
        let pos: HashMap<usize, usize> =
            visible.iter().enumerate().map(|(order, &idx)| (idx, order)).collect();

        // Mark every node whose subtree contains a visible repo (walk up from each visible repo).
        let mut owner: HashMap<usize, usize> = HashMap::new();
        for (node_idx, node) in self.tree_nodes.iter().enumerate() {
            for &repo_idx in &node.repos {
                owner.insert(repo_idx, node_idx);
            }
        }
        let mut has_visible = vec![false; self.tree_nodes.len()];
        for &repo_idx in visible {
            let mut current = owner.get(&repo_idx).copied();
            while let Some(node_idx) = current {
                if has_visible[node_idx] {
                    break;
                }
                has_visible[node_idx] = true;
                current = self.tree_nodes[node_idx].parent;
            }
        }

        let mut rows = Vec::new();
        // Root-level repos: those not assigned to any folder node (the tree's implicit root), in
        // sort order — and grouped when grouping's on. Uses the node ownership map rather than the
        // raw rel_path so the multi-root forest (paths prefixed with a root label) partitions right.
        let root_repos: Vec<usize> =
            visible.iter().copied().filter(|idx| !owner.contains_key(idx)).collect();
        if !root_repos.is_empty() {
            if self.grouping_active() {
                rows.extend(self.grouped_rows(&root_repos, None, 0));
            } else {
                rows.extend(root_repos.into_iter().map(ListRow::repo));
            }
        }

        // Top-level folders, sorted by name, each walked in pre-order.
        let mut top: Vec<usize> = (0..self.tree_nodes.len())
            .filter(|&idx| self.tree_nodes[idx].parent.is_none())
            .collect();
        top.sort_by(|&a, &b| self.tree_nodes[a].name.cmp(&self.tree_nodes[b].name));
        for node_idx in top {
            self.emit_tree_node(node_idx, &pos, &has_visible, &mut rows);
        }
        rows
    }

    /// Emit one folder node (and its visible subtree) into `rows`. Pre-order: header, then child
    /// folders, then this folder's own repos. Skipped entirely when the subtree has no visible repo.
    fn emit_tree_node(
        &self,
        node_idx: usize,
        pos: &std::collections::HashMap<usize, usize>,
        has_visible: &[bool],
        rows: &mut Vec<ListRow>,
    ) {
        if !has_visible.get(node_idx).copied().unwrap_or(false) {
            return;
        }
        let node = &self.tree_nodes[node_idx];
        rows.push(ListRow::FolderHeader { node_idx, depth: node.depth });
        if self.collapsed_folders.contains(&node.rel_path) {
            return;
        }
        for &child in &node.children {
            self.emit_tree_node(child, pos, has_visible, rows);
        }
        // This folder's own repos, in global sort order.
        let mut own: Vec<usize> = node.repos.iter().copied().filter(|idx| pos.contains_key(idx)).collect();
        own.sort_by_key(|idx| pos[idx]);
        if own.is_empty() {
            return;
        }
        if self.grouping_active() {
            rows.extend(self.grouped_rows(&own, Some(node_idx), node.depth + 1));
        } else {
            let depth = node.depth + 1;
            rows.extend(own.into_iter().map(|repo_idx| ListRow::Repo { repo_idx, depth }));
        }
    }

    /// The collapse-set key for a group section: the bare group name at the top level, or
    /// `"{folder}::{name}"` inside a folder (so the same group collapses independently per folder).
    pub fn group_collapse_key(&self, group_idx: usize, parent: Option<usize>) -> String {
        let name = self.group_name(group_idx);
        match parent.and_then(|node_idx| self.tree_nodes.get(node_idx)) {
            Some(node) => format!("{}::{name}", node.rel_path),
            None => name.to_string(),
        }
    }

    /// The visible (filtered) members of a group, in display order. `groups.len()` = ungrouped.
    pub fn group_visible_members(&self, group_idx: usize) -> Vec<usize> {
        let sentinel = self.groups.len();
        self.visible_indices()
            .into_iter()
            .filter(|&repo_idx| {
                self.repo_group_map
                    .get(repo_idx)
                    .copied()
                    .flatten()
                    .unwrap_or(sentinel)
                    == group_idx
            })
            .collect()
    }

    /// The row under the current selection (None when Result/Errors is selected).
    pub fn selected_row(&self) -> Option<ListRow> {
        self.visible_rows().get(self.selected).copied()
    }

    /// Whether the logical row at `idx` can hold the selection. Repo rows and collapsible
    /// headers can; static headers and spacers can't; Result/Errors (past the rows) always can.
    pub(crate) fn row_selectable_in(rows: &[ListRow], total: usize, idx: usize) -> bool {
        match rows.get(idx) {
            Some(ListRow::Repo { .. }) => true,
            Some(ListRow::FolderHeader { .. }) => true,
            Some(ListRow::GroupHeader { collapsible, .. }) => *collapsible,
            Some(ListRow::FavoritesHeader) | Some(ListRow::Spacer) => false,
            None => idx < total,
        }
    }

    /// Whether `row` is a header the selection can land on (a folder, or a collapsible group).
    fn is_selectable_header(row: ListRow) -> bool {
        matches!(
            row,
            ListRow::FolderHeader { .. } | ListRow::GroupHeader { collapsible: true, .. }
        )
    }

    /// Whether the header `row` is currently collapsed (false for non-headers).
    fn header_collapsed(&self, row: ListRow) -> bool {
        match row {
            ListRow::FolderHeader { node_idx, .. } => self
                .tree_nodes
                .get(node_idx)
                .is_some_and(|node| self.collapsed_folders.contains(&node.rel_path)),
            ListRow::GroupHeader { group_idx, parent, collapsible: true, .. } => {
                self.collapsed_groups.contains(&self.group_collapse_key(group_idx, parent))
            }
            _ => false,
        }
    }

    /// Collapse or expand the header `row` (no-op for non-headers / static group headers).
    fn set_header_collapsed(&mut self, row: ListRow, collapsed: bool) {
        match row {
            ListRow::FolderHeader { node_idx, .. } => {
                if let Some(node) = self.tree_nodes.get(node_idx) {
                    let key = node.rel_path.clone();
                    if collapsed {
                        self.collapsed_folders.insert(key);
                    } else {
                        self.collapsed_folders.remove(&key);
                    }
                }
            }
            ListRow::GroupHeader { group_idx, parent, collapsible: true, .. } => {
                let key = self.group_collapse_key(group_idx, parent);
                if collapsed {
                    self.collapsed_groups.insert(key);
                } else {
                    self.collapsed_groups.remove(&key);
                }
            }
            _ => {}
        }
    }

    /// Toggle whichever header the selection sits on (folder or collapsible group), keeping the
    /// selection valid. Returns true if a header was toggled (so callers fall through otherwise).
    pub fn toggle_selected_header(&mut self) -> bool {
        let Some(row) = self.selected_row() else {
            return false;
        };
        if Self::is_selectable_header(row) {
            let collapsed = self.header_collapsed(row);
            self.set_header_collapsed(row, !collapsed);
            let total = self.list_len();
            self.selected = self.selected.min(total.saturating_sub(1));
            self.snap_selection(false);
            true
        } else {
            false
        }
    }

    /// Move the selection off a non-selectable row to the nearest selectable one, scanning the
    /// preferred direction first, then the other. (No-op when the current row is selectable.)
    pub(crate) fn snap_selection(&mut self, prefer_down: bool) {
        let rows = self.visible_rows();
        let total = rows.len() + 1 + usize::from(self.has_errors());
        self.selected = self.selected.min(total.saturating_sub(1));
        if Self::row_selectable_in(&rows, total, self.selected) {
            return;
        }
        let down = (self.selected + 1..total).find(|&idx| Self::row_selectable_in(&rows, total, idx));
        let up = (0..self.selected)
            .rev()
            .find(|&idx| Self::row_selectable_in(&rows, total, idx));
        let (first, second) = if prefer_down { (down, up) } else { (up, down) };
        if let Some(idx) = first.or(second) {
            self.selected = idx;
        }
    }

    /// Collapse/expand a group section (by header position + enclosing folder) and keep the
    /// selection valid. `parent` is the folder node the section lives in (None at the top level).
    pub fn toggle_group_collapsed(&mut self, group_idx: usize, parent: Option<usize>) {
        let key = self.group_collapse_key(group_idx, parent);
        if !self.collapsed_groups.remove(&key) {
            self.collapsed_groups.insert(key);
        }
        let total = self.list_len();
        self.selected = self.selected.min(total.saturating_sub(1));
        self.snap_selection(false);
        // Persisted on exit (like sort), not on every toggle.
    }

    /// Clamp the selection into range and move it off any non-selectable row.
    fn clamp_and_snap(&mut self) {
        let total = self.list_len();
        self.selected = self.selected.min(total.saturating_sub(1));
        self.snap_selection(false);
    }

    /// Folder node indices in a subtree, including the node itself.
    fn tree_descendant_nodes(&self, node_idx: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![node_idx];
        while let Some(idx) = stack.pop() {
            out.push(idx);
            if let Some(node) = self.tree_nodes.get(idx) {
                stack.extend(node.children.iter().copied());
            }
        }
        out
    }

    /// Collapse every folder and every collapsible group section (`-` / `z M`).
    pub fn collapse_all(&mut self) {
        for row in self.visible_rows() {
            if Self::is_selectable_header(row) {
                self.set_header_collapsed(row, true);
            }
        }
        self.result_overlay = false;
        self.clamp_and_snap();
    }

    /// Expand every folder and group section (`+`/`=` / `z R`).
    pub fn expand_all(&mut self) {
        self.collapsed_folders.clear();
        self.collapsed_groups.clear();
        self.result_overlay = false;
        self.clamp_and_snap();
    }

    /// Expand the selected header's whole subtree (`*` / `z O`): for a folder, every descendant
    /// folder + group section within it; for a group header, just that section; for a repo, its
    /// enclosing folder chain.
    pub fn expand_subtree(&mut self) {
        use std::collections::HashSet;
        match self.selected_row() {
            Some(ListRow::FolderHeader { node_idx, .. }) => {
                let nodes = self.tree_descendant_nodes(node_idx);
                let folders: HashSet<String> = nodes
                    .iter()
                    .filter_map(|&idx| self.tree_nodes.get(idx))
                    .map(|node| node.rel_path.clone())
                    .collect();
                for folder in &folders {
                    self.collapsed_folders.remove(folder);
                }
                // Group sections nested under any expanded folder are keyed `folder::name`.
                self.collapsed_groups.retain(|key| match key.rsplit_once("::") {
                    Some((folder, _)) => !folders.contains(folder),
                    None => true,
                });
            }
            Some(ListRow::GroupHeader { group_idx, parent, collapsible: true, .. }) => {
                let key = self.group_collapse_key(group_idx, parent);
                self.collapsed_groups.remove(&key);
            }
            _ => {}
        }
        self.result_overlay = false;
        self.clamp_and_snap();
    }

    /// `←` (tree-style): on an expanded folder/group header, collapse it in place; on a repo or
    /// a collapsed header, jump to the nearest selectable header above (the enclosing parent).
    pub fn nav_left(&mut self) {
        let rows = self.visible_rows();
        let Some(&current) = rows.get(self.selected) else {
            return;
        };
        if Self::is_selectable_header(current) && !self.header_collapsed(current) {
            self.user_navigated = true;
            self.result_overlay = false;
            self.set_header_collapsed(current, true);
            let total = self.list_len();
            self.selected = self.selected.min(total.saturating_sub(1));
            self.snap_selection(false);
            return;
        }
        // Jump to the immediate enclosing header (nearest header above), but only when it's
        // selectable — a repo under a static (small-group / ungrouped) header has no foldable
        // parent, so ← is inert there.
        if let Some(header_idx) = (0..self.selected).rev().find(|&idx| {
            matches!(rows[idx], ListRow::FolderHeader { .. } | ListRow::GroupHeader { .. })
        }) {
            if Self::is_selectable_header(rows[header_idx]) {
                self.user_navigated = true;
                self.result_overlay = false;
                self.selected = header_idx;
            }
        }
    }

    /// `→`: on a collapsed folder/group header, expand it. No-op elsewhere.
    pub fn nav_right(&mut self) {
        let Some(current) = self.selected_row() else {
            return;
        };
        if Self::is_selectable_header(current) && self.header_collapsed(current) {
            self.user_navigated = true;
            self.result_overlay = false;
            self.set_header_collapsed(current, false);
        }
    }

    /// Re-point the selection at the same repo after the row layout changed (grouping toggled,
    /// dynamic membership arrived). Falls back to clamp + snap when the repo is gone from view.
    pub fn reselect_repo(&mut self, prev: Option<usize>) {
        if let Some(repo_idx) = prev {
            let rows = self.visible_rows();
            if let Some(pos) = rows
                .iter()
                .position(|row| matches!(row, ListRow::Repo { repo_idx: idx, .. } if *idx == repo_idx))
            {
                self.selected = pos;
                return;
            }
        }
        self.snap_selection(false);
    }

    /// Open the folder picker, starting at the first root (or home) with the saved bookmarks.
    pub fn open_picker(&mut self) {
        self.close_all_modals();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let start = self.root_dirs.first().cloned().unwrap_or_else(|| home.clone());
        let bookmarks = self.folder_bookmarks.iter().map(PathBuf::from).collect();
        self.picker = Some(tui_pick::picker::PickerState::new(start, home, bookmarks));
    }

    /// Add a folder/repo as a workspace root (canonicalized, deduped) and persist. Returns the
    /// canonical path when it's newly added (so the caller can kick off discovery for it).
    pub fn add_root(&mut self, path: PathBuf) -> Option<PathBuf> {
        let abs = std::fs::canonicalize(&path).unwrap_or(path);
        if self.root_dirs.contains(&abs) {
            self.show_toast(format!("Already in the workspace: {}", abs.display()));
            return None;
        }
        self.root_dirs.push(abs.clone());
        // Re-adding a previously-removed root: un-hide its repos (discovery's dedup would otherwise
        // skip them, leaving them hidden). The discovery pass then fills in any genuinely new ones.
        let mut unhidden = false;
        for repo in &self.repos {
            let mut state = repo.lock().unwrap();
            if state.root == abs && state.hidden {
                state.hidden = false;
                unhidden = true;
            }
        }
        if unhidden {
            self.rebuild_tree();
        }
        self.save_state();
        self.show_toast(format!("Added {}", abs.display()));
        Some(abs)
    }

    /// Remove the workspace root that the selected repo (or selected folder header) belongs to:
    /// drop it from `root_dirs`, persist, and hide its repos (kept in the append-only vec so worker
    /// indices stay valid). No-op when there's only one root or nothing is selected.
    pub fn remove_selected_root(&mut self) {
        let Some(root) = self.selected_root() else {
            return;
        };
        if self.root_dirs.len() <= 1 {
            self.show_toast("Can't remove the only folder in the workspace".to_string());
            return;
        }
        let mut hidden = 0;
        for repo in &self.repos {
            let mut state = repo.lock().unwrap();
            if state.root == root && !state.hidden {
                state.hidden = true;
                hidden += 1;
            }
        }
        self.root_dirs.retain(|dir| dir != &root);
        self.save_state();
        self.rebuild_tree();
        self.recompute_group_assignments();
        self.snap_selection(false);
        self.show_toast(format!("Removed {} ({hidden} repos)", root.display()));
    }

    /// The root of the current selection — a folder header's root, else the selected repo's root.
    fn selected_root(&self) -> Option<PathBuf> {
        match self.selected_row()? {
            ListRow::Repo { repo_idx, .. } => Some(self.repos[repo_idx].lock().unwrap().root.clone()),
            ListRow::FolderHeader { node_idx, .. } => {
                // A top-level folder node maps to a root; find a repo under it to read its root.
                let repos = self.tree_subtree_repos(node_idx);
                repos.first().map(|&idx| self.repos[idx].lock().unwrap().root.clone())
            }
            _ => None,
        }
    }

    /// Persist any bookmark changes the picker made back into the saved set (call on close).
    pub fn sync_picker_bookmarks(&mut self) {
        if let Some(picker) = self.picker.as_ref() {
            let bookmarks: Vec<String> =
                picker.bookmarks.iter().map(|path| path.display().to_string()).collect();
            if bookmarks != self.folder_bookmarks {
                self.folder_bookmarks = bookmarks;
                self.save_state();
            }
        }
    }

    /// Open the fzf-style finder over every repo (most-used first, mirroring goto-repo's default).
    pub fn open_finder(&mut self) {
        self.close_all_modals();
        let rows: Vec<tui_pick::finder::FinderRow> = self
            .repos
            .iter()
            .map(|repo| {
                let state = repo.lock().unwrap();
                let path = state.path.display().to_string();
                tui_pick::finder::FinderRow {
                    key: path.clone(),
                    kind: "repo".to_string(),
                    display: path,
                }
            })
            .collect();
        self.finder = Some(tui_pick::finder::FinderState::new(
            rows,
            tui_pick::SortMode::MostUsed,
            &self.finder_history,
        ));
    }

    /// Jump the list selection to the repo with absolute path `key` (a finder accept) and record the
    /// visit in the shared goto-repo history so recent/most-used reflect it next time.
    pub fn finder_jump(&mut self, key: &str) {
        if let Some(idx) =
            self.repos.iter().position(|repo| repo.lock().unwrap().path.display().to_string() == key)
        {
            self.finder_history.record_use(key);
            self.user_navigated = true;
            self.result_overlay = false;
            self.reselect_repo(Some(idx));
            self.ensure_list_selection_visible(self.list_rows_area.height as usize);
        }
    }

    /// Whether `repo_idx` is marked a favorite. Keyed by **absolute path** so favorites stay
    /// unambiguous across multiple roots (two roots can share a relative path).
    pub fn is_favorite(&self, repo_idx: usize) -> bool {
        self.repos
            .get(repo_idx)
            .is_some_and(|repo| self.favorites.contains(&favorite_key(&repo.lock().unwrap().path)))
    }

    /// Toggle a repo's favorite state (persists), keyed by its absolute path.
    pub fn toggle_favorite(&mut self, repo_idx: usize) {
        let Some(repo) = self.repos.get(repo_idx) else {
            return;
        };
        let key = favorite_key(&repo.lock().unwrap().path);
        if !self.favorites.remove(&key) {
            self.favorites.insert(key);
        }
        let prev = self.selected_repo_index();
        self.reselect_repo(prev);
        self.save_state();
    }

    /// Toggle the favorite state of the currently-selected repo.
    pub fn toggle_selected_favorite(&mut self) {
        if let Some(repo_idx) = self.selected_repo_index() {
            self.toggle_favorite(repo_idx);
        }
    }

    /// Toggle the "★ Favorites pinned to top" mode, keeping the selection on the same repo.
    pub fn toggle_favorites_first(&mut self) {
        self.favorites_first = !self.favorites_first;
        let prev = self.selected_repo_index();
        self.reselect_repo(prev);
        self.save_state();
    }

    /// Whether any repo is favorited (gates the favorites-first footer toggle + pinned section).
    pub fn has_favorites(&self) -> bool {
        !self.favorites.is_empty()
    }

    /// Visible repos that are favorited, in the active sort order.
    fn favorite_visible(&self, visible: &[usize]) -> Vec<usize> {
        visible.iter().copied().filter(|&idx| self.is_favorite(idx)).collect()
    }

    /// Enter name-filter input mode, remembering the current selection so Esc can restore it.
    pub fn begin_filter_input(&mut self) {
        self.filter_input_mode = true;
        if self.filter.is_none() {
            self.filter = Some(String::new());
        }
        self.filter_prev_selection = self.selected_repo_index();
    }

    /// Commit the name filter (Enter / click the hint again): stop editing, keep the current
    /// selection, and forget the remembered pre-filter repo.
    pub fn commit_filter_input(&mut self) {
        self.filter_input_mode = false;
        self.filter_prev_selection = None;
    }

    /// Cancel name-filter input (Esc): clear the filter and restore the pre-filter selection.
    pub fn cancel_filter_input(&mut self) {
        self.filter_input_mode = false;
        self.filter = None;
        let prev = self.filter_prev_selection.take();
        self.reselect_repo(prev);
    }

    /// While typing a non-empty filter, snap the selection to the first matching repo row so the
    /// match is previewed live. A no-op for an empty filter (don't jump just for opening `/`).
    pub fn select_first_filtered_row(&mut self) {
        if self.filter.as_deref().unwrap_or("").is_empty() {
            return;
        }
        let rows = self.visible_rows();
        if let Some(pos) = rows.iter().position(|row| matches!(row, ListRow::Repo { .. })) {
            self.selected = pos;
        } else {
            self.snap_selection(false);
        }
    }

    /// Compare two repos by the active sort column (ascending). Missing details sort as 0.
    fn compare_repos(&self, a: usize, b: usize) -> std::cmp::Ordering {
        let left = self.repos[a].lock().unwrap();
        let right = self.repos[b].lock().unwrap();
        let worktrees = |name: &str| self.worktrees.iter().filter(|wt| wt.repo == name).count();
        match self.sort_column {
            SortColumn::Name => left.rel_path.to_lowercase().cmp(&right.rel_path.to_lowercase()),
            SortColumn::Branch => {
                let key = |state: &RepoState| {
                    state.branch.as_deref().unwrap_or("").to_lowercase()
                };
                key(&left).cmp(&key(&right))
            }
            SortColumn::Status => left.status.sort_rank().cmp(&right.status.sort_rank()),
            SortColumn::AheadBehind => {
                let key = |state: &RepoState| {
                    let details = state.details.as_ref();
                    (
                        details.and_then(|d| d.behind).unwrap_or(0),
                        details.and_then(|d| d.ahead).unwrap_or(0),
                    )
                };
                key(&left).cmp(&key(&right))
            }
            SortColumn::Dirty => {
                let key = |state: &RepoState| state.details.as_ref().map_or(0, |d| d.dirty_count);
                key(&left).cmp(&key(&right))
            }
            SortColumn::LastCommit => {
                // Newest first under ascending feels wrong; use the raw timestamp ascending
                // (oldest first), so Desc gives newest first.
                let key =
                    |state: &RepoState| state.details.as_ref().map_or(0, |d| d.commit_timestamp);
                key(&left).cmp(&key(&right))
            }
            SortColumn::Worktrees => worktrees(&left.name).cmp(&worktrees(&right.name)),
            SortColumn::Branches => {
                let key = |state: &RepoState| state.details.as_ref().map_or(0, |d| d.branch_count);
                key(&left).cmp(&key(&right))
            }
            SortColumn::Stashes => {
                let key = |state: &RepoState| state.details.as_ref().map_or(0, |d| d.stash_count);
                key(&left).cmp(&key(&right))
            }
            SortColumn::PulledCommits => {
                let key = |state: &RepoState| state.pull_result.as_ref().map_or(0, |p| p.commits);
                key(&left).cmp(&key(&right))
            }
            SortColumn::PulledFiles => {
                let key = |state: &RepoState| state.pull_result.as_ref().map_or(0, |p| p.files);
                key(&left).cmp(&key(&right))
            }
            SortColumn::PullRequest => {
                // Repos with a shown PR first (by number asc), PR-less repos last (in Asc). A
                // merged/closed PR counts as PR-less unless the "Merged PRs" setting is on.
                let show_merged = self.show_merged_prs;
                let key = |state: &RepoState| {
                    let number = state
                        .pr
                        .as_ref()
                        .filter(|pr| pr.shown(show_merged))
                        .map(|pr| pr.number);
                    (number.is_none(), number.unwrap_or(0))
                };
                key(&left).cmp(&key(&right))
            }
            SortColumn::Favorite => {
                // Favorited repos first (Asc) — keyed by absolute path, like `is_favorite`.
                let key = |state: &RepoState| !self.favorites.contains(&favorite_key(&state.path));
                key(&left).cmp(&key(&right))
            }
        }
    }

    /// Apply a sort column: re-pressing the active column flips direction, a new column resets to Asc.
    pub fn set_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_dir = self.sort_dir.flip();
        } else {
            self.sort_column = column;
            self.sort_dir = SortDir::Asc;
        }
        self.result_overlay = false;
        let max = self.list_len().saturating_sub(1);
        if self.selected > max {
            self.selected = max;
        }
        self.snap_selection(true);
        // Persisted on exit (like the column toggles), not on every keystroke.
    }

    /// The sort column whose header cell is at `(col,row)`, if any (mouse click-to-sort).
    pub fn header_sort_at(&self, col: u16, row: u16) -> Option<SortColumn> {
        let area = self.header_area;
        if area.height == 0 || row < area.y || row >= area.y + area.height {
            return None;
        }
        self.header_click
            .iter()
            .find(|(start, end, _)| col >= *start && col < *end)
            .map(|(_, _, column)| *column)
    }

    /// Apply a status filter and reset the selection (the visible set just changed).
    pub fn set_status_filter(&mut self, filter: StatusFilter) {
        self.status_filter = filter;
        self.selected = 0;
        self.result_overlay = false;
        self.snap_selection(true);
    }

    /// Number of rows in the settings modal.
    pub const SETTINGS_ROWS: usize = 30;

    /// One-line tooltip for a settings row (or a specific option, where it adds something) —
    /// shown after ~1s of hovering, like the footer command tooltips. Keyed by the global row
    /// index (see `SETTINGS_TABS`) and the hovered option, if any.
    pub fn settings_tip(row: usize, option: Option<usize>) -> Option<&'static str> {
        // Derived from the single-source `SETTINGS` table (co-located label + tip), so a tooltip
        // can never drift to the wrong row on a reorder/insert. An option-specific tip (e.g. the
        // Icons unicode/emoji rows) wins when present; otherwise the row's general tip.
        let info = crate::app::SETTINGS.get(row)?;
        if let Some(opt) = option {
            if let Some(tip) = info.option_tips.get(opt) {
                return Some(tip);
            }
        }
        Some(info.tip)
    }

    /// `(first global row, row count)` for settings tab `tab` (index into `SETTINGS_TABS`).
    pub fn settings_tab_range(tab: usize) -> (usize, usize) {
        let start: usize = SETTINGS_TABS.iter().take(tab).map(|(_, count)| count).sum();
        let len = SETTINGS_TABS.get(tab).map_or(0, |(_, count)| *count);
        (start, len)
    }

    /// Whether the tabbed settings view draws a blank group-separator row *before* this global
    /// settings row. Visual only (nav/clicks ignore the blank). Splits the Theming tab into
    /// Icons (+ Hide zeros) / palette / selection+hover groups.
    pub fn settings_tabbed_blank_before(row: usize) -> bool {
        // Blank before Theme (5) — separates the Icons group (Icons + Hide zeros) from the palette
        // group — and before List selection (8) — groups List selection + Button hover.
        row == 5 || row == 8
    }

    /// Which settings tab a global row belongs to.
    pub fn settings_tab_of_row(row: usize) -> usize {
        let mut acc = 0;
        for (tab, (_, count)) in SETTINGS_TABS.iter().enumerate() {
            acc += count;
            if row < acc {
                return tab;
            }
        }
        SETTINGS_TABS.len().saturating_sub(1)
    }

    /// Switch to settings tab `tab`, moving the selection to that tab's first row.
    pub fn settings_select_tab(&mut self, tab: usize) {
        if tab >= SETTINGS_TABS.len() {
            return;
        }
        self.settings_tab = tab;
        self.settings_selected = Self::settings_tab_range(tab).0;
    }

    /// Cycle to the next/previous settings tab (wrapping).
    pub fn settings_cycle_tab(&mut self, forward: bool) {
        let count = SETTINGS_TABS.len();
        let next = if forward {
            (self.settings_tab + 1) % count
        } else {
            (self.settings_tab + count - 1) % count
        };
        self.settings_select_tab(next);
    }

    /// Whether settings section `tab_idx` is collapsed (accordion layout only).
    pub fn settings_section_collapsed(&self, tab_idx: usize) -> bool {
        SETTINGS_TABS
            .get(tab_idx)
            .is_some_and(|(name, _)| self.collapsed_settings.contains(*name))
    }

    /// Toggle a settings section's collapsed state (accordion layout). The selection stays on its
    /// row — even when hidden — so its header stays highlighted and ←/→ can re-expand it.
    pub fn toggle_settings_section(&mut self, tab_idx: usize) {
        let Some((name, _)) = SETTINGS_TABS.get(tab_idx) else {
            return;
        };
        if self.collapsed_settings.contains(*name) {
            self.collapsed_settings.remove(*name);
        } else {
            self.collapsed_settings.insert((*name).to_string());
        }
        self.save_state();
    }

    /// Collapse (or expand) the focused section (accordion ←/→) — its header or the selected row's
    /// section. Expanding while on a header keeps focus on the header.
    pub fn set_selected_settings_section(&mut self, collapse: bool) {
        if self.settings_layout != SettingsLayout::Accordion {
            return;
        }
        let tab = self
            .settings_on_header
            .unwrap_or_else(|| Self::settings_tab_of_row(self.settings_selected));
        if self.settings_section_collapsed(tab) != collapse {
            self.toggle_settings_section(tab);
        }
        // Collapsing hides the section's rows. If focus was on one of those rows, move it to the
        // section header — otherwise the selection would point at a now-hidden row and nothing would
        // read as focused (you couldn't tell what just happened). The header then shows its
        // highlight, so a left-press always lands somewhere visible.
        if collapse {
            self.settings_on_header = Some(tab);
        }
    }

    /// Whether every settings section is collapsed (drives the expand/collapse-all label).
    pub fn settings_all_collapsed(&self) -> bool {
        SETTINGS_TABS.iter().all(|(name, _)| self.collapsed_settings.contains(*name))
    }

    /// Collapse every section, or expand every section if all are already collapsed (accordion).
    pub fn toggle_all_settings_sections(&mut self) {
        if self.settings_all_collapsed() {
            self.collapsed_settings.clear();
        } else {
            for (name, _) in SETTINGS_TABS {
                self.collapsed_settings.insert((*name).to_string());
            }
        }
        self.save_state();
    }

    /// The accordion's navigable positions top-to-bottom: each section's header, then (when the
    /// section is expanded) its rows. Rows in collapsed sections are omitted but still advance the
    /// global row index so `Row(_)` stays correct.
    pub fn accordion_positions(&self) -> Vec<AccPos> {
        let mut positions = Vec::new();
        let mut row = 0usize;
        for (section, (_, count)) in SETTINGS_TABS.iter().enumerate() {
            positions.push(AccPos::Header(section));
            let collapsed = self.settings_section_collapsed(section);
            for _ in 0..*count {
                if !collapsed {
                    positions.push(AccPos::Row(row));
                }
                row += 1;
            }
        }
        positions
    }

    /// The accordion's currently-selected position (header or row).
    pub fn accordion_selection(&self) -> AccPos {
        match self.settings_on_header {
            Some(section) => AccPos::Header(section),
            None => AccPos::Row(self.settings_selected),
        }
    }

    /// Apply an accordion position as the selection (updates header-vs-row state).
    fn set_accordion_selection(&mut self, position: AccPos) {
        match position {
            AccPos::Header(section) => self.settings_on_header = Some(section),
            AccPos::Row(row) => {
                self.settings_on_header = None;
                self.settings_selected = row;
                self.settings_tab = Self::settings_tab_of_row(row);
            }
        }
    }

    /// Toggle the accordion section that currently holds focus — its own header, or the section of
    /// the selected row. Used by `enter`/`space` and a header click.
    pub fn toggle_focused_accordion_section(&mut self) {
        let section = self
            .settings_on_header
            .unwrap_or_else(|| Self::settings_tab_of_row(self.settings_selected));
        self.toggle_settings_section(section);
    }

    /// Move the settings selection by `delta`, clamped to the active tab in the tabbed layout (and
    /// to the whole list in flat/accordion). In accordion mode, rows in collapsed sections are
    /// skipped. Keeps `settings_tab` in sync with the selection.
    pub fn settings_move(&mut self, delta: isize) {
        // While searching, navigate the flat filtered list regardless of layout.
        if !self.settings_search.is_empty() {
            let matches = self.settings_filtered_rows();
            if matches.is_empty() {
                return;
            }
            let current = matches.iter().position(|&row| row == self.settings_selected).unwrap_or(0);
            let next = (current as isize + delta).clamp(0, matches.len() as isize - 1) as usize;
            self.settings_selected = matches[next];
            self.settings_tab = Self::settings_tab_of_row(self.settings_selected);
            return;
        }
        if self.settings_layout == SettingsLayout::Accordion {
            // Navigate the interleaved header/row sequence (headers are selectable; rows in
            // collapsed sections are skipped because they aren't in `accordion_positions`).
            let positions = self.accordion_positions();
            if positions.is_empty() {
                return;
            }
            let current =
                positions.iter().position(|pos| *pos == self.accordion_selection()).unwrap_or(0);
            let next = (current as isize + delta).clamp(0, positions.len() as isize - 1) as usize;
            self.set_accordion_selection(positions[next]);
            return;
        }
        let (lo, hi) = if self.settings_layout == SettingsLayout::Tabbed {
            let (start, len) = Self::settings_tab_range(self.settings_tab);
            (start as isize, (start + len).saturating_sub(1) as isize)
        } else {
            (0, Self::SETTINGS_ROWS.saturating_sub(1) as isize)
        };
        let current = self.settings_selected as isize;
        self.settings_selected = (current + delta).clamp(lo, hi) as usize;
        self.settings_tab = Self::settings_tab_of_row(self.settings_selected);
    }

    /// Toggle/cycle the currently-selected settings row one step forward, persisting immediately.
    /// Row indices are the global order in `SETTINGS` / `SETTINGS_LABELS` (alphabetical sections).
    pub fn toggle_selected_setting(&mut self) {
        match self.settings_selected {
            // Agent
            0 => self.claude_agent = self.claude_agent.cycle(),
            1 => self.claude_skip_permissions = !self.claude_skip_permissions,
            // Interaction
            2 => self.hover_effects = !self.hover_effects,
            3 => self.changed_row_effect = self.changed_row_effect.cycle(),
            // Layout
            4 => self.panel_padding = !self.panel_padding,
            5 => self.show_borders = !self.show_borders,
            6 => self.splitter_mode = self.splitter_mode.cycle(),
            7 => {
                self.repo_page_tabs = self.repo_page_tabs.cycle();
                self.repo_page_tabbed_override = None; // changing the preference clears any `v` flip
            }
            8 => self.branch_check = self.branch_check.cycle(),
            9 => self.info_layout = self.info_layout.cycle(),
            // Lists
            10 => {
                let prev = self.selected_repo_index();
                self.grouping_enabled = !self.grouping_enabled;
                self.reselect_repo(prev);
            }
            11 => {
                let prev = self.selected_repo_index();
                self.tree_enabled = !self.tree_enabled;
                self.reselect_repo(prev);
            }
            12 => self.hide_folder_lines = !self.hide_folder_lines,
            // Pull requests
            13 => self.show_merged_prs = !self.show_merged_prs,
            // Sync
            14 => self.auto_pull_on_launch = !self.auto_pull_on_launch,
            15 => self.auto_pull_max_repos = next_auto_pull_limit(self.auto_pull_max_repos),
            16 => self.auto_pull_in_tree = !self.auto_pull_in_tree,
            // Theming
            17 => {
                self.icon_style = match self.icon_style {
                    IconStyle::Unicode => IconStyle::Emoji,
                    IconStyle::Emoji => IconStyle::Unicode,
                };
            }
            // Inert in emoji mode (always hides zeros); only togglable with the Unicode set.
            18 if self.icon_style != IconStyle::Emoji => {
                self.hide_zero_counts = !self.hide_zero_counts;
            }
            19 => self.theme = self.theme.cycle(),
            20 => self.background = self.background.cycle(),
            21 => self.contrast = self.contrast.cycle(),
            22 => self.selection_style = self.selection_style.cycle(),
            23 => self.button_hover_style = self.button_hover_style.cycle(),
            // Tooltips
            24 => self.tooltips.set_all(!self.tooltips.all_on()),
            25 => self.tooltips.footer = !self.tooltips.footer,
            26 => self.tooltips.headers = !self.tooltips.headers,
            27 => self.tooltips.counts = !self.tooltips.counts,
            28 => self.tooltips.settings = !self.tooltips.settings,
            29 => self.tooltips.links = !self.tooltips.links,
            _ => {}
        }
        self.save_state();
    }

    /// Cycle the selected setting's value one option forward / backward — the ←/→ keys in the tabbed
    /// and flat layouts. Built on the same `set_setting_option` dispatch the radio chips use, so all
    /// side effects (grouping/tree reselect, the emoji-mode Hide-zeros guard, etc.) are identical.
    pub fn cycle_selected_setting(&mut self, forward: bool) {
        let row = self.settings_selected;
        let count = Self::settings_option_labels(row).len();
        if count == 0 {
            return;
        }
        let active = self.settings_active_option(row).min(count - 1);
        let next = if forward { (active + 1) % count } else { (active + count - 1) % count };
        self.set_setting_option(row, next);
    }
}
