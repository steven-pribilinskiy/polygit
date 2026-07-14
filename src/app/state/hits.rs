//! Cross-cutting hit-test and tooltip helpers for the list/info panes.

use super::*;

impl AppState {
    pub fn info_action_at(&self, col: u16, row: u16) -> Option<InfoAction> {
        self.info_click
            .iter()
            .find(|(click_row, start, end, _)| *click_row == row && col >= *start && col < *end)
            .map(|(_, _, _, action)| action.clone())
    }

    pub fn toggle_info_expanded(&mut self, field: &str) {
        if !self.info_expanded.remove(field) {
            self.info_expanded.insert(field.to_string());
        }
    }

    pub fn list_selection_at(&self, col: u16, row: u16) -> Option<usize> {
        let area = self.list_rows_area;
        if area.width == 0 || area.height == 0 {
            return None;
        }
        if col < area.x || col >= area.x + area.width || row < area.y || row >= area.y + area.height
        {
            return None;
        }
        let row_idx = (row - area.y) as usize + self.list_offset;
        let rows = self.visible_rows();
        // The scroll area holds only repo/header/spacer rows now (the Result/Errors summary is a
        // pinned footer, hit-tested separately below).
        if row_idx < rows.len() {
            return match rows[row_idx] {
                // Static (small-group) headers, the favorites header, and spacers are inert.
                ListRow::GroupHeader { collapsible: false, .. }
                | ListRow::FavoritesHeader
                | ListRow::Spacer => None,
                _ => Some(row_idx),
            };
        }
        None
    }

    pub fn list_footer_selection_at(&self, col: u16, row: u16) -> Option<usize> {
        let area = self.list_footer_area;
        if area.width == 0
            || col < area.x
            || col >= area.x + area.width
            || row < area.y
            || row >= area.y + area.height
        {
            return None;
        }
        let rows = self.visible_rows();
        match row - area.y {
            1 => Some(rows.len()),                              // the "Result" summary line
            3 if self.has_errors() => Some(rows.len() + 1),     // the "Errors" line
            _ => None,
        }
    }

    pub fn scrollbar_at(&self, col: u16, row: u16) -> Option<ScrollKind> {
        self.scroll_hits
            .iter()
            .find(|hit| {
                if hit.total <= hit.viewport || hit.track.width == 0 {
                    return false;
                }
                if hit.track.height == 1 && hit.track.width > 1 {
                    // Horizontal bar: along the bottom row, any column within the track.
                    row == hit.track.y && col >= hit.track.x && col < hit.track.x + hit.track.width
                } else {
                    // Vertical bar: the rightmost column of the track.
                    col == hit.track.x + hit.track.width - 1
                        && row >= hit.track.y
                        && row < hit.track.y + hit.track.height
                }
            })
            .map(|hit| hit.kind)
    }

    pub fn scroll_value_for(&self, kind: ScrollKind, col: u16, row: u16) -> Option<usize> {
        let hit = self.scroll_hits.iter().find(|hit| hit.kind == kind)?;
        let horizontal = hit.track.height == 1 && hit.track.width > 1;
        let (rel, span) = if horizontal {
            (f64::from(col.saturating_sub(hit.track.x)), f64::from(hit.track.width.max(1)))
        } else {
            (f64::from(row.saturating_sub(hit.track.y)), f64::from(hit.track.height.max(1)))
        };
        let fraction = (rel / span).clamp(0.0, 1.0);
        let max_scroll = hit.total.saturating_sub(hit.viewport);
        Some(((fraction * hit.total as f64) as usize).min(max_scroll))
    }

    pub fn apply_scroll(&mut self, kind: ScrollKind, value: usize) -> bool {
        match kind {
            ScrollKind::List => {
                // Scroll the list view independently of the selection (render clamps to range).
                self.list_scroll = value;
                false
            }
            ScrollKind::Info => {
                if let Some(idx) = self.selected_repo_index() {
                    self.repos[idx].lock().unwrap().info_scroll = value;
                }
                false
            }
            ScrollKind::Preview => {
                if let Some(idx) = self.selected_repo_index() {
                    let mut state = self.repos[idx].lock().unwrap();
                    state.auto_scroll = false;
                    state.preview_scroll = value;
                }
                false
            }
            ScrollKind::DiffBody => {
                if let Some(modal) = self.diff_modal.as_mut() {
                    modal.scroll = value;
                }
                false
            }
            ScrollKind::DiffFiles => {
                // Scroll the file-list view independently of the selection (no diff reload).
                if let Some(modal) = self.diff_modal.as_mut() {
                    modal.file_scroll = value;
                }
                false
            }
            ScrollKind::Help => {
                self.help_scroll = value;
                false
            }
            ScrollKind::RepoPage => {
                self.repo_page_scroll = value;
                false
            }
            ScrollKind::Keyboard => {
                self.keyboard_scroll = value;
                false
            }
            ScrollKind::Settings => {
                self.settings_scroll = value;
                false
            }
            ScrollKind::Changelog => {
                self.changelog_scroll = value;
                false
            }
            ScrollKind::BuildInfo => {
                self.build_info_scroll = value;
                false
            }
            ScrollKind::PrModal => {
                if let Some(modal) = self.pr_modal.as_mut() {
                    modal.scroll = value;
                }
                false
            }
            ScrollKind::Keybindings => {
                // The editor's view follows the selection (ensure-visible). To keep a wheel/drag from
                // being snapped back, move the selection into the dragged viewport too.
                let layout = crate::keybindings::flat_layout();
                let viewport = (self.keybindings_inner.height.max(1)) as usize;
                let value = value.min(layout.len().saturating_sub(viewport));
                self.keybindings_scroll = value;
                let sel_flat = layout.iter().position(|row| *row == Some(self.keybindings_selected));
                if let Some(sel_flat) = sel_flat {
                    if sel_flat < value || sel_flat >= value + viewport {
                        let end = (value + viewport).min(layout.len());
                        if let Some(idx) = layout[value..end].iter().flatten().next() {
                            self.keybindings_selected = *idx;
                        }
                    }
                }
                false
            }
            ScrollKind::ExplorerList => {
                if let Some(explorer) = self.explorer.as_mut() {
                    explorer.list_scroll = value;
                }
                false
            }
            ScrollKind::ExplorerPreview => {
                if let Some(preview) = self.explorer.as_mut().and_then(|ex| ex.preview.as_mut()) {
                    preview.scroll = value;
                }
                false
            }
            ScrollKind::ExplorerPreviewH => {
                if let Some(explorer) = self.explorer.as_mut() {
                    explorer.preview_hscroll = value;
                }
                false
            }
        }
    }

