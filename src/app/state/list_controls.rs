//! Sort/filter/favorites/column toggles for the repo list.

use super::*;

impl AppState {
    pub fn status_token_matches(state: &RepoState, token: &str) -> bool {
        if token.is_empty() {
            return true;
        }
        let status_key = match state.status {
            RepoStatus::Queued => "queued",
            RepoStatus::Running { .. } => "running",
            RepoStatus::UpToDate => "up-to-date",
            RepoStatus::Updated => "updated",
            RepoStatus::NoUpstream => "no-upstream",
            RepoStatus::Skipped => "skipped",
            RepoStatus::Throttled => "throttled",
            RepoStatus::Failed => "failed",
        };
        let mut keys: Vec<&str> = vec![status_key];
        if let Some(details) = &state.details {
            keys.push(if details.dirty_count > 0 { "dirty" } else { "clean" });
            if details.ahead.unwrap_or(0) > 0 {
                keys.push("ahead");
            }
            if details.behind.unwrap_or(0) > 0 {
                keys.push("behind");
            }
        }
        keys.iter().any(|key| key.contains(token))
    }

    pub fn is_favorite(&self, repo_idx: usize) -> bool {
        self.repos
            .get(repo_idx)
            .is_some_and(|repo| self.favorites.contains(&favorite_key(&repo.lock().unwrap().path)))
    }

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

    pub fn toggle_selected_favorite(&mut self) {
        if let Some(repo_idx) = self.selected_repo_index() {
            self.toggle_favorite(repo_idx);
        }
    }

    pub fn toggle_favorites_first(&mut self) {
        self.favorites_first = !self.favorites_first;
        let prev = self.selected_repo_index();
        self.reselect_repo(prev);
        self.save_state();
    }

    pub fn has_favorites(&self) -> bool {
        !self.favorites.is_empty()
    }

    pub(super) fn favorite_visible(&self, visible: &[usize]) -> Vec<usize> {
        visible.iter().copied().filter(|&idx| self.is_favorite(idx)).collect()
    }

    pub fn begin_filter_input(&mut self) {
        self.filter_input_mode = true;
        if self.filter.is_none() {
            self.filter = Some(String::new());
        }
        self.filter_prev_selection = self.selected_repo_index();
    }

    pub fn commit_filter_input(&mut self) {
        self.filter_input_mode = false;
        self.filter_prev_selection = None;
    }

    pub fn cancel_filter_input(&mut self) {
        self.filter_input_mode = false;
        self.filter = None;
        let prev = self.filter_prev_selection.take();
        self.reselect_repo(prev);
    }

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

    pub fn set_status_filter(&mut self, filter: StatusFilter) {
        self.status_filter = filter;
        self.selected = 0;
        self.result_overlay = false;
        self.snap_selection(true);
    }

    /// Which filter kinds are currently narrowing the list, in `FilterKind::ALL` order — the
    /// filter bar's active chips.
    pub fn active_filter_kinds(&self) -> Vec<FilterKind> {
        FilterKind::ALL
            .iter()
            .copied()
            .filter(|kind| match kind {
                FilterKind::Status => self.status_filter != StatusFilter::All,
                FilterKind::Branch => self.branch_filter.is_some(),
            })
            .collect()
    }

    /// Open (or reopen, to reconfigure) a filter kind's own config UI — the same action whether
    /// triggered by a direct key (`f`, `Ctrl+F`), a click on an active chip's label, or a pick
    /// from the "+ add filter" menu. `anchor` is `(right, row)`, mirroring `open_dropdown`'s params.
    pub fn open_filter_kind(&mut self, kind: FilterKind, anchor: (u16, u16)) {
        match kind {
            FilterKind::Status => self.open_dropdown(DropdownKind::ListFilter, anchor.0, anchor.1),
            FilterKind::Branch => self.open_branch_filter(),
        }
    }

    /// Clear one filter kind back to neutral — a filter-bar chip's `×`.
    pub fn clear_filter_kind(&mut self, kind: FilterKind) {
        match kind {
            FilterKind::Status => self.set_status_filter(StatusFilter::All),
            FilterKind::Branch => self.branch_filter = None,
        }
    }

