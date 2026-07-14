//! Pane focus/maximize and the draggable splitters.

use super::*;

impl AppState {
    pub fn cycle_info_layout(&mut self) {
        self.info_layout = self.info_layout.cycle();
        self.save_state();
    }

    pub const DEFAULT_SPLIT: f64 = 0.4;
    pub const MIN_SPLIT: f64 = 0.2;
    pub const MAX_SPLIT: f64 = 0.7;

    /// Nudge the split ratio by `delta`, clamped to the allowed range.
    pub fn adjust_split(&mut self, delta: f64) {
        self.split_ratio = (self.split_ratio + delta).clamp(Self::MIN_SPLIT, Self::MAX_SPLIT);
    }

    pub fn set_split_from_col(&mut self, col: u16) {
        if self.main_area.width == 0 {
            return;
        }
        let rel = f64::from(col.saturating_sub(self.main_area.x)) / f64::from(self.main_area.width);
        self.split_ratio = rel.clamp(Self::MIN_SPLIT, Self::MAX_SPLIT);
    }

    pub const DOCK_DEFAULT: f64 = 0.45;
    pub const DOCK_MIN: f64 = 0.2;
    pub const DOCK_MAX: f64 = 0.7;

    /// Set the docked-panel height ratio from an absolute screen row (mouse drag on the dock's
    /// top boundary): rows *below* the boundary become the dock.
    pub fn set_dock_from_row(&mut self, row: u16) {
        let area = self.dock_full_area;
        if area.height == 0 {
            return;
        }
        let below = (area.y + area.height).saturating_sub(row);
        let rel = f64::from(below) / f64::from(area.height);
        self.dock_ratio = rel.clamp(Self::DOCK_MIN, Self::DOCK_MAX);
    }

    pub const PREVIEW_SPLIT_DEFAULT: f64 = 0.4;
    pub const PREVIEW_SPLIT_MIN: f64 = 0.2;
    pub const PREVIEW_SPLIT_MAX: f64 = 0.8;

    /// Toggle the result/log panel (the bottom of the preview). Hidden, the info panel fills the
    /// pane (so it reads like the repo list). Persisted.
    pub fn toggle_result_panel(&mut self) {
        self.show_result_panel = !self.show_result_panel;
        self.show_toast(if self.show_result_panel {
            "result panel: shown"
        } else {
            "result panel: hidden"
        });
        self.save_state();
    }

    pub fn set_preview_split_from_row(&mut self, row: u16) {
        let area = self.preview_split_area;
        if area.height == 0 {
            return;
        }
        let above = row.saturating_sub(area.y);
        let rel = f64::from(above) / f64::from(area.height);
        self.preview_split_ratio = rel.clamp(Self::PREVIEW_SPLIT_MIN, Self::PREVIEW_SPLIT_MAX);
    }

    pub fn is_pane_available(&self, pane: Pane) -> bool {
        match pane {
            Pane::List | Pane::Result => true,
            Pane::Info => self.selected_repo_index().is_some(),
            Pane::RepoPage => self.repo_page.is_some(),
        }
    }

    pub fn active_pane(&self) -> Pane {
        match self.maximized {
            Some(pane) if self.is_pane_available(pane) => pane,
            _ => self.focus,
        }
    }

    pub fn toggle_maximized(&mut self, pane: Pane) {
        if !self.is_pane_available(pane) {
            return;
        }
        self.maximized = if self.maximized == Some(pane) { None } else { Some(pane) };
        self.focus = pane;
        self.save_state();
    }

    pub fn focus_or_maximize_pane(&mut self, pane: Pane) {
        if !self.is_pane_available(pane) {
            return;
        }
        if self.maximized.is_some() {
            self.maximized = Some(pane);
            self.focus = pane;
            self.save_state();
        } else {
            self.focus_pane(pane);
        }
    }

    pub fn visible_panes(&self) -> Vec<Pane> {
        if let Some(pane) = self.maximized {
            if self.is_pane_available(pane) {
                return vec![pane];
            }
        }
        let mut panes = vec![Pane::List];
        if self.info_pinned && !self.result_overlay && self.selected_repo_index().is_some() {
            panes.push(Pane::Info);
        }
        if self.show_result_panel {
            panes.push(Pane::Result);
        }
        if self.repo_page.is_some() {
            panes.push(Pane::RepoPage);
        }
        panes
    }

    pub fn cycle_focus(&mut self, forward: bool) {
        let panes = self.visible_panes();
        if panes.is_empty() {
            return;
        }
        let next = match panes.iter().position(|&pane| pane == self.focus) {
            Some(idx) => {
                let len = panes.len();
                if forward { (idx + 1) % len } else { (idx + len - 1) % len }
            }
            None => 0,
        };
        self.focus = panes[next];
    }

    pub fn focus_pane(&mut self, pane: Pane) {
        if self.visible_panes().contains(&pane) {
            self.focus = pane;
        }
    }

    pub fn title_button_hit(&self, col: u16, row: u16) -> bool {
        let hit = |region: Option<(u16, u16, u16)>| {
            region.is_some_and(|(button_row, start, end)| row == button_row && col >= start && col < end)
        };
        hit(self.repo_page_back_click)
            || hit(self.repo_page_window_click)
            || hit(self.page_cols_click)
            || hit(self.page_sort_click)
            || self.max_click.iter().any(|&(r, s, e, _)| row == r && col >= s && col < e)
            || self.info_click.iter().any(|&(r, s, e, _)| row == r && col >= s && col < e)
            || self.clickable.iter().any(|hit| hit.row == row && col >= hit.col_start && col < hit.col_end)
    }
}