    pub fn hint_at(&self, col: u16, row: u16) -> Option<HintKey> {
        self.hint_click
            .iter()
            .find(|hint| hint.row == row && col >= hint.col_start && col < hint.col_end)
            .map(|hint| hint.key)
    }

    pub fn command_at(&self, col: u16, row: u16) -> Option<Command> {
        self.clickable
            .iter()
            .find(|region| region.row == row && col >= region.col_start && col < region.col_end)
            .map(|region| region.command)
    }

    pub fn command_tooltip(&self, command: Command) -> String {
        let base = command.tooltip().to_string();
        let info = self.selected_repo_index().map(|idx| {
            let repo = self.repos[idx].lock().unwrap();
            (repo.remote_url.clone(), repo.path.display().to_string(), repo.pr.clone())
        });
        let line2: Option<String> = match command {
            Command::OpenRemote | Command::CopyRemote => Some(
                info.as_ref()
                    .and_then(|(remote, ..)| remote.clone())
                    .unwrap_or_else(|| "(no remote)".to_string()),
            ),
            Command::CopyPath => info.as_ref().map(|(_, path, _)| path.clone()),
            Command::OpenPr => Some(match info.as_ref().and_then(|(.., pr)| pr.clone()) {
                Some(pr) => format!("PR #{} — {}", pr.number, pr.title),
                None => "(no open PR detected)".to_string(),
            }),
            Command::OpenPrWeb => Some(match info.as_ref().and_then(|(.., pr)| pr.clone()) {
                Some(pr) => pr.url,
                None => "(opens the compare page for this branch)".to_string(),
            }),
            _ => None,
        };
        match line2 {
            Some(line2) => format!("{base}\n{line2}"),
            None => base,
        }
    }

    pub fn tooltip_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<(String, Rect, tui_pick::Placement, Option<Column>, TooltipArea)> {
        self.hover_tooltips
            .iter()
            .find(|region| region.row == row && col >= region.col_start && col < region.col_end)
            .map(|region| {
                (region.text.clone(), region.anchor, region.placement, region.hide_column, region.area)
            })
    }

    pub fn command_applicable(&self, command: Command) -> bool {
        match command {
            // Need a real repo row selected (not Result/Errors or a header).
            Command::Info
            | Command::DiffView
            | Command::CycleResultCategory
            | Command::OpenPage
            | Command::Claude
            | Command::Lazygit
            | Command::Explore
            | Command::OpenRemote
            | Command::CopyPath
            | Command::CopyRemote => self.selected_repo_index().is_some(),
            // Folding only applies in tree or grouped view.
            Command::NavLeft
            | Command::NavRight
            | Command::FoldCollapseAll
            | Command::FoldExpandAll
            | Command::FoldExpandSubtree => self.tree_active() || self.grouping_active(),
            // View toggles need their data to exist.
            Command::GroupingToggle => !self.groups.is_empty(),
            Command::TreeToggle => !self.tree_nodes.is_empty(),
            Command::FavoritesFirst => self.has_favorites(),
            // Selection moves need a non-empty list.
            Command::NavDown | Command::NavUp => !self.repos.is_empty(),
            // Retry/refetch reuse their existing no-op predicates.
            Command::Retry => self.selected_repo_retryable() || self.selected_header_retryable(),
            Command::RetryAll => self.any_retryable(),
            Command::Refetch => self.selected_repo_refetchable() || self.selected_header_refetchable(),
            Command::RefetchAll => self.any_refetchable(),
            // Everything else is always available (filters, sort, columns, resize, dock, focus,
            // result overlay, settings/help/quit, build info, menu items).
            _ => true,
        }
    }
}
