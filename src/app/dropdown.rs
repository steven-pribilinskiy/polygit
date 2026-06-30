use super::*;

/// The toggleable list columns, in dropdown order: `(column, label, mnemonic)`.
const LIST_COLS: &[(Column, &str, char)] = &[
    (Column::Status, "status", 'u'),
    (Column::AheadBehind, "ahead/behind", 'a'),
    (Column::Dirty, "dirty", 'd'),
    (Column::LastCommit, "last commit", 'l'),
    (Column::Worktrees, "worktrees", 'w'),
    (Column::Branches, "branches", 'b'),
    (Column::Stashes, "stashes", 's'),
    (Column::PulledCommits, "pulled", 'p'),
    (Column::PulledFiles, "changed", 'c'),
    (Column::PullRequest, "pull request", 'r'),
    (Column::Favorite, "favorite", 'f'),
];

/// The sortable list columns, in dropdown order: `(sort, label, mnemonic)`.
const LIST_SORTS: &[(SortColumn, &str, char)] = &[
    (SortColumn::Name, "name", 'n'),
    (SortColumn::Branch, "branch", 'c'),
    (SortColumn::Status, "status", 's'),
    (SortColumn::AheadBehind, "ahead/behind", 'a'),
    (SortColumn::Dirty, "dirty", 'd'),
    (SortColumn::LastCommit, "last commit", 'l'),
    (SortColumn::Worktrees, "worktrees", 'w'),
    (SortColumn::Branches, "branches", 'b'),
    (SortColumn::Stashes, "stashes", 'k'),
    (SortColumn::PulledCommits, "pulled", 'p'),
    (SortColumn::PulledFiles, "changed", 'g'),
    (SortColumn::Favorite, "favorite", 'v'),
];

/// The status-filter options, in dropdown order: `(filter, label, mnemonic)`.
const LIST_FILTERS: &[(StatusFilter, &str, char)] = &[
    (StatusFilter::All, "all", 'a'),
    (StatusFilter::Updated, "updated", 'u'),
    (StatusFilter::UpToDate, "up-to-date", 'c'),
    (StatusFilter::Skipped, "skipped", 's'),
    (StatusFilter::Failed, "failed", 'f'),
    (StatusFilter::Issues, "issues", 'i'),
    (StatusFilter::Favorites, "favorites", 'v'),
];

/// The toggleable repo-page columns, in dropdown order: `(column, label, mnemonic)`.
const PAGE_COLS: &[(RepoPageColumn, &str, char)] = &[
    (RepoPageColumn::AheadBehind, "ahead/behind", 'b'),
    (RepoPageColumn::Dirty, "dirty", 'y'),
    (RepoPageColumn::Added, "added", 'a'),
    (RepoPageColumn::Modified, "modified", 'm'),
    (RepoPageColumn::Deleted, "deleted", 'd'),
    (RepoPageColumn::Total, "total", 'c'),
    (RepoPageColumn::Upstream, "upstream", 'u'),
    (RepoPageColumn::Base, "base", 'f'),
    (RepoPageColumn::Age, "age", 'g'),
    (RepoPageColumn::PullRequest, "pr", 'r'),
    (RepoPageColumn::Subject, "subject", 's'),
];

/// The toggleable Stashes-tab columns, in dropdown order: `(column, label, mnemonic)`.
const STASH_COLS: &[(RepoPageStashColumn, &str, char)] = &[
    (RepoPageStashColumn::Age, "age", 'a'),
    (RepoPageStashColumn::Stats, "stats", 's'),
];

/// The sortable repo-page columns, in dropdown order: `(sort, label, mnemonic)`.
const PAGE_SORTS: &[(RepoPageSort, &str, char)] = &[
    (RepoPageSort::Name, "name", 'n'),
    (RepoPageSort::AheadBehind, "ahead/behind", 'b'),
    (RepoPageSort::Dirty, "dirty", 'y'),
    (RepoPageSort::Added, "added", 'a'),
    (RepoPageSort::Modified, "modified", 'm'),
    (RepoPageSort::Deleted, "deleted", 'd'),
    (RepoPageSort::Total, "total", 'c'),
    (RepoPageSort::Upstream, "upstream", 'u'),
    (RepoPageSort::Base, "base", 'f'),
    (RepoPageSort::Age, "age", 'g'),
    (RepoPageSort::Subject, "subject", 's'),
];

