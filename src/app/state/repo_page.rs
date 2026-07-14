//! The full-screen repo page: tabs, sections, sort, and columns.

use super::*;

impl AppState {
    pub fn open_repo_page(&mut self) {
        if let Some(idx) = self.selected_repo_index() {
            self.repo_page = Some(idx);
            self.repo_page_selected = 0;
            self.repo_page_scroll = 0;
            self.repo_page_message = None;
            self.repo_page_focus_head = true;
            self.repo_page_tab = RepoTab::Branches;
            self.focus = Pane::RepoPage;
            self.repos[idx].lock().unwrap().page = None;
        }
    }

    pub fn retarget_repo_page(&mut self, idx: usize) {
        if self.repo_page == Some(idx) {
            return;
        }
        self.repo_page = Some(idx);
        self.repo_page_selected = 0;
        self.repo_page_scroll = 0;
        self.repo_page_message = None;
        self.repo_page_focus_head = true;
        self.repo_page_tab = RepoTab::Branches;
    }

    pub fn focus_head_branch_if_pending(&mut self) {
        if !self.repo_page_focus_head {
            return;
        }
        let head = self.repo_page_rows().iter().position(|row| row.is_head);
        if let Some(index) = head {
            self.repo_page_selected = index;
            self.repo_page_focus_head = false;
        }
    }

    pub fn close_repo_page(&mut self) {
        self.repo_page = None;
        self.repo_page_message = None;
        if self.focus == Pane::RepoPage {
            self.focus = Pane::List;
        }
    }

