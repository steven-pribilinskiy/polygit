//! Read-only status/pull queries plus theme and icon lookups.

use super::*;

impl AppState {
    pub fn effective_jobs(&self) -> usize {
        self.throttle.effective()
    }

    pub fn icons(&self) -> &'static IconSet {
        self.icon_style.icons()
    }

    pub fn dark_active(&self) -> bool {
        match self.theme {
            Theme::Auto => self.auto_dark,
            Theme::Dark => true,
            Theme::Light => false,
        }
    }

    pub fn palette(&self) -> crate::theme::Palette {
        crate::theme::palette(self.dark_active(), self.background, self.contrast)
    }

    pub fn list_len(&self) -> usize {
        self.visible_rows().len() + 1 + usize::from(self.has_errors())
    }

    pub fn counts(&self) -> (usize, usize, usize, usize, usize, usize, usize, usize) {
        let mut queued = 0;
        let mut running = 0;
        let mut updated = 0;
        let mut up_to_date = 0;
        let mut skipped = 0;
        let mut failed = 0;
        let mut no_upstream = 0;
        let mut throttled = 0;
        for repo in &self.repos {
            let state = repo.lock().unwrap();
            if state.hidden {
                continue;
            }
            match &state.status {
                RepoStatus::Queued => queued += 1,
                RepoStatus::Running { .. } => running += 1,
                RepoStatus::Updated => updated += 1,
                RepoStatus::UpToDate => up_to_date += 1,
                RepoStatus::NoUpstream => no_upstream += 1,
                RepoStatus::Skipped => skipped += 1,
                RepoStatus::Throttled => throttled += 1,
                RepoStatus::Failed => failed += 1,
            }
        }
        (queued, running, updated, up_to_date, skipped, failed, no_upstream, throttled)
    }

    pub fn done_count(&self) -> usize {
        let (_, _, updated, up_to_date, skipped, failed, no_upstream, throttled) = self.counts();
        updated + up_to_date + skipped + failed + no_upstream + throttled
    }

    pub fn has_errors(&self) -> bool {
        self.counts().5 > 0
    }

    pub fn retryable_repos(&self) -> Vec<usize> {
        self.repos
            .iter()
            .enumerate()
            .filter(|(_, repo)| repo.lock().unwrap().status.is_retryable())
            .map(|(index, _)| index)
            .collect()
    }

    pub fn refetchable_repos(&self) -> Vec<usize> {
        self.repos
            .iter()
            .enumerate()
            .filter(|(_, repo)| !repo.lock().unwrap().status.is_running())
            .map(|(index, _)| index)
            .collect()
    }

    pub fn should_auto_pull(&self, repo_count: usize) -> bool {
        self.auto_pull_on_launch
            && (self.auto_pull_max_repos == 0 || repo_count <= self.auto_pull_max_repos as usize)
            && (self.auto_pull_in_tree || !self.tree_active())
    }

    pub fn branch_check_interval_secs(repo_count: usize) -> u64 {
        ((repo_count as u64) / 10).clamp(1, 60)
    }

    pub fn any_pull_running(&self) -> bool {
        self.repos.iter().any(|repo| repo.lock().unwrap().status.is_running())
    }

    fn selected_status_matches(&self, predicate: impl Fn(&RepoStatus) -> bool) -> bool {
        self.selected_repo_index()
            .is_some_and(|index| predicate(&self.repos[index].lock().unwrap().status))
    }

    pub fn selected_repo_retryable(&self) -> bool {
        self.selected_status_matches(RepoStatus::is_retryable)
    }

    pub fn selected_repo_refetchable(&self) -> bool {
        self.selected_status_matches(|status| !status.is_running())
    }

    pub fn selected_header_repos(&self) -> Option<Vec<usize>> {
        match self.selected_row()? {
            ListRow::FolderHeader { node_idx, .. } => Some(self.tree_subtree_repos(node_idx)),
            ListRow::GroupHeader { group_idx, .. } => Some(self.group_visible_members(group_idx)),
            _ => None,
        }
    }

    pub fn selected_header_retryable(&self) -> bool {
        self.selected_header_repos().is_some_and(|repos| {
            repos.iter().any(|&idx| self.repos[idx].lock().unwrap().status.is_retryable())
        })
    }

    pub fn selected_header_refetchable(&self) -> bool {
        self.selected_header_repos().is_some_and(|repos| {
            repos.iter().any(|&idx| !self.repos[idx].lock().unwrap().status.is_running())
        })
    }

    pub fn any_retryable(&self) -> bool {
        self.repos
            .iter()
            .any(|repo| repo.lock().unwrap().status.is_retryable())
    }

    pub fn any_refetchable(&self) -> bool {
        self.repos
            .iter()
            .any(|repo| !repo.lock().unwrap().status.is_running())
    }
}