    /// Clear every currently-active filter at once (`F`, the filter bar's "reset filters" button).
    pub fn reset_all_filters(&mut self) {
        for kind in self.active_filter_kinds() {
            self.clear_filter_kind(kind);
        }
    }

    pub fn toggle_column(&mut self, column: Column) {
        match column {
            Column::Status => self.columns.status = !self.columns.status,
            Column::AheadBehind => self.columns.ahead_behind = !self.columns.ahead_behind,
            Column::Dirty => self.columns.dirty = !self.columns.dirty,
            Column::LastCommit => self.columns.last_commit = !self.columns.last_commit,
            Column::Worktrees => self.columns.worktrees = !self.columns.worktrees,
            Column::Branches => self.columns.branches = !self.columns.branches,
            Column::Stashes => self.columns.stashes = !self.columns.stashes,
            Column::PulledCommits => self.columns.pulled_commits = !self.columns.pulled_commits,
            Column::PulledFiles => self.columns.pulled_files = !self.columns.pulled_files,
            Column::PullRequest => {
                self.columns.pull_request = !self.columns.pull_request;
                // Re-arm the all-repos PR pass so re-enabling re-resolves stale entries.
                if !self.columns.pull_request {
                    self.pr_pass_spawned = false;
                }
            }
            Column::Favorite => self.columns.favorite = !self.columns.favorite,
        }
    }

    fn any_pull_result(&self) -> bool {
        self.repos.iter().any(|repo| repo.lock().unwrap().pull_result.is_some())
    }

    pub fn refresh_pulled_seen(&mut self) {
        if !self.pulled_seen && self.any_pull_result() {
            self.pulled_seen = true;
        }
    }

    pub fn column_available(&self, column: Column) -> bool {
        match column {
            Column::Status | Column::AheadBehind | Column::Dirty | Column::LastCommit => true,
            Column::Worktrees => {
                if !self.worktrees_done {
                    return true;
                }
                self.repos.iter().any(|repo| {
                    let name = repo.lock().unwrap().name.clone();
                    self.worktrees.iter().any(|entry| entry.repo == name)
                })
            }
            Column::Branches => {
                if !self.discovery_done {
                    return true;
                }
                self.repos.iter().any(|repo| {
                    match repo.lock().unwrap().details.as_ref() {
                        None => true,
                        Some(details) => details.branch_count > 1,
                    }
                })
            }
            Column::Stashes => {
                if !self.discovery_done {
                    return true;
                }
                self.repos.iter().any(|repo| {
                    match repo.lock().unwrap().details.as_ref() {
                        None => true,
                        Some(details) => details.stash_count > 0,
                    }
                })
            }
            // The pulled columns come from the pulls themselves. Once any pull has landed a delta
            // this session the columns latch on (`pulled_seen`) and stay — so a retry/refetch, which
            // briefly clears every `pull_result`, no longer flickers them out and back in.
            Column::PulledCommits | Column::PulledFiles => self.pulled_seen,
            // Self-fills via `gh` in the background; always available when enabled (cells are
            // blank for repos without a PR or not yet resolved).
            Column::PullRequest => true,
            // The star is always meaningful (it's how you favorite a repo).
            Column::Favorite => true,
        }
    }

    pub fn effective_columns(&self) -> ColumnFlags {
        ColumnFlags {
            status: self.columns.status,
            ahead_behind: self.columns.ahead_behind,
            dirty: self.columns.dirty,
            last_commit: self.columns.last_commit,
            worktrees: self.columns.worktrees && self.column_available(Column::Worktrees),
            branches: self.columns.branches && self.column_available(Column::Branches),
            stashes: self.columns.stashes && self.column_available(Column::Stashes),
            pulled_commits: self.columns.pulled_commits
                && self.column_available(Column::PulledCommits),
            pulled_files: self.columns.pulled_files && self.column_available(Column::PulledFiles),
            pull_request: self.columns.pull_request && self.column_available(Column::PullRequest),
            favorite: self.columns.favorite,
        }
    }
}