impl AppState {
    /// Open a header dropdown so its right edge aligns under the chip's right column (`right`).
    pub fn open_dropdown(&mut self, kind: DropdownKind, right: u16, row: u16) {
        self.dropdown = Some(Dropdown { kind, anchor_right: right, anchor_row: row, selected: None });
    }

    pub fn close_dropdown(&mut self) {
        self.dropdown = None;
    }

    /// The open dropdown's rows. Empty when none is open.
    pub fn dropdown_items(&self) -> Vec<DropdownItem> {
        let Some(dropdown) = self.dropdown else {
            return Vec::new();
        };
        match dropdown.kind {
            DropdownKind::ListColumns => LIST_COLS
                .iter()
                .map(|&(column, label, mnemonic)| DropdownItem {
                    label: label.to_string(),
                    on: self.column_on(column),
                    mnemonic,
                    enabled: self.column_available(column),
                })
                .collect(),
            DropdownKind::ListSort => LIST_SORTS
                .iter()
                .map(|&(sort, label, mnemonic)| DropdownItem {
                    label: label.to_string(),
                    on: self.sort_column == sort,
                    mnemonic,
                    enabled: true,
                })
                .collect(),
            DropdownKind::ListFilter => LIST_FILTERS
                .iter()
                .map(|&(filter, label, mnemonic)| DropdownItem {
                    label: label.to_string(),
                    on: self.status_filter == filter,
                    mnemonic,
                    enabled: true,
                })
                .collect(),
            DropdownKind::PageColumns => PAGE_COLS
                .iter()
                .map(|&(column, label, mnemonic)| DropdownItem {
                    label: label.to_string(),
                    on: self.repo_page_column_on(column),
                    mnemonic,
                    enabled: self.repo_page_column_available(column),
                })
                .collect(),
            DropdownKind::PageSort => PAGE_SORTS
                .iter()
                .map(|&(sort, label, mnemonic)| DropdownItem {
                    label: label.to_string(),
                    on: self.repo_page_sort == Some(sort),
                    mnemonic,
                    enabled: true,
                })
                .collect(),
            DropdownKind::StashColumns => STASH_COLS
                .iter()
                .map(|&(column, label, mnemonic)| DropdownItem {
                    label: label.to_string(),
                    on: self.repo_page_stash_column_on(column),
                    mnemonic,
                    enabled: true,
                })
                .collect(),
            DropdownKind::ExplorerColumns => crate::explorer::ExplorerColumn::ALL
                .iter()
                .map(|&(column, label, mnemonic)| DropdownItem {
                    label: label.to_string(),
                    on: self.explorer.as_ref().is_some_and(|explorer| column.enabled(&explorer.columns)),
                    mnemonic,
                    enabled: true,
                })
                .collect(),
            DropdownKind::ExplorerSort => crate::explorer::SortKey::ALL
                .iter()
                .map(|&(key, label, mnemonic)| DropdownItem {
                    label: label.to_string(),
                    on: self.explorer.as_ref().is_some_and(|explorer| explorer.sort == key),
                    mnemonic,
                    enabled: true,
                })
                .collect(),
        }
    }

    /// Whether a Stashes-tab column is currently shown.
    fn repo_page_stash_column_on(&self, column: RepoPageStashColumn) -> bool {
        match column {
            RepoPageStashColumn::Age => self.repo_page_stash_columns.age,
            RepoPageStashColumn::Stats => self.repo_page_stash_columns.stats,
        }
    }

    /// Toggle a Stashes-tab column on/off.
    pub fn toggle_repo_page_stash_column(&mut self, column: RepoPageStashColumn) {
        match column {
            RepoPageStashColumn::Age => self.repo_page_stash_columns.age = !self.repo_page_stash_columns.age,
            RepoPageStashColumn::Stats => self.repo_page_stash_columns.stats = !self.repo_page_stash_columns.stats,
        }
    }

