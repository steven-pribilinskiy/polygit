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

impl AppState {
    /// The perf panel's menu rows: corner, graph metric, graph window, graph height, then reset.
    ///
    /// One flat radio list rather than a nested menu, because the whole panel has five settings and
    /// a submenu for each would cost more keystrokes than it saves. Mnemonics are unique across the
    /// whole list — the dropdown activates by letter and a duplicate would make one row unreachable.
    pub fn perf_menu_rows(&self) -> Vec<crate::app::DropdownItem> {
        use crate::app::DropdownItem;
        use crate::perf::{Corner, GraphPrefs};

        let placement = self.perf.placement;
        let graph = self.perf.graph;
        let mut rows: Vec<DropdownItem> = Vec::new();

        for (corner, mnemonic) in Corner::ALL.into_iter().zip(['q', 'w', 'a', 's']) {
            rows.push(DropdownItem {
                label: format!("corner · {}", corner.label()),
                on: placement.corner == corner,
                mnemonic,
                enabled: true,
            });
        }
        for (metric, mnemonic) in crate::perf::Metric::ALL.into_iter().zip(['f', 'h', 'l', 'r', 'b'])
        {
            rows.push(DropdownItem {
                label: format!("graph · {}", metric.label()),
                on: graph.metric == metric,
                mnemonic,
                enabled: true,
            });
        }
        for ((secs, label), mnemonic) in GraphPrefs::WINDOWS.into_iter().zip(['1', '2', '3', '4']) {
            rows.push(DropdownItem {
                label: format!("window · {label}"),
                on: graph.window_secs == secs,
                mnemonic,
                enabled: true,
            });
        }
        for (height, mnemonic) in GraphPrefs::HEIGHTS.into_iter().zip(['7', '8', '9']) {
            rows.push(DropdownItem {
                label: format!("height · {height} rows"),
                on: graph.rows == height,
                mnemonic,
                enabled: true,
            });
        }
        rows.push(DropdownItem {
            label: "set this corner as default".to_string(),
            on: placement.default_corner == placement.corner,
            mnemonic: 'd',
            enabled: true,
        });
        rows.push(DropdownItem {
            label: "reset position".to_string(),
            on: false,
            mnemonic: '0',
            enabled: true,
        });
        rows
    }

    /// Apply the menu row at `index`. Returns whether the menu should close.
    pub fn perf_menu_activate(&mut self, index: usize) -> bool {
        use crate::perf::{Corner, GraphPrefs, Metric};

        let corners = Corner::ALL.len();
        let metrics = Metric::ALL.len();
        let windows = GraphPrefs::WINDOWS.len();
        let heights = GraphPrefs::HEIGHTS.len();

        if index < corners {
            self.perf.placement.move_to_corner(Corner::ALL[index]);
        } else if index < corners + metrics {
            self.perf.graph.metric = Metric::ALL[index - corners];
        } else if index < corners + metrics + windows {
            self.perf.graph.window_secs = GraphPrefs::WINDOWS[index - corners - metrics].0;
        } else if index < corners + metrics + windows + heights {
            self.perf.graph.rows = GraphPrefs::HEIGHTS[index - corners - metrics - windows];
        } else if index == corners + metrics + windows + heights {
            self.perf.placement.default_corner = self.perf.placement.corner;
        } else {
            self.perf.placement.reset();
        }
        self.save_state();
        // Stay open: picking a corner and then a window is the common case, and a menu that closes
        // after every choice makes the second one cost a reopen.
        false
    }

    /// Open the panel's menu BESIDE the panel, never under it.
    ///
    /// The dropdown draws in the normal widget pass and the panel draws last — deliberately, so the
    /// panel's own cost stays out of the channels it reports — which means a menu overlapping the
    /// panel is painted over by it. Anchoring under the chip did exactly that and cut every label
    /// in half. The dropdown right-aligns to `anchor_right`, so anchoring to the panel's left edge
    /// places the whole menu to its left; when the panel is hard against the left of the screen
    /// there is no room there, so it goes to the right instead.
    pub fn open_perf_menu(&mut self) {
        let Some((row, _start, _end)) = self.perf_menu_click else {
            return;
        };
        let panel = self.perf_panel_rect;
        // Widest row is "corner · bottom right" plus the checkbox, mnemonic, padding and borders.
        const MENU_WIDTH: u16 = 32;
        let anchor_right = if panel.x >= MENU_WIDTH {
            panel.x
        } else {
            panel.x + panel.width + MENU_WIDTH
        };
        self.dropdown = Some(crate::app::Dropdown {
            kind: crate::app::DropdownKind::PerfPanel,
            anchor_right,
            anchor_row: row,
            selected: None,
        });
    }
}