    pub fn repo_page_rows(&self) -> Vec<PageRow> {
        let mut rows = Vec::new();
        let Some(idx) = self.repo_page else {
            return rows;
        };
        let state = self.repos[idx].lock().unwrap();
        let Some(page) = &state.page else {
            return rows;
        };
        let repo_path = state.path.clone();
        for branch in &page.branches {
            rows.push(PageRow {
                kind: PageRowKind::Branch,
                branch: branch.name.clone(),
                path: repo_path.clone(),
                deletable: branch.deletable(),
                is_head: branch.is_head,
                dirty: branch.is_head && page.head_dirty_count > 0,
                dirty_count: if branch.is_head { page.head_dirty_count } else { 0 },
                stash_index: None,
                ahead: branch.ahead,
                behind: branch.behind,
                upstream: branch.upstream.clone(),
                last_commit_rel: branch.last_commit_rel.clone(),
                last_commit_secs: branch.last_commit_secs,
                subject: branch.subject.clone(),
                stats: branch.stats,
                commit_sha: branch.commit_sha.clone(),
                author: branch.author.clone(),
                merge_base_short: branch.merge_base_short.clone(),
                base: branch.base.clone(),
                base_is_override: branch.base_is_override,
                parents: Vec::new(),
                upstream_gone: branch.upstream_gone,
            });
        }
        for worktree in &page.worktrees {
            let branch_info = page.branches.iter().find(|info| info.name == worktree.branch);
            let dirty_count = page
                .dirty_worktrees
                .iter()
                .find(|(path, _)| path == &worktree.path)
                .map_or(0, |(_, count)| *count);
            rows.push(PageRow {
                kind: PageRowKind::Worktree,
                branch: worktree.branch.clone(),
                path: worktree.path.clone(),
                deletable: false,
                is_head: false,
                dirty: dirty_count > 0,
                dirty_count,
                stash_index: None,
                ahead: branch_info.and_then(|info| info.ahead),
                behind: branch_info.and_then(|info| info.behind),
                upstream: branch_info.and_then(|info| info.upstream.clone()),
                last_commit_rel: branch_info
                    .map(|info| info.last_commit_rel.clone())
                    .unwrap_or_default(),
                last_commit_secs: branch_info.map(|info| info.last_commit_secs).unwrap_or(0),
                subject: String::new(),
                stats: branch_info.and_then(|info| info.stats),
                commit_sha: branch_info.map(|info| info.commit_sha.clone()).unwrap_or_default(),
                author: branch_info.map(|info| info.author.clone()).unwrap_or_default(),
                merge_base_short: branch_info.and_then(|info| info.merge_base_short.clone()),
                base: branch_info.and_then(|info| info.base.clone()),
                base_is_override: branch_info.is_some_and(|info| info.base_is_override),
                parents: Vec::new(),
                upstream_gone: false,
            });
        }
        for stash in &page.stashes {
            rows.push(PageRow {
                kind: PageRowKind::Stash,
                branch: stash.label.clone(),
                path: repo_path.clone(),
                deletable: false,
                is_head: false,
                dirty: false,
                dirty_count: 0,
                stash_index: Some(stash.index),
                ahead: None,
                behind: None,
                upstream: None,
                // A stash is a commit — reuse the row's commit-date fields for its creation time.
                last_commit_rel: stash.date_rel.clone(),
                last_commit_secs: stash.created_secs,
                subject: String::new(),
                stats: stash.stats,
                commit_sha: String::new(),
                author: String::new(),
                merge_base_short: None,
                base: None,
                base_is_override: false,
                parents: Vec::new(),
                upstream_gone: false,
            });
        }
        for commit in &page.commits {
            rows.push(PageRow {
                kind: PageRowKind::Commit,
                branch: String::new(),
                path: repo_path.clone(),
                deletable: false,
                is_head: false,
                dirty: false,
                dirty_count: 0,
                stash_index: None,
                ahead: None,
                behind: None,
                upstream: None,
                last_commit_rel: commit.rel_date.clone(),
                last_commit_secs: 0,
                subject: commit.subject.clone(),
                stats: None,
                commit_sha: commit.sha.clone(),
                author: commit.author.clone(),
                merge_base_short: None,
                base: None,
                base_is_override: false,
                parents: commit.parents.clone(),
                upstream_gone: false,
            });
        }
        // Sort the branch and worktree sections independently by the active column (stashes +
        // commits keep their natural recency order). `None` leaves git's order (HEAD first).
        if let Some(sort) = self.repo_page_sort {
            let dir = self.repo_page_sort_dir;
            let branch_count = page.branches.len();
            let worktree_count = page.worktrees.len();
            let order = |first: &PageRow, second: &PageRow| {
                let ord = repo_page_row_cmp(sort, first, second);
                if dir == SortDir::Desc { ord.reverse() } else { ord }
            };
            rows[..branch_count].sort_by(order);
            rows[branch_count..branch_count + worktree_count].sort_by(order);
        }
        // Tabbed mode ONLY: keep just the active tab's rows (so selection / clicks / nav scope to
        // it). When maximized the page is a single stacked view of every section, so it must keep
        // ALL rows — mirror `repo_page_tabbed`'s condition (which excludes maximized) inline here,
        // computed from the locked `page` + the lock-free `maximized` field to avoid re-locking.
        let present = u8::from(!page.branches.is_empty())
            + u8::from(!page.worktrees.is_empty())
            + u8::from(!page.stashes.is_empty())
            + u8::from(!page.commits.is_empty());
        // Mirror `repo_page_tabbed` from the locked `page` (avoid re-locking): maximized opts into
        // tabs only via `repo_page_maximized_tabbed`; restored honors the `v` override over auto.
        let tabbed = if self.maximized == Some(Pane::RepoPage) {
            self.repo_page_maximized_tabbed && self.repo_page_tabs == RepoTabsMode::Auto && present >= 2
        } else {
            self.repo_page_tabbed_override
                .unwrap_or(self.repo_page_tabs == RepoTabsMode::Auto && present >= 2)
        };
        if tabbed {
            match self.repo_page_tab.row_kind() {
                Some(kind) => rows.retain(|row| row.kind == kind),
                None => rows.clear(),
            }
        } else {
            // Flat (stacked) view: hide rows in collapsed sections (their header stays, so they can
            // be re-expanded). Headers aren't rows, so nav/selection skip the hidden rows.
            rows.retain(|row| !self.repo_page_collapsed_sections.contains(row.tab().section_name()));
        }
        rows
    }

    pub fn repo_page_section_counts(&self) -> (usize, usize, usize, usize) {
        let Some(idx) = self.repo_page else { return (0, 0, 0, 0) };
        let state = self.repos[idx].lock().unwrap();
        state.page.as_ref().map_or((0, 0, 0, 0), |page| {
            (page.branches.len(), page.worktrees.len(), page.stashes.len(), page.commits.len())
        })
    }

    pub fn repo_page_present_tabs(&self) -> Vec<RepoTab> {
        let (branches, worktrees, stashes, commits) = self.repo_page_section_counts();
        let mut tabs = Vec::new();
        if branches > 0 {
            tabs.push(RepoTab::Branches);
        }
        if worktrees > 0 {
            tabs.push(RepoTab::Worktrees);
        }
        if stashes > 0 {
            tabs.push(RepoTab::Stashes);
        }
        if commits > 0 {
            tabs.push(RepoTab::Commits);
        }
        tabs
    }

