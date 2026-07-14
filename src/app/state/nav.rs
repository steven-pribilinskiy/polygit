//! Selection/movement/scroll, the roots picker, and the name finder.

use super::*;

impl AppState {
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

    pub fn open_picker(&mut self) {
        self.close_all_modals();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let start = self.root_dirs.first().cloned().unwrap_or_else(|| home.clone());
        let bookmarks = self.folder_bookmarks.iter().map(PathBuf::from).collect();
        self.picker = Some(tui_pick::picker::PickerState::new(start, home, bookmarks));
    }

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

    pub fn nav_up(&mut self) -> bool {
        self.user_navigated = true;
        self.result_overlay = false;
        let rows = self.visible_rows();
        let total = rows.len() + 1 + usize::from(self.has_errors());
        let mut idx = self.selected.min(total.saturating_sub(1));
        while idx > 0 {
            idx -= 1;
            if Self::row_selectable_in(&rows, total, idx) {
                self.selected = idx;
                return true;
            }
        }
        false
    }

    pub fn nav_down(&mut self) -> bool {
        self.user_navigated = true;
        self.result_overlay = false;
        let rows = self.visible_rows();
        let total = rows.len() + 1 + usize::from(self.has_errors());
        let mut idx = self.selected;
        while idx + 1 < total {
            idx += 1;
            if Self::row_selectable_in(&rows, total, idx) {
                self.selected = idx;
                return true;
            }
        }
        false
    }

    pub fn nav_top(&mut self) {
        self.user_navigated = true;
        self.result_overlay = false;
        self.selected = 0;
        self.snap_selection(true);
    }

    pub fn nav_bottom(&mut self) {
        self.user_navigated = true;
        self.result_overlay = false;
        self.selected = self.list_len().saturating_sub(1);
    }

    pub fn selected_item_row(&self) -> usize {
        let rows = self.visible_rows().len();
        if self.selected < rows {
            self.selected
        } else if self.selected == rows {
            rows + 1 // one separator before Result
        } else {
            rows + 3 // separator + Result + separator before Errors
        }
    }

    pub fn total_item_rows(&self) -> usize {
        let rows = self.visible_rows().len();
        rows + 2 // separator + Result
            + if self.has_errors() { 2 } else { 0 } // separator + Errors
            + if self.discovery_done && self.repos.is_empty() { 2 } else { 0 } // blank + hint
    }

    pub fn max_list_scroll(&self, viewport: usize) -> usize {
        self.total_item_rows().saturating_sub(viewport)
    }

    pub fn scroll_list(&mut self, delta: isize, viewport: usize) {
        let max = self.max_list_scroll(viewport) as isize;
        self.list_scroll = (self.list_scroll as isize + delta).clamp(0, max.max(0)) as usize;
    }

    pub fn ensure_list_selection_visible(&mut self, viewport: usize) {
        if viewport == 0 {
            return;
        }
        let item = self.selected_item_row();
        if item < self.list_scroll {
            self.list_scroll = item;
        } else if item >= self.list_scroll + viewport {
            self.list_scroll = item + 1 - viewport;
        }
        self.list_scroll = self.list_scroll.min(self.max_list_scroll(viewport));
    }

    pub fn nav_page_up(&mut self, step: usize) {
        self.user_navigated = true;
        self.result_overlay = false;
        self.selected = self.selected.saturating_sub(step.max(1));
        self.snap_selection(false);
    }

    pub fn nav_page_down(&mut self, step: usize) {
        self.user_navigated = true;
        self.result_overlay = false;
        let max = self.list_len().saturating_sub(1);
        self.selected = (self.selected + step.max(1)).min(max);
        self.snap_selection(true);
    }

    pub fn selected_repo_index(&self) -> Option<usize> {
        match self.visible_rows().get(self.selected) {
            Some(ListRow::Repo { repo_idx, .. }) => Some(*repo_idx),
            _ => None,
        }
    }
}
