//! The diff modal: view mode, file selection, and filtering.

use super::*;

impl AppState {
    pub fn open_diff_modal(&mut self, source: DiffSource) {
        self.diff_modal = Some(DiffModal {
            source,
            mode: DiffMode::Uncommitted,
            view: self.diff_view,
            focus: DiffFocus::Files,
            files: Vec::new(),
            selected: 0,
            file_scroll: 0,
            lines: vec!["(loading…)".to_string()],
            scroll: 0,
            loading: true,
            diff_loading: true,
            status_filter: None,
        });
    }

    pub fn diff_modal_cycle_view(&mut self) {
        self.diff_view = self.diff_view.cycle();
        if let Some(modal) = self.diff_modal.as_mut() {
            modal.view = self.diff_view;
            modal.scroll = 0;
        }
        self.save_state();
    }

    pub fn diff_modal_toggle_focus(&mut self) {
        if let Some(modal) = self.diff_modal.as_mut() {
            modal.focus = match modal.focus {
                DiffFocus::Files => DiffFocus::Diff,
                DiffFocus::Diff => DiffFocus::Files,
            };
        }
    }

    pub fn diff_modal_toggle_mode(&mut self) -> bool {
        let Some(modal) = self.diff_modal.as_mut() else {
            return false;
        };
        if !matches!(modal.source, DiffSource::Dirty { .. }) {
            return false;
        }
        modal.mode = match modal.mode {
            DiffMode::Uncommitted => DiffMode::BaseBranch,
            DiffMode::BaseBranch => DiffMode::Uncommitted,
        };
        modal.files = Vec::new();
        modal.selected = 0;
        modal.file_scroll = 0;
        modal.lines = vec!["(loading…)".to_string()];
        modal.scroll = 0;
        modal.loading = true;
        modal.diff_loading = true;
        modal.status_filter = None;
        true
    }

    pub fn diff_modal_select(&mut self, delta: isize) -> bool {
        let viewport = self.diff_files_viewport;
        let Some(modal) = self.diff_modal.as_mut() else {
            return false;
        };
        let visible = modal.visible_file_indices();
        if visible.is_empty() {
            return false;
        }
        let pos = visible.iter().position(|&index| index == modal.selected).unwrap_or(0);
        let next_pos = (pos as isize + delta).clamp(0, visible.len() as isize - 1) as usize;
        let next = visible[next_pos];
        if next == modal.selected {
            return false;
        }
        modal.selected = next;
        modal.scroll = 0;
        modal.lines = vec!["(loading…)".to_string()];
        modal.diff_loading = true;
        Self::keep_file_selected_visible(modal, viewport);
        true
    }

    pub fn diff_modal_select_index(&mut self, pos: usize) -> bool {
        let viewport = self.diff_files_viewport;
        let Some(modal) = self.diff_modal.as_mut() else {
            return false;
        };
        let visible = modal.visible_file_indices();
        let Some(&next) = visible.get(pos) else {
            return false;
        };
        if next == modal.selected {
            return false;
        }
        modal.selected = next;
        modal.scroll = 0;
        modal.lines = vec!["(loading…)".to_string()];
        modal.diff_loading = true;
        Self::keep_file_selected_visible(modal, viewport);
        true
    }

    pub fn diff_modal_set_filter(&mut self, status: Option<char>) -> bool {
        let viewport = self.diff_files_viewport;
        let Some(modal) = self.diff_modal.as_mut() else {
            return false;
        };
        modal.status_filter = status;
        modal.file_scroll = 0;
        let visible = modal.visible_file_indices();
        if visible.contains(&modal.selected) {
            Self::keep_file_selected_visible(modal, viewport);
            return false;
        }
        let Some(&first) = visible.first() else {
            return false;
        };
        modal.selected = first;
        modal.scroll = 0;
        modal.lines = vec!["(loading…)".to_string()];
        modal.diff_loading = true;
        Self::keep_file_selected_visible(modal, viewport);
        true
    }

    pub fn diff_modal_cycle_filter(&mut self) -> bool {
        let Some(modal) = self.diff_modal.as_ref() else {
            return false;
        };
        if !modal.chips_active() {
            return false;
        }
        let chips: Vec<char> = modal.status_chips().into_iter().map(|(bucket, _)| bucket).collect();
        let next = match modal.status_filter {
            None => chips.first().copied(),
            Some(current) => {
                let pos = chips.iter().position(|&bucket| bucket == current);
                match pos {
                    Some(index) => chips.get(index + 1).copied(),
                    None => chips.first().copied(),
                }
            }
        };
        self.diff_modal_set_filter(next)
    }

    fn keep_file_selected_visible(modal: &mut DiffModal, viewport: usize) {
        if viewport == 0 {
            return;
        }
        let visible = modal.visible_file_indices();
        let Some(pos) = visible.iter().position(|&index| index == modal.selected) else {
            return;
        };
        if pos < modal.file_scroll {
            modal.file_scroll = pos;
        } else if pos >= modal.file_scroll + viewport {
            modal.file_scroll = pos + 1 - viewport;
        }
    }

    pub fn diff_files_scroll(&mut self, delta: isize) {
        let viewport = self.diff_files_viewport;
        if let Some(modal) = self.diff_modal.as_mut() {
            let max = modal.visible_file_indices().len().saturating_sub(viewport);
            let next = (modal.file_scroll as isize + delta).clamp(0, max as isize);
            modal.file_scroll = next as usize;
        }
    }

    pub fn diff_chip_at(&self, col: u16, row: u16) -> Option<Option<char>> {
        self.diff_chips_click
            .iter()
            .find(|(chip_row, start, end, _)| *chip_row == row && col >= *start && col < *end)
            .map(|(_, _, _, bucket)| *bucket)
    }

    pub fn diff_modal_file_at(&self, row: u16) -> Option<usize> {
        let modal = self.diff_modal.as_ref()?;
        let area = self.diff_files_area;
        if row < area.y || row >= area.y + area.height {
            return None;
        }
        let pos = (row - area.y) as usize + modal.file_scroll;
        (pos < modal.visible_file_indices().len()).then_some(pos)
    }
}
