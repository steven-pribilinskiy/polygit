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
            DropdownKind::PageColumns => PAGE_COLS.len(),
            DropdownKind::PageSort => PAGE_SORTS.len(),
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
        }
    }
}