    /// Whether a list `Column` is currently enabled.
    fn column_on(&self, column: Column) -> bool {
        let flags = &self.columns;
        match column {
            Column::Status => flags.status,
            Column::AheadBehind => flags.ahead_behind,
            Column::Dirty => flags.dirty,
            Column::LastCommit => flags.last_commit,
            Column::Worktrees => flags.worktrees,
            Column::Branches => flags.branches,
            Column::Stashes => flags.stashes,
            Column::PulledCommits => flags.pulled_commits,
            Column::PulledFiles => flags.pulled_files,
            Column::PullRequest => flags.pull_request,
            Column::Favorite => flags.favorite,
        }
    }

    /// Whether a repo-page column is currently enabled.
    fn repo_page_column_on(&self, column: RepoPageColumn) -> bool {
        let flags = &self.repo_page_columns;
        match column {
            RepoPageColumn::AheadBehind => flags.ahead_behind,
            RepoPageColumn::Dirty => flags.dirty,
            RepoPageColumn::Added => flags.added,
            RepoPageColumn::Modified => flags.modified,
            RepoPageColumn::Deleted => flags.deleted,
            RepoPageColumn::Total => flags.total,
            RepoPageColumn::Upstream => flags.upstream,
            RepoPageColumn::Base => flags.base,
            RepoPageColumn::Age => flags.age,
            RepoPageColumn::PullRequest => flags.pull_request,
            RepoPageColumn::Subject => flags.subject,
        }
    }

    /// Number of items in the open dropdown.
    pub fn dropdown_len(&self) -> usize {
        self.dropdown.map_or(0, |dropdown| match dropdown.kind {
            DropdownKind::ListColumns => LIST_COLS.len(),
            DropdownKind::ListSort => LIST_SORTS.len(),
            DropdownKind::ListFilter => LIST_FILTERS.len(),
            DropdownKind::PageColumns => PAGE_COLS.len(),
            DropdownKind::PageSort => PAGE_SORTS.len(),
            DropdownKind::StashColumns => STASH_COLS.len(),
            DropdownKind::ExplorerColumns => crate::explorer::ExplorerColumn::ALL.len(),
            DropdownKind::ExplorerSort => crate::explorer::SortKey::ALL.len(),
        })
    }

    /// Move the dropdown highlight by `delta`, clamped. From nothing-highlighted, a downward move
    /// lands on the first row and an upward move on the last.
    pub fn dropdown_move(&mut self, delta: isize) {
        let len = self.dropdown_len();
        if len == 0 {
            return;
        }
        let last = (len - 1) as isize;
        if let Some(dropdown) = self.dropdown.as_mut() {
            let next = match dropdown.selected {
                Some(current) => (current as isize + delta).clamp(0, last),
                None if delta < 0 => last,
                None => 0,
            };
            dropdown.selected = Some(next as usize);
        }
    }

    /// Activate the enabled item whose mnemonic key matches `ch` (case-insensitive). Returns whether
    /// the dropdown should now close; `false` (stay open) when no item matches.
    pub fn dropdown_activate_key(&mut self, ch: char) -> bool {
        let index = self
            .dropdown_items()
            .iter()
            .position(|item| item.enabled && item.mnemonic == ch);
        match index {
            Some(index) => self.dropdown_activate(index),
            None => false,
        }
    }

    /// Whether every column in the open columns dropdown is currently on (drives the dynamic
    /// select/deselect-all label). False for non-columns dropdowns.
    pub fn dropdown_all_columns_on(&self) -> bool {
        match self.dropdown.map(|dropdown| dropdown.kind) {
            Some(DropdownKind::ListColumns) => LIST_COLS.iter().all(|&(column, ..)| self.column_on(column)),
            Some(DropdownKind::PageColumns) => {
                PAGE_COLS.iter().all(|&(column, ..)| self.repo_page_column_on(column))
            }
            Some(DropdownKind::StashColumns) => {
                STASH_COLS.iter().all(|&(column, ..)| self.repo_page_stash_column_on(column))
            }
            Some(DropdownKind::ExplorerColumns) => self
                .explorer
                .as_ref()
                .is_some_and(|explorer| {
                    crate::explorer::ExplorerColumn::ALL.iter().all(|&(column, ..)| column.enabled(&explorer.columns))
                }),
            _ => false,
        }
    }