    pub fn repo_page_tabbed(&self) -> bool {
        // Restored: tabbed per `repo_page_tabs`. Maximized: flat stacked by default, tabbed only
        // when the `v` toggle (`repo_page_maximized_tabbed`) is on.
        if self.maximized == Some(Pane::RepoPage) {
            // Maximized stays flat-stacked by default; `v` (repo_page_maximized_tabbed) opts into tabs.
            return self.repo_page_maximized_tabbed
                && self.repo_page_tabs == RepoTabsMode::Auto
                && self.repo_page_present_tabs().len() >= 2;
        }
        // Restored: the auto decision (Auto mode + ≥2 sections), unless `v` set an explicit override.
        let auto = self.repo_page_tabs == RepoTabsMode::Auto && self.repo_page_present_tabs().len() >= 2;
        self.repo_page_tabbed_override.unwrap_or(auto)
    }

    pub fn repo_page_cols_dropdown_kind(&self) -> DropdownKind {
        if self.repo_page_tabbed() && self.repo_page_tab == RepoTab::Stashes {
            DropdownKind::StashColumns
        } else {
            DropdownKind::PageColumns
        }
    }

    pub fn repo_page_select_tab(&mut self, tab: RepoTab) {
        self.repo_page_tab = tab;
        self.repo_page_selected = 0;
        self.repo_page_scroll = 0;
    }

    pub fn repo_page_section_collapsed(&self, tab: RepoTab) -> bool {
        self.repo_page_collapsed_sections.contains(tab.section_name())
    }

    pub fn toggle_repo_page_section(&mut self, tab: RepoTab) {
        let name = tab.section_name().to_string();
        if !self.repo_page_collapsed_sections.remove(&name) {
            self.repo_page_collapsed_sections.insert(name);
        }
        let len = self.repo_page_selectable_len();
        if self.repo_page_selected >= len {
            self.repo_page_selected = len.saturating_sub(1);
        }
        self.save_state();
    }

    pub fn toggle_selected_repo_page_section(&mut self) {
        if let Some(row) = self.repo_page_target() {
            self.toggle_repo_page_section(row.tab());
        }
    }

    pub fn toggle_all_repo_page_sections(&mut self) {
        if self.repo_page_collapsed_sections.is_empty() {
            for tab in self.repo_page_present_tabs() {
                self.repo_page_collapsed_sections.insert(tab.section_name().to_string());
            }
        } else {
            self.repo_page_collapsed_sections.clear();
        }
        let len = self.repo_page_selectable_len();
        if self.repo_page_selected >= len {
            self.repo_page_selected = len.saturating_sub(1);
        }
        self.save_state();
    }

    pub fn repo_page_cycle_tab(&mut self, forward: bool) {
        let tabs = self.repo_page_present_tabs();
        if tabs.is_empty() {
            return;
        }
        let current = tabs.iter().position(|&tab| tab == self.repo_page_tab).unwrap_or(0);
        let next = if forward {
            (current + 1) % tabs.len()
        } else {
            (current + tabs.len() - 1) % tabs.len()
        };
        self.repo_page_select_tab(tabs[next]);
    }

    pub fn set_repo_page_sort(&mut self, sort: RepoPageSort) {
        let prev = self
            .repo_page_rows()
            .get(self.repo_page_selected)
            .map(|row| (row.kind, row.branch.clone(), row.stash_index));
        if self.repo_page_sort == Some(sort) {
            self.repo_page_sort_dir = self.repo_page_sort_dir.flip();
        } else {
            self.repo_page_sort = Some(sort);
            self.repo_page_sort_dir = SortDir::Asc;
        }
        if let Some(prev) = prev {
            let rows = self.repo_page_rows();
            if let Some(index) = rows
                .iter()
                .position(|row| (row.kind, row.branch.clone(), row.stash_index) == prev)
            {
                self.repo_page_selected = index;
            }
        }
    }

    pub fn repo_page_sort_at(&self, col: u16, row: u16) -> Option<RepoPageSort> {
        self.repo_page_sort_click
            .iter()
            .find(|(header_row, start, end, _)| *header_row == row && col >= *start && col < *end)
            .map(|(_, _, _, sort)| *sort)
    }

