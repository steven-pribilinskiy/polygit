//! Result-pane view state: diff view and category tabs.

use super::*;

impl AppState {
    pub fn toggle_diff_view(&mut self) {
        let showing_diff_render =
            self.right_view == RightView::Diff && self.pane_diff_view != ResultDiffView::Log;
        let style = if showing_diff_render {
            ResultDiffView::Log
        } else if self.pane_diff_view == ResultDiffView::Log {
            ResultDiffView::Raw
        } else {
            self.pane_diff_view
        };
        self.apply_result_view(RightView::Diff, style);
    }

    pub fn cycle_result_diff_view(&mut self) {
        let style =
            if self.right_view == RightView::Diff { self.pane_diff_view.cycle() } else { self.pane_diff_view };
        self.apply_result_view(RightView::Diff, style);
    }

    pub fn cycle_result_category(&mut self) {
        let categories = self.visible_result_categories();
        let current = categories.iter().position(|&view| view == self.right_view).unwrap_or(0);
        let next = categories[(current + 1) % categories.len()];
        self.apply_result_view(next, self.pane_diff_view);
    }

    pub fn set_result_category(&mut self, view: RightView) {
        self.apply_result_view(view, self.pane_diff_view);
    }

    pub fn set_result_diff_view(&mut self, style: ResultDiffView) {
        self.apply_result_view(RightView::Diff, style);
    }

    pub fn visible_result_categories(&self) -> Vec<RightView> {
        let mut views = vec![RightView::Diff];
        let Some(repo_idx) = self.selected_repo_index() else {
            return views;
        };
        let state = self.repos[repo_idx].lock().unwrap();
        for view in [RightView::Tags, RightView::Branches, RightView::Commits, RightView::Files] {
            if self.result_category_count(&state, view) > 0 {
                views.push(view);
            }
        }
        views
    }

    pub fn result_category_count(&self, state: &RepoState, view: RightView) -> usize {
        // Every category is scoped to the LAST PULL's delta — tags/branches fetched, commits/files
        // delivered — so all four counts read straight off `pull_result` (no separate fetch, and
        // an up-to-date pull shows none of them). The repo page (`[4]`) holds the full inventory.
        let pull = state.pull_result.as_ref();
        match view {
            RightView::Diff => 0,
            RightView::Tags => pull.map_or(0, |result| result.new_tag_names.len()),
            RightView::Branches => pull.map_or(0, |result| result.fetched_branches.len()),
            RightView::Commits => pull.map_or(0, |result| result.commits as usize),
            RightView::Files => pull.map_or(0, |result| result.files as usize),
        }
    }

    fn apply_result_view(&mut self, view: RightView, style: ResultDiffView) {
        let showing_diff_render_before =
            self.right_view == RightView::Diff && self.pane_diff_view != ResultDiffView::Log;
        let showing_diff_render_after = view == RightView::Diff && style != ResultDiffView::Log;
        self.right_view = view;
        self.pane_diff_view = style;
        if let Some(repo_idx) = self.selected_repo_index() {
            let mut state = self.repos[repo_idx].lock().unwrap();
            if view != RightView::Diff || style != ResultDiffView::Log {
                // Any non-log tab (tags/branches/commits/files/diff, or a diff style change)
                // starts at the top, not the log's scroll / auto-scroll tail.
                state.preview_scroll = 0;
                state.auto_scroll = false;
            }
            if showing_diff_render_before && !showing_diff_render_after {
                state.diff = None;
            }
        }
        self.save_state();
    }
}