    /// Turn every column in the open columns dropdown on — or, when all are already on, off
    /// (dynamic select/deselect-all). No-op for non-columns dropdowns.
    pub fn dropdown_toggle_all_columns(&mut self) {
        let target = !self.dropdown_all_columns_on(); // all-on → turn off; otherwise turn on
        match self.dropdown.map(|dropdown| dropdown.kind) {
            Some(DropdownKind::ListColumns) => {
                for &(column, ..) in LIST_COLS {
                    if self.column_on(column) != target {
                        self.toggle_column(column);
                    }
                }
            }
            Some(DropdownKind::PageColumns) => {
                for &(column, ..) in PAGE_COLS {
                    if self.repo_page_column_on(column) != target {
                        self.toggle_repo_page_column(column);
                    }
                }
            }
            Some(DropdownKind::StashColumns) => {
                for &(column, ..) in STASH_COLS {
                    if self.repo_page_stash_column_on(column) != target {
                        self.toggle_repo_page_stash_column(column);
                    }
                }
            }
            Some(DropdownKind::ExplorerColumns) => {
                let current = self.explorer.as_ref().map(|explorer| explorer.columns);
                if let Some(columns) = current {
                    for &(column, ..) in &crate::explorer::ExplorerColumn::ALL {
                        if column.enabled(&columns) != target {
                            self.toggle_explorer_column(column);
                        }
                    }
                }
            }
            _ => return,
        }
        self.save_state();
    }

    /// Reset the open columns dropdown's selection to its defaults. No-op for non-columns dropdowns.
    pub fn dropdown_reset_columns(&mut self) {
        match self.dropdown.map(|dropdown| dropdown.kind) {
            Some(DropdownKind::ListColumns) => self.columns = ColumnFlags::default(),
            Some(DropdownKind::PageColumns) => self.repo_page_columns = RepoPageColumns::default(),
            Some(DropdownKind::StashColumns) => {
                self.repo_page_stash_columns = RepoPageStashColumns::default()
            }
            Some(DropdownKind::ExplorerColumns) => {
                self.explorer_prefs.columns = crate::explorer::ExplorerColumns::default();
                if let Some(explorer) = self.explorer.as_mut() {
                    explorer.columns = self.explorer_prefs.columns;
                }
            }
            _ => return,
        }
        self.save_state();
    }

    /// Run a columns-dropdown footer action (the select/deselect-all + reset buttons).
    pub fn dropdown_run_action(&mut self, action: DropdownColAction) {
        match action {
            DropdownColAction::ToggleAll => self.dropdown_toggle_all_columns(),
            DropdownColAction::Reset => self.dropdown_reset_columns(),
        }
    }

    /// Activate the item at `index`: toggle a column (dropdown stays open) or set a sort (closes).
    /// Returns whether the dropdown should now close.
    pub fn dropdown_activate(&mut self, index: usize) -> bool {
        let Some(dropdown) = self.dropdown else {
            return true;
        };
        match dropdown.kind {
            DropdownKind::ListColumns => {
                if let Some((column, ..)) = LIST_COLS.get(index) {
                    self.toggle_column(*column);
                    self.save_state();
                }
                false
            }
            DropdownKind::ListSort => {
                if let Some((sort, ..)) = LIST_SORTS.get(index) {
                    self.set_sort(*sort);
                }
                true
            }
            DropdownKind::ListFilter => {
                if let Some((filter, ..)) = LIST_FILTERS.get(index) {
                    self.set_status_filter(*filter);
                }
                true
            }
            DropdownKind::PageColumns => {
                if let Some((column, ..)) = PAGE_COLS.get(index) {
                    self.toggle_repo_page_column(*column);
                    self.save_state();
                }
                false
            }
            DropdownKind::PageSort => {
                if let Some((sort, ..)) = PAGE_SORTS.get(index) {
                    self.set_repo_page_sort(*sort);
                }
                true
            }
            DropdownKind::StashColumns => {
                if let Some((column, ..)) = STASH_COLS.get(index) {
                    self.toggle_repo_page_stash_column(*column);
                    self.save_state();
                }
                false
            }
            DropdownKind::ExplorerColumns => {
                if let Some((column, ..)) = crate::explorer::ExplorerColumn::ALL.get(index) {
                    self.toggle_explorer_column(*column);
                }
                false
            }
            DropdownKind::ExplorerSort => {
                if let Some((key, ..)) = crate::explorer::SortKey::ALL.get(index) {
                    self.set_explorer_sort(*key);
                }
                true
            }
        }
    }
}
