//! Visible-row emission: filter/sort, grouped and tree row lists, header collapse.

use super::*;

/// A repo's frozen filter/sort fields, snapshotted under a single lock in `visible_indices`
/// before sorting — see that function for why re-locking live inside the comparator is unsafe.
struct RankedRepo {
    index: usize,
    /// Fuzzy-match score against a name filter; 0 when no name filter is active.
    score: u32,
    name_lower: String,
    sort_key: RepoSortKey,
}

/// A frozen per-repo sort key for the currently-active `SortColumn`, captured once per repo
/// instead of read live on every pairwise comparison during a sort. Only same-variant keys are
/// ever compared in practice since `sort_column` is fixed for the whole `visible_indices` call,
/// but the derived `Ord` (which orders by variant position first) is a valid total order
/// regardless of that.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum RepoSortKey {
    Text(String),
    Num(u32),
    NumPair(u32, u32),
    Timestamp(i64),
    PullRequest(bool, u32),
    Bool(bool),
}

impl AppState {
    pub fn visible_indices(&self) -> Vec<usize> {
        let filter = self.filter.as_ref().map(|filter| filter.to_lowercase());
        // A non-`@` name filter ranks results by fuzzy relevance (best first) like fzf; the `@`
        // status filter and the no-filter case keep the active column sort.
        let name_needle = filter
            .as_deref()
            .filter(|needle| !needle.is_empty() && !needle.starts_with('@'));
        // Snapshot each candidate repo's filter/sort fields under ONE lock, before sorting. The
        // comparators here used to re-lock `self.repos[x]` live on every comparison — since each
        // repo's mutex is independent of AppState's own lock, a background pull/detail worker
        // mutating `RepoState` (status, dirty_count, ahead/behind, …) between two comparisons of
        // the SAME pair could flip the result mid-sort, violating the total-order guarantee
        // `sort_by` requires and panicking ("user-provided comparison function does not correctly
        // implement a total order"). Locking once up front freezes every comparison against a
        // consistent snapshot instead of live, concurrently-mutating state.
        let mut scored: Vec<RankedRepo> = self
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
                let score = match filter.as_deref() {
                    None => Some(0),
                    Some(needle) => match needle.strip_prefix('@') {
                        Some(token) => Self::status_token_matches(&state, token).then_some(0),
                        None => tui_pick::finder::fuzzy_match(&state.rel_path, needle)
                            .map(|(score, _)| score),
                    },
                }?;
                Some(RankedRepo {
                    index,
                    score,
                    name_lower: state.rel_path.to_lowercase(),
                    sort_key: self.repo_sort_key(&state),
                })
            })
            .collect();
        if name_needle.is_some() {
            // Rank by fuzzy score (best first), tie-break by name ascending.
            scored.sort_by(|left, right| {
                right.score.cmp(&left.score).then_with(|| left.name_lower.cmp(&right.name_lower))
            });
        } else {
            // The list is sorted by the active column (direction-aware), then ties break by name
            // (rel_path) ascending — always alphabetical, never discovery order, and independent of
            // the primary direction (so `branch ▼` lists branches Z→A but each branch's repos A→Z).
            scored.sort_by(|left, right| {
                let primary = match self.sort_dir {
                    SortDir::Asc => left.sort_key.cmp(&right.sort_key),
                    SortDir::Desc => left.sort_key.cmp(&right.sort_key).reverse(),
                };
                primary.then_with(|| left.name_lower.cmp(&right.name_lower))
            });
        }
        scored.into_iter().map(|ranked| ranked.index).collect()
    }

    fn repo_sort_key(&self, state: &RepoState) -> RepoSortKey {
        match self.sort_column {
            SortColumn::Name => RepoSortKey::Text(state.rel_path.to_lowercase()),
            SortColumn::Branch => {
                RepoSortKey::Text(state.branch.as_deref().unwrap_or("").to_lowercase())
            }
            SortColumn::Status => RepoSortKey::Num(state.status.sort_rank() as u32),
            SortColumn::AheadBehind => {
                let details = state.details.as_ref();
                RepoSortKey::NumPair(
                    details.and_then(|d| d.behind).unwrap_or(0),
                    details.and_then(|d| d.ahead).unwrap_or(0),
                )
            }
            SortColumn::Dirty => {
                RepoSortKey::Num(state.details.as_ref().map_or(0, |d| d.dirty_count))
            }
            SortColumn::LastCommit => {
                // Newest first under ascending feels wrong; use the raw timestamp ascending
                // (oldest first), so Desc gives newest first.
                RepoSortKey::Timestamp(state.details.as_ref().map_or(0, |d| d.commit_timestamp))
            }
            SortColumn::Worktrees => {
                let count =
                    self.worktrees.iter().filter(|worktree| worktree.repo == state.name).count();
                RepoSortKey::Num(count as u32)
            }
            SortColumn::Branches => {
                RepoSortKey::Num(state.details.as_ref().map_or(0, |d| d.branch_count))
            }
            SortColumn::Stashes => {
                RepoSortKey::Num(state.details.as_ref().map_or(0, |d| d.stash_count))
            }
            SortColumn::PulledCommits => {
                RepoSortKey::Num(state.pull_result.as_ref().map_or(0, |p| p.commits))
            }
            SortColumn::PulledFiles => {
                RepoSortKey::Num(state.pull_result.as_ref().map_or(0, |p| p.files))
            }
            SortColumn::PullRequest => {
                // Repos with a shown PR first (by number asc), PR-less repos last (in Asc). A
                // merged/closed PR counts as PR-less unless the "Merged PRs" setting is on.
                let number = state
                    .pr
                    .as_ref()
                    .filter(|pr| pr.shown(self.show_merged_prs))
                    .map(|pr| pr.number);
                RepoSortKey::PullRequest(number.is_none(), number.unwrap_or(0))
            }
            SortColumn::Favorite => {
                // Favorited repos first (Asc) — keyed by absolute path, like `is_favorite`.
                RepoSortKey::Bool(!self.favorites.contains(&favorite_key(&state.path)))
            }
        }
    }

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

    pub fn group_collapse_key(&self, group_idx: usize, parent: Option<usize>) -> String {
        let name = self.group_name(group_idx);
        match parent.and_then(|node_idx| self.tree_nodes.get(node_idx)) {
            Some(node) => format!("{}::{name}", node.rel_path),
            None => name.to_string(),
        }
    }

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

    pub fn selected_row(&self) -> Option<ListRow> {
        self.visible_rows().get(self.selected).copied()
    }

    pub(crate) fn row_selectable_in(rows: &[ListRow], total: usize, idx: usize) -> bool {
        match rows.get(idx) {
            Some(ListRow::Repo { .. }) => true,
            Some(ListRow::FolderHeader { .. }) => true,
            Some(ListRow::GroupHeader { collapsible, .. }) => *collapsible,
            Some(ListRow::FavoritesHeader) | Some(ListRow::Spacer) => false,
            None => idx < total,
        }
    }

    pub(super) fn is_selectable_header(row: ListRow) -> bool {
        matches!(
            row,
            ListRow::FolderHeader { .. } | ListRow::GroupHeader { collapsible: true, .. }
        )
    }

    pub(super) fn header_collapsed(&self, row: ListRow) -> bool {
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

    pub(super) fn set_header_collapsed(&mut self, row: ListRow, collapsed: bool) {
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

    fn clamp_and_snap(&mut self) {
        let total = self.list_len();
        self.selected = self.selected.min(total.saturating_sub(1));
        self.snap_selection(false);
    }

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

    pub fn collapse_all(&mut self) {
        for row in self.visible_rows() {
            if Self::is_selectable_header(row) {
                self.set_header_collapsed(row, true);
            }
        }
        self.result_overlay = false;
        self.clamp_and_snap();
    }

    pub fn expand_all(&mut self) {
        self.collapsed_folders.clear();
        self.collapsed_groups.clear();
        self.result_overlay = false;
        self.clamp_and_snap();
    }

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
}
