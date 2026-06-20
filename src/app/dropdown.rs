use super::*;

/// The toggleable list columns, in dropdown order: `(column, label)`.
const LIST_COLS: &[(Column, &str)] = &[
    (Column::Status, "status"),
    (Column::AheadBehind, "ahead/behind"),
    (Column::Dirty, "dirty"),
    (Column::LastCommit, "last commit"),
    (Column::Worktrees, "worktrees"),
    (Column::Branches, "branches"),
    (Column::Stashes, "stashes"),
    (Column::PulledCommits, "pulled"),
    (Column::PulledFiles, "changed"),
    (Column::PullRequest, "pull request"),
    (Column::Favorite, "favorite"),
];

/// The sortable list columns, in dropdown order: `(sort, label)`.
const LIST_SORTS: &[(SortColumn, &str)] = &[
    (SortColumn::Name, "name"),
    (SortColumn::Branch, "branch"),
    (SortColumn::Status, "status"),
    (SortColumn::AheadBehind, "ahead/behind"),
    (SortColumn::Dirty, "dirty"),
    (SortColumn::LastCommit, "last commit"),
    (SortColumn::Worktrees, "worktrees"),
    (SortColumn::Branches, "branches"),
    (SortColumn::Stashes, "stashes"),
    (SortColumn::PulledCommits, "pulled"),
    (SortColumn::PulledFiles, "changed"),
];

/// The toggleable repo-page columns, in dropdown order.
const PAGE_COLS: &[(RepoPageColumn, &str)] = &[
    (RepoPageColumn::AheadBehind, "ahead/behind"),
    (RepoPageColumn::Dirty, "dirty"),
    (RepoPageColumn::Added, "added"),
    (RepoPageColumn::Modified, "modified"),
    (RepoPageColumn::Deleted, "deleted"),
    (RepoPageColumn::Total, "total"),
    (RepoPageColumn::Upstream, "upstream"),
    (RepoPageColumn::Base, "base"),
    (RepoPageColumn::Age, "age"),
    (RepoPageColumn::PullRequest, "pr"),
    (RepoPageColumn::Subject, "subject"),
];

/// The sortable repo-page columns, in dropdown order.
const PAGE_SORTS: &[(RepoPageSort, &str)] = &[
    (RepoPageSort::Name, "name"),
    (RepoPageSort::AheadBehind, "ahead/behind"),
    (RepoPageSort::Dirty, "dirty"),
    (RepoPageSort::Added, "added"),
    (RepoPageSort::Modified, "modified"),
    (RepoPageSort::Deleted, "deleted"),
    (RepoPageSort::Total, "total"),
    (RepoPageSort::Upstream, "upstream"),
    (RepoPageSort::Base, "base"),
    (RepoPageSort::Age, "age"),
    (RepoPageSort::Subject, "subject"),
];

impl AppState {
    /// Open a header dropdown anchored at the chip's screen position.
    pub fn open_dropdown(&mut self, kind: DropdownKind, col: u16, row: u16) {
        self.dropdown = Some(Dropdown { kind, anchor_col: col, anchor_row: row, selected: 0 });
    }

    pub fn close_dropdown(&mut self) {
        self.dropdown = None;
    }

    /// The open dropdown's items as `(label, checked-or-active)`. Empty when none is open.
    pub fn dropdown_items(&self) -> Vec<(String, bool)> {
        let Some(dropdown) = self.dropdown else {
            return Vec::new();
        };
        match dropdown.kind {
            DropdownKind::ListColumns => LIST_COLS
                .iter()
                .map(|(column, label)| (label.to_string(), self.column_on(*column)))
                .collect(),
            DropdownKind::ListSort => LIST_SORTS
                .iter()
                .map(|(sort, label)| (label.to_string(), self.sort_column == *sort))
                .collect(),
            DropdownKind::PageColumns => PAGE_COLS
                .iter()
                .map(|(column, label)| (label.to_string(), self.repo_page_column_on(*column)))
                .collect(),
            DropdownKind::PageSort => PAGE_SORTS
                .iter()
                .map(|(sort, label)| (label.to_string(), self.repo_page_sort == Some(*sort)))
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

    /// Move the dropdown highlight by `delta`, clamped.
    pub fn dropdown_move(&mut self, delta: isize) {
        let len = self.dropdown_len();
        if let Some(dropdown) = self.dropdown.as_mut() {
            let next = (dropdown.selected as isize + delta).clamp(0, len.saturating_sub(1) as isize);
            dropdown.selected = next as usize;
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
                if let Some((column, _)) = LIST_COLS.get(index) {
                    self.toggle_column(*column);
                    self.save_state();
                }
                false
            }
            DropdownKind::ListSort => {
                if let Some((sort, _)) = LIST_SORTS.get(index) {
                    self.set_sort(*sort);
                }
                true
            }
            DropdownKind::PageColumns => {
                if let Some((column, _)) = PAGE_COLS.get(index) {
                    self.toggle_repo_page_column(*column);
                    self.save_state();
                }
                false
            }
            DropdownKind::PageSort => {
                if let Some((sort, _)) = PAGE_SORTS.get(index) {
                    self.set_repo_page_sort(*sort);
                }
                true
            }
        }
    }
}