    pub fn toggle_repo_page_column(&mut self, column: RepoPageColumn) {
        let columns = &mut self.repo_page_columns;
        match column {
            RepoPageColumn::AheadBehind => columns.ahead_behind = !columns.ahead_behind,
            RepoPageColumn::Dirty => columns.dirty = !columns.dirty,
            RepoPageColumn::Added => columns.added = !columns.added,
            RepoPageColumn::Modified => columns.modified = !columns.modified,
            RepoPageColumn::Deleted => columns.deleted = !columns.deleted,
            RepoPageColumn::Total => columns.total = !columns.total,
            RepoPageColumn::Upstream => columns.upstream = !columns.upstream,
            RepoPageColumn::Base => columns.base = !columns.base,
            RepoPageColumn::Age => columns.age = !columns.age,
            RepoPageColumn::PullRequest => columns.pull_request = !columns.pull_request,
            RepoPageColumn::Subject => columns.subject = !columns.subject,
        }
    }

    pub fn repo_page_column_available(&self, column: RepoPageColumn) -> bool {
        let Some(idx) = self.repo_page else {
            return true;
        };
        let state = self.repos[idx].lock().unwrap();
        let Some(page) = state.page.as_ref() else {
            return true;
        };
        match column {
            RepoPageColumn::Age | RepoPageColumn::Subject | RepoPageColumn::Base => true,
            // The PR column only carries data when the repo's current branch has an open PR.
            RepoPageColumn::PullRequest => state.pr.is_some(),
            RepoPageColumn::AheadBehind | RepoPageColumn::Upstream => {
                page.branches.iter().any(|branch| branch.upstream.is_some())
            }
            RepoPageColumn::Dirty => {
                page.head_dirty_count > 0
                    || page.dirty_worktrees.iter().any(|(_, count)| *count > 0)
            }
            RepoPageColumn::Added
            | RepoPageColumn::Modified
            | RepoPageColumn::Deleted
            | RepoPageColumn::Total => page.branches.iter().any(|branch| match branch.stats {
                None => true,
                Some(stats) => match column {
                    RepoPageColumn::Added => stats.added > 0,
                    RepoPageColumn::Modified => stats.modified > 0,
                    RepoPageColumn::Deleted => stats.deleted > 0,
                    _ => stats.total() > 0,
                },
            }),
        }
    }

    pub fn effective_repo_page_columns(&self) -> RepoPageColumns {
        let columns = self.repo_page_columns;
        let on = |flag: bool, column: RepoPageColumn| flag && self.repo_page_column_available(column);
        RepoPageColumns {
            ahead_behind: on(columns.ahead_behind, RepoPageColumn::AheadBehind),
            dirty: on(columns.dirty, RepoPageColumn::Dirty),
            added: on(columns.added, RepoPageColumn::Added),
            modified: on(columns.modified, RepoPageColumn::Modified),
            deleted: on(columns.deleted, RepoPageColumn::Deleted),
            total: on(columns.total, RepoPageColumn::Total),
            upstream: on(columns.upstream, RepoPageColumn::Upstream),
            base: on(columns.base, RepoPageColumn::Base),
            age: columns.age,
            pull_request: on(columns.pull_request, RepoPageColumn::PullRequest),
            subject: columns.subject,
        }
    }

    pub fn diff_source_for_selected(&self) -> Option<DiffSource> {
        let row = self.repo_page_target()?;
        match row.kind {
            PageRowKind::Stash => Some(DiffSource::Stash {
                path: row.path,
                index: row.stash_index?,
                label: row.branch,
            }),
            // A dirty branch/worktree shows its uncommitted (toggle to base) diff; a clean one
            // shows what the branch added vs its base branch.
            PageRowKind::Branch | PageRowKind::Worktree if row.dirty => Some(DiffSource::Dirty {
                path: row.path,
                name: row.branch,
            }),
            PageRowKind::Branch | PageRowKind::Worktree => Some(DiffSource::Branch {
                path: row.path,
                name: row.branch,
            }),
            PageRowKind::Commit => Some(DiffSource::Commit {
                path: row.path,
                sha: row.commit_sha,
                label: row.subject,
            }),
        }
    }

    pub fn repo_page_selectable_len(&self) -> usize {
        self.repo_page_rows().len()
    }

    pub fn repo_page_target(&self) -> Option<PageRow> {
        self.repo_page_rows().into_iter().nth(self.repo_page_selected)
    }

    pub fn repo_page_row_at(&self, row: u16) -> Option<usize> {
        self.repo_page_click
            .iter()
            .find(|(click_row, _)| *click_row == row)
            .map(|(_, index)| *index)
    }
}
