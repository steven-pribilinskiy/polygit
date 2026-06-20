
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{
    AppState, ClickRegion, Column, ColumnFlags, Command, DiffFocus, DiffMode, DiffSource,
    DropdownKind, HelpTab, HintClick, HintKey, IconSet, InfoAction, Leader, ListRow, PageRow,
    PageRowKind, Pane, RepoPageColumn, RepoPageSort, RepoState, RepoStatus, RightView, ScrollHit,
    ScrollKind, SortColumn, SortDir, StatusFilter,
};

/// The published documentation site (opened by the `D` hotkey and linked in the help modal).
pub const DOCS_URL: &str = "https://steven-pribilinskiy.github.io/polygit/";

mod list;
mod preview;
mod status_bar;
mod help;
mod repo_page;
mod modals;
use list::*;
use preview::*;
use status_bar::*;
use help::*;
use repo_page::*;
use modals::*;

#[cfg(test)]
mod tests;
/// A repo-page list entry: the rendered line, an optional selectable-row index, and the optional
/// `base` cell column range (start, end relative to the line start) for click hit-testing.
type PageItem = (Line<'static>, Option<usize>, Option<(u16, u16)>);

/// The spinner frame for the current render tick (advances every 2 ticks). Shared by the
/// list status glyph and the repo-page loading indicator so they animate identically.
fn spinner_frame(tick: u64, icons: &IconSet) -> &'static str {
    icons.spinner[(tick as usize / 2) % icons.spinner.len()]
}

/// Border color for a main pane: a bright accent when it's the focused pane, dim otherwise.
fn pane_border_style(active: bool, modal_open: bool) -> Style {
    if modal_open {
        // A modal overlays the panes — recede all pane borders so the modal is the focus.
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)
    } else if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Title style for the main panes: dim while a modal overlays them, so the background chrome
/// recedes. (Pane titles are plain strings, so a base `title_style` dims them wholesale.)
fn pane_title_style(modal_open: bool) -> Style {
    if modal_open {
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM)
    } else {
        Style::default()
    }
}

/// Borders for the two main panes (and the info panel): all sides, or none when the user turns
/// borders off (the panes' inner areas then expand to reclaim the border cells).
fn pane_borders(app: &AppState) -> Borders {
    if app.show_borders {
        Borders::ALL
    } else {
        Borders::NONE
    }
}

/// Remap every cell's ANSI-palette colors to the active theme + contrast RGB palette.
/// Runs once per frame after all widgets are drawn — draw code keeps using the semantic
/// ANSI colors (`Color::Cyan`, `Color::DarkGray`, …) and this pass resolves them, so the
/// app looks identical in every terminal regardless of the terminal's own palette.
fn apply_palette(frame: &mut Frame, palette: &crate::theme::Palette) {
    for cell in frame.buffer_mut().content.iter_mut() {
        cell.fg = palette.map_fg(cell.fg);
        cell.bg = palette.map_bg(cell.bg);
        // Materialize DIM (disabled/no-op hints): terminals render the attribute
        // inconsistently, so fade the foreground toward the background instead. On a light
        // background the faint fg already sits close to the bg, so fade it less — a 0.7 fade
        // there washes disabled hints out to near-invisible.
        if cell.modifier.contains(Modifier::DIM) {
            if let (Color::Rgb(..), Color::Rgb(bg_r, bg_g, bg_b)) = (cell.fg, cell.bg) {
                let light_bg = u16::from(bg_r) + u16::from(bg_g) + u16::from(bg_b) > 3 * 140;
                let amount = if light_bg { 0.4 } else { 0.7 };
                cell.fg = crate::theme::blend_toward(cell.fg, cell.bg, amount);
                cell.modifier.remove(Modifier::DIM);
            }
        }
    }
}

/// Paint a subtle hover background over the actionable element under the cursor (status-bar
/// commands, footer hints, table-sort headers, column chips, info links/copy buttons, settings
/// options, keyboard keys, scrollbars, the splitter, and main-list rows). Runs after the palette
/// pass; only does anything when `hover_effects` is on (then `app.hover` carries the cursor).
fn apply_hover(frame: &mut Frame, app: &AppState, palette: &crate::theme::Palette) {
    let Some((hcol, hrow)) = app.hover else {
        return;
    };
    // While dragging the splitter or a scrollbar, suppress hover — the drag has its own feedback
    // and a moving highlight under the cursor is just noise.
    if app.divider_dragging || app.scrollbar_dragging.is_some() {
        return;
    }
    // Three hover tints, all derived from the palette so one edit propagates everywhere (and they
    // stay correct in Terminal-bg mode, which has no live RGB surface):
    //  - `hover_bg`         : a hovered, unselected row (subtle).
    //  - `selection_hover_bg`: the selected row while hovered (distinct — deeper than the selection,
    //                          so it never washes out into the plain hover tint).
    let hover_bg = palette.hover_bg();
    let selection_hover_bg = match app.selection_style {
        crate::app::SelectionStyle::Subtle => palette.subtle_selection_hover_bg(),
        crate::app::SelectionStyle::Blue => palette.selection_hover_bg(),
    };
    let contains = |row: u16, start: u16, end: u16| hrow == row && hcol >= start && hcol < end;
    let row_rect =
        |row: u16, start: u16, end: u16| Rect { x: start, y: row, width: end.saturating_sub(start), height: 1 };
    let inner_row = |area: Rect| Rect { x: area.x + 1, y: hrow, width: area.width.saturating_sub(2), height: 1 };
    // A scroll track spans the full pane width (for wheel hit-testing), so highlighting the whole
    // track on hover tints the entire pane. Only the scrollbar column (the draggable bar) should
    // react, and only when the pane actually overflows.
    let scrollbar_col_hit = || -> Option<Rect> {
        app.scroll_hits.iter().find_map(|hit| {
            let bar_col = hit.track.x + hit.track.width.saturating_sub(1);
            (hit.total > hit.viewport
                && hcol == bar_col
                && hrow >= hit.track.y
                && hrow < hit.track.y + hit.track.height)
                .then_some(Rect { x: bar_col, y: hit.track.y, width: 1, height: hit.track.height })
        })
    };

    // Only the foreground's OWN regions are considered — every modal/view registers click regions
    // into shared vecs, so gathering them all lets a large modal's background bleed through. The
    // first match in each branch wins; for command/hint chrome we highlight every span that shares
    // the hovered one's action (so a key and its label light up together).
    // Three buckets:
    //  - `hits`        : row-type hovers (list rows, file/menu rows, scrollbars, divider, headers) —
    //                    always a soft background tint, regardless of the button-hover setting.
    //  - `strong_hits` : the selected row while hovered — the deeper selection tint.
    //  - `button_hits` : button-type hovers (footer/modal hint chips, tabs, radio chips, close
    //                    buttons, keyboard keys, info-panel links) — painted per `button_hover_style`
    //                    (reverse-video when Inverted, the same soft tint when Subtle).
    let mut hits: Vec<Rect> = Vec::new();
    let mut strong_hits: Vec<Rect> = Vec::new();
    let mut button_hits: Vec<Rect> = Vec::new();
    // Footer status-bar commands stay clickable over any modal (only settings/help/quit keep a
    // region there). Check them first, everywhere — so the live footer reacts to hover even with a
    // modal on top, where the per-modal branches below only inspect that modal's own regions.
    if let Some(region) = app.clickable.iter().find(|c| contains(c.row, c.col_start, c.col_end)) {
        for sibling in app.clickable.iter().filter(|c| c.command == region.command) {
            button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
        }
    } else if app.confirm.is_some() {
        if let Some(region) = app.clickable.iter().find(|c| contains(c.row, c.col_start, c.col_end)) {
            button_hits.push(row_rect(region.row, region.col_start, region.col_end));
        }
    } else if app.show_settings {
        if let Some(&(row, start, end, ..)) =
            app.settings_click.iter().find(|&&(r, s, e, ..)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, tab)) =
            app.settings_tab_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // The active tab keeps its highlight (no hover tint over it).
            if tab != app.settings_tab {
                button_hits.push(row_rect(row, start, end));
            }
        } else if let Some(&(row, start, end, section)) =
            app.settings_section_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // Accordion header chips tint on hover like the tab buttons; the active one keeps its
            // solid highlight (no extra tint).
            if app.settings_on_header != Some(section) {
                button_hits.push(row_rect(row, start, end));
            }
        } else if let Some((row, start, end)) =
            app.settings_collapse_all_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.settings_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.settings_search_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        }
    } else if app.show_keyboard {
        if let Some(&(_, _, _, code)) =
            app.keyboard_key_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // Highlight the whole key cell, not just the hovered row: a boxed key spans 3 screen
            // rows (╭─╮ / │…│ / ╰─╯), each registered under the same key code.
            for &(row, start, end, _) in
                app.keyboard_key_click.iter().filter(|&&(_, _, _, c)| c == code)
            {
                button_hits.push(row_rect(row, start, end));
            }
        } else if let Some((row, start, end)) =
            app.keyboard_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        }
    } else if app.show_help {
        if let Some(&(row, start, end, tab)) =
            app.help_tab_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // The active tab keeps its active color on hover (no hover tint over it).
            if tab != app.help_tab {
                button_hits.push(row_rect(row, start, end));
            }
        } else if let Some((row, start, end)) =
            app.help_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.help_keyboard_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.help_maximize_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.cli_copy_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, ..)) =
            app.help_design_click.iter().find(|&&(r, s, e, ..)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if app.help_links.iter().any(|&(row, _)| row == hrow)
            || app.help_notes_toggle_row == Some(hrow)
            || app.cli_flag_click.iter().any(|&(row, _)| row == hrow)
        {
            // A full-width in-text link row — a tint reads better here than reverse-video.
            hits.push(inner_row(app.help_area));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        }
    } else if app.diff_modal.is_some() {
        if let Some((row, start, end)) =
            app.diff_modal_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some(scroll) =
            scrollbar_col_hit()
        {
            hits.push(scroll);
        } else if let Some(idx) = app.diff_modal_file_at(hrow) {
            let rect = inner_row(app.diff_modal_area);
            if app.diff_modal.as_ref().is_some_and(|modal| modal.selected == idx) {
                strong_hits.push(rect);
            } else {
                hits.push(rect);
            }
        }
    } else if app.copy_menu.is_some() {
        if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) =
            app.copy_menu_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if app.copy_menu_click.iter().any(|&(row, _)| row == hrow) {
            hits.push(inner_row(app.copy_menu_area));
        }
    } else if app.base_picker.is_some() {
        if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) =
            app.base_picker_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        }
    } else if app.show_build_info {
        if let Some((row, start, end)) =
            app.build_info_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        }
    } else if app.repo_page.is_some() {
        if let Some(&(row, start, end, _)) =
            app.repo_page_tab_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.repo_page_sort_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.repo_page_toggle_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.base_cell_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) =
            app.repo_page_window_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.repo_page_back_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(scroll) =
            scrollbar_col_hit()
        {
            hits.push(scroll);
        }
        // No body-row hover tint on the repo page: a full-width row following the cursor reads as
        // the whole page tinting. Only the controls above stay reactive.
    } else {
        // Main two-pane view. (Footer status-bar commands are handled by the top-level check above.)
        if let Some(column) = app.header_sort_at(hcol, hrow) {
            // A sortable list column header cell — highlight it across the header's rows (a wide,
            // multi-row cell reads better tinted than reverse-video).
            if let Some(&(start, end, _)) =
                app.header_click.iter().find(|&&(s, e, c)| c == column && hcol >= s && hcol < e)
            {
                let header = app.header_area;
                for row in header.y..header.y + header.height {
                    hits.push(row_rect(row, start, end));
                }
            }
        } else if let Some(&(row, start, end, _)) =
            app.info_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) = app
            .pr_cell_click
            .iter()
            .find(|(r, s, e, _)| contains(*r, *s, *e))
            .map(|&(row, start, end, _)| (row, start, end))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(scroll) =
            scrollbar_col_hit()
        {
            hits.push(scroll);
        } else if (i32::from(hcol) - i32::from(app.divider_col)).abs() <= 1
            && hrow >= app.main_area.y
            && hrow < app.main_area.y + app.main_area.height
        {
            hits.push(Rect { x: app.divider_col, y: app.main_area.y, width: 1, height: app.main_area.height });
        } else if let Some(idx) = app.list_selection_at(hcol, hrow) {
            // Any selectable list row — repo/group/folder rows plus the Result/Errors summary
            // rows. Hovering the *selected* row gets the stronger tint so it stays distinct
            // instead of washing out.
            let rect = Rect {
                x: app.list_area.x,
                y: hrow,
                width: app.divider_col.saturating_sub(app.list_area.x),
                height: 1,
            };
            if idx == app.selected {
                strong_hits.push(rect);
            } else {
                hits.push(rect);
            }
        }
    }

    let button_style = match app.button_hover_style {
        crate::app::ButtonHoverStyle::Inverted => Style::default().add_modifier(Modifier::REVERSED),
        crate::app::ButtonHoverStyle::Subtle => Style::default().bg(hover_bg),
    };
    let frame_area = frame.area();
    let buf = frame.buffer_mut();
    for rect in hits {
        buf.set_style(rect.intersection(frame_area), Style::default().bg(hover_bg));
    }
    for rect in strong_hits {
        buf.set_style(rect.intersection(frame_area), Style::default().bg(selection_hover_bg));
    }
    for rect in button_hits {
        buf.set_style(rect.intersection(frame_area), button_style);
    }
}

/// The background+text style for the selected row, per the user's `Selection` setting:
/// **Blue** = a solid blue bar with white text (high contrast, overrides column colors);
/// **Subtle** = a soft tint that keeps each column's own color readable. Bold either way.
fn selection_highlight_style(app: &AppState) -> Style {
    let palette = app.palette();
    match app.selection_style {
        crate::app::SelectionStyle::Blue => Style::default()
            .bg(palette.selection_bg)
            .fg(palette.selection_fg)
            .add_modifier(Modifier::BOLD),
        crate::app::SelectionStyle::Subtle => {
            Style::default().bg(palette.subtle_selection_bg()).add_modifier(Modifier::BOLD)
        }
    }
}

/// 1-cell inner padding for every bordered panel/modal when the setting is on; none otherwise.
fn panel_pad(app: &AppState) -> Padding {
    if app.panel_padding {
        Padding::uniform(1)
    } else {
        Padding::ZERO
    }
}

/// Pad `s` with trailing spaces until its display width reaches `width` (width-aware so
/// double-width emoji glyphs don't shift the columns that follow).
fn pad_display(s: &str, width: usize) -> String {
    let current = UnicodeWidthStr::width(s);
    if current >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - current))
    }
}

/// Tri-state text for a count cell, plus whether it should render dim. `None` = still loading
/// (`…`); `Some(0)` = a dim `{glyph}0` (visible zero, not a blank); `Some(n)` = `{glyph}n`.
fn count_cell_text(glyph: &str, count: Option<u32>) -> (String, bool) {
    match count {
        None => ("…".to_string(), true),
        Some(0) => (format!("{glyph}0"), true),
        Some(positive) => (format!("{glyph}{positive}"), false),
    }
}

/// Whether a list count cell should be hidden entirely (rendered blank): a zero count when emoji
/// is active (a colorful glyph beside `0` is clutter) OR the explicit "hide zero values" setting is
/// on. Otherwise a zero renders as a dim `{glyph}0`.
fn count_cell_hidden(emoji: bool, hide_zero: bool, count: Option<u32>) -> bool {
    (emoji || hide_zero) && count == Some(0)
}

/// A padded count-cell span: `color` when positive, dim gray when zero or still loading.
/// Used where no flash animation applies (the repo page); the root list inlines
/// `count_cell_text` so it can keep its flash wrapper.
fn count_cell(glyph: &str, count: Option<u32>, width: usize, color: Color) -> Span<'static> {
    let (text, dim) = count_cell_text(glyph, count);
    let style = if dim {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(color)
    };
    Span::styled(format!(" {}", pad_display(&text, width)), style)
}

fn status_glyph_colored(status: &RepoStatus, tick: u64, icons: &IconSet) -> Span<'static> {
    match status {
        RepoStatus::Queued => Span::styled(icons.queued, Style::default().fg(Color::DarkGray)),
        RepoStatus::Running { .. } => {
            Span::styled(spinner_frame(tick, icons).to_string(), Style::default().fg(Color::Yellow))
        }
        RepoStatus::UpToDate => Span::styled(icons.up_to_date, Style::default().fg(Color::Gray)),
        RepoStatus::Updated => Span::styled(icons.updated, Style::default().fg(Color::Green)),
        RepoStatus::NoUpstream => {
            Span::styled(icons.no_upstream, Style::default().fg(Color::DarkGray))
        }
        RepoStatus::Skipped => Span::styled(icons.skipped, Style::default().fg(Color::DarkGray)),
        RepoStatus::Throttled => {
            Span::styled(icons.throttled, Style::default().fg(Color::Magenta))
        }
        RepoStatus::Failed => Span::styled(icons.failed, Style::default().fg(Color::Red)),
    }
}

fn truncate_str(s: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(s) <= max_width {
        s.to_string()
    } else {
        let mut result = String::new();
        let mut width = 0;
        for ch in s.chars() {
            let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            if width + char_width + 1 > max_width {
                result.push('…');
                break;
            }
            result.push(ch);
            width += char_width;
        }
        result
    }
}

/// Truncate from the *left*, keeping the tail (a leading `…`). For file paths the filename at
/// the end is the informative part, so `…features/Foo.tsx` beats `src/features/Fo…`.
fn truncate_left(s: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(s) <= max_width {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut tail: Vec<char> = Vec::new();
    let mut width = 0;
    for &ch in chars.iter().rev() {
        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if width + char_width + 1 > max_width {
            break;
        }
        tail.push(ch);
        width += char_width;
    }
    tail.reverse();
    let mut result = String::from('…');
    result.extend(tail);
    result
}

/// Render a single frame into `frame`: draw every widget with semantic ANSI colors, then
/// remap the whole buffer to the active theme + contrast palette.
pub fn render(frame: &mut Frame, app: &mut AppState, tick: u64) {
    render_widgets(frame, app, tick);
    render_tooltip(frame, app);
    let palette = app.palette();
    apply_palette(frame, &palette);
    apply_hover(frame, app, &palette);
}

/// Render the active dwell tooltip (a small bordered popup), placed by the floating engine relative
/// to its anchor — flipping to the opposite side and shifting along the cross axis to stay on-screen
/// (e.g. a column header drops below, flipping above when cramped). Drawn before the palette pass so
/// its semantic colors remap.
fn render_tooltip(frame: &mut Frame, app: &mut AppState) {
    app.tooltip_hide_click = None;
    app.tooltip_rect = Rect::default();
    let Some(tip) = app.hover_tooltip.clone() else {
        return;
    };
    let area = frame.area();
    if area.width < 6 || area.height < 3 {
        return;
    }
    // A `[x]` hide-column button trails the text when the tooltip is for an optional column.
    let x_label = " [x]";
    let text_width = UnicodeWidthStr::width(tip.text.as_str()) as u16;
    let extra = if tip.hide_column.is_some() { x_label.len() as u16 } else { 0 };
    // border (2) + 1-cell horizontal padding (2) around the text (+ the optional `[x]`).
    let width = (text_width + extra + 4).min(area.width);
    let height = 3;
    let rect = tui_pick::position(
        tip.anchor,
        (width, height),
        area,
        tip.placement,
        tui_pick::PositionOptions { offset: 0, flip: true, shift: true },
    )
    .rect;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    cast_shadow(frame, rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    app.tooltip_rect = rect;
    if let Some(column) = tip.hide_column {
        let line = Line::from(vec![
            Span::raw(tip.text.clone()),
            Span::styled(x_label, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        ]);
        frame.render_widget(Paragraph::new(line), inner);
        // The `[x]` sits after the text + a leading space (3 cells wide).
        let x_start = inner.x + text_width + 1;
        app.tooltip_hide_click = Some((inner.y, x_start, x_start + 3, column));
    } else {
        frame.render_widget(Paragraph::new(tip.text.clone()), inner);
    }
}

/// Draw all widgets for the current state (colors still in the semantic ANSI palette).
fn render_widgets(frame: &mut Frame, app: &mut AppState, tick: u64) {
    let area = frame.area();
    // Draggable scrollbars and clickable hint regions are re-registered every frame by
    // whatever panels are visible (status bar, preview footer, …).
    app.scroll_hits.clear();
    app.clickable.clear();
    app.hint_click.clear();

    // A maximized repo page is full-screen and replaces the normal layout. A restored one falls
    // through to render as a docked bottom panel below the two panes (panel [4]).
    if app.repo_page.is_some() && app.repo_page_maximized {
        app.dock_rect = Rect::default();
        render_repo_page(frame, app, area, tick);
        render_throttle_banner(frame, app, area);
        if app.confirm.is_some() {
            render_confirm(frame, app, area);
        }
        if app.diff_modal.is_some() {
            render_diff_modal(frame, app, area);
        }
        if app.show_settings {
            render_settings(frame, app, area);
        }
        if app.show_build_info {
            render_build_info(frame, app, area);
        }
        if app.copy_menu.is_some() {
            render_copy_menu(frame, app, area);
        }
        if app.base_picker.is_some() {
            render_base_picker(frame, app, area);
        }
        // Help overlays the page / diff modal, showing that view's contextual hotkeys.
        if app.show_help {
            render_help(frame, app, area);
        }
        // The keyboard viewer sits on top of help (it's launched from the Hotkeys tab).
        if app.show_keyboard {
            render_keyboard_modal(frame, app, area);
        }
        if app.dropdown.is_some() {
            render_dropdown(frame, app, area);
        }
        // The new-build notice and transient toast sit on top of everything, on every screen.
        render_update_notice(frame, app, area, tick);
        render_toast(frame, app, area);
        return;
    }

    // Layout: main area + three-line status bar at bottom
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    let full_main_area = vertical_chunks[0];
    let status_bar_area = vertical_chunks[1];

    // Docked repo page: carve a bottom panel off the main area; the two panes share what's left.
    // The boundary is a draggable horizontal splitter (height = dock_ratio of the main area).
    app.dock_full_area = full_main_area;
    app.dock_divider_row = None;
    app.dock_rect = Rect::default();
    let dock_area = if app.repo_page.is_some() && !app.repo_page_maximized {
        let dock_height = (f64::from(full_main_area.height) * app.dock_ratio).round() as u16;
        let dock_height = dock_height.clamp(6, full_main_area.height.saturating_sub(6).max(6));
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(dock_height)])
            .split(full_main_area);
        app.dock_divider_row = Some(split[1].y);
        Some((split[0], split[1]))
    } else {
        None
    };
    let main_area = dock_area.map_or(full_main_area, |(top, _)| top);

    // Split main area horizontally using the adjustable ratio.
    let left_width = ((f64::from(main_area.width)) * app.split_ratio).round() as u16;
    let left_width = left_width.clamp(1, main_area.width.saturating_sub(1).max(1));
    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(left_width), Constraint::Min(0)])
        .split(main_area);

    let list_area = horizontal_chunks[0];
    let preview_area = horizontal_chunks[1];

    // Capture geometry for mouse hit-testing in the event loop.
    app.main_area = main_area;
    app.list_area = list_area;
    app.preview_area = preview_area;
    app.divider_col = preview_area.x;

    // Render left pane (returns the list's scroll offset for hit-testing).
    let list_offset = render_list(frame, app, list_area, tick);
    app.list_offset = list_offset;

    // Render right pane
    render_preview(frame, app, preview_area, tick);

    // Restored repo page (panel [4]): render into the bottom panel (it captures its own geometry
    // from the area it's given, so selection/scroll/clicks work there too). `dock_rect` lets the
    // event loop route clicks outside it to the list/preview (master-detail).
    if let Some((_, dock)) = dock_area {
        app.dock_rect = dock;
        render_repo_page(frame, app, dock, tick);
    }

    // Render status bar
    render_status_bar(frame, app, status_bar_area);

    // Draw the draggable divider grip (and a live highlight while it's being dragged), unless the
    // user hid the splitter.
    if app.show_splitter {
        render_divider(frame, app);
    }

    // Throttle warning (top-center) while a remote is rate-limiting us.
    render_throttle_banner(frame, app, area);

    // Help modal overlays everything else.
    if app.show_help {
        render_help(frame, app, area);
    }
    // Confirmation dialog overlays all.
    if app.confirm.is_some() {
        render_confirm(frame, app, area);
    }
    // Settings modal overlays everything.
    if app.show_settings {
        render_settings(frame, app, area);
    }
    if app.show_build_info {
        render_build_info(frame, app, area);
    }
    if app.finder.is_some() {
        render_finder_overlay(frame, app, area);
    }
    if app.picker.is_some() {
        render_picker_overlay(frame, app, area);
    }
    // The keyboard viewer sits on top of help (it's launched from the Hotkeys tab).
    if app.show_keyboard {
        render_keyboard_modal(frame, app, area);
    }
    if app.dropdown.is_some() {
        render_dropdown(frame, app, area);
    }
    // The new-build notice (top-right) and transient toast sit on top of everything.
    render_update_notice(frame, app, area, tick);
    render_toast(frame, app, area);
}

/// Render the fzf-style finder overlay (the `tui-pick` widget) and capture its click geometry. The
/// crate emits its own `HintClick` type; map them into polygit's so the shared footer-click path works.
fn render_finder_overlay(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let Some(finder) = app.finder.as_ref() else {
        return;
    };
    let mut crate_hints: Vec<tui_pick::HintClick> = Vec::new();
    let geo = tui_pick::finder::render_finder(
        frame,
        area,
        finder,
        &app.finder_history,
        &tui_pick::FinderStyle::default(),
        &mut crate_hints,
    );
    app.hint_click.clear();
    for hint in crate_hints {
        app.hint_click.push(HintClick {
            row: hint.row,
            col_start: hint.col_start,
            col_end: hint.col_end,
            key: map_crate_hint_key(hint.key),
        });
    }
    app.finder_area = centered_rect(
        area.width.saturating_sub(8).clamp(40, 120),
        area.height.saturating_sub(4).max(8),
        area,
    );
    app.finder_close_click = geo.close;
    app.finder_rows_click = geo.rows;
}

/// Render the folder picker overlay (the `tui-pick` widget) and capture its click geometry.
fn render_picker_overlay(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let Some(picker) = app.picker.as_ref() else {
        return;
    };
    let mut crate_hints: Vec<tui_pick::HintClick> = Vec::new();
    let geo = tui_pick::picker::render_picker(
        frame,
        area,
        picker,
        &tui_pick::PickerStyle::default(),
        &mut crate_hints,
    );
    app.hint_click.clear();
    for hint in crate_hints {
        app.hint_click.push(HintClick {
            row: hint.row,
            col_start: hint.col_start,
            col_end: hint.col_end,
            key: map_crate_hint_key(hint.key),
        });
    }
    app.picker_area = centered_rect(
        area.width.saturating_sub(8).clamp(40, 110),
        area.height.saturating_sub(4).max(10),
        area,
    );
    app.picker_close_click = geo.close;
    app.picker_rows_click = geo.rows;
    app.picker_crumbs_click = geo.crumbs;
}

/// Map a `tui-pick` hint key to polygit's `HintKey` (the crate's subset has no ShiftEnter).
fn map_crate_hint_key(key: tui_pick::HintKey) -> HintKey {
    match key {
        tui_pick::HintKey::Char(ch) => HintKey::Char(ch),
        tui_pick::HintKey::Enter => HintKey::Enter,
        tui_pick::HintKey::Tab => HintKey::Tab,
        tui_pick::HintKey::Esc => HintKey::Esc,
    }
}

/// Draw a grip marker at the center of the pane divider so it reads as draggable, and—while a
/// drag is in progress—brighten the whole divider column for live feedback.
fn render_divider(frame: &mut Frame, app: &AppState) {
    let area = app.main_area;
    let col = app.divider_col;
    if area.height < 3 || col <= area.x || col >= area.x + area.width {
        return;
    }
    let top = area.y + 1;
    let bottom = area.y + area.height - 1;
    let center = area.y + area.height / 2;
    let dragging = app.divider_dragging;
    // Hovered (not dragging): the grip brightens to cyan so the handle reacts to the cursor.
    let hovered = !dragging
        && app.hover_effects
        && app.hover.is_some_and(|(hover_col, hover_row)| {
            (i32::from(hover_col) - i32::from(col)).abs() <= 1 && hover_row >= top && hover_row < bottom
        });
    // The grip sits on the divider column itself (the right pane's first column). It must NOT
    // straddle into `col - 1`: that's the left pane's last column, where the vertical scrollbar is
    // drawn — a 2-wide grip would paint over the scrollbar.
    let cols = [col];
    let buffer = frame.buffer_mut();

    if dragging {
        for &grip_col in &cols {
            for row in top..bottom {
                if let Some(cell) = buffer.cell_mut((grip_col, row)) {
                    cell.set_fg(Color::Cyan);
                }
            }
        }
    }

    // A shaded run at center hints "grab here"; its length scales with the pane height. While
    // dragging it brightens to cyan AND fills solid for unmistakable grabbed feedback.
    let (grip_symbol, grip_color) = if dragging {
        ("█", Color::Cyan)
    } else if hovered {
        ("▒", Color::Cyan)
    } else {
        ("▒", Color::Gray)
    };
    let half = (area.height / 5).clamp(3, 9) / 2;
    let start = center.saturating_sub(half).max(top);
    let end = (center + half + 1).min(bottom);
    for &grip_col in &cols {
        for row in start..end {
            if let Some(cell) = buffer.cell_mut((grip_col, row)) {
                cell.set_symbol(grip_symbol).set_fg(grip_color);
            }
        }
    }
}

/// Cast a drop-shadow for a modal: dim the cells on the 1-col strip down the right edge and the
/// 1-row strip across the bottom, offset by +1 — call before the modal's `Clear` so the shadow
/// falls on the underlying UI just outside the box.
fn cast_shadow(frame: &mut Frame, area: Rect) {
    let bounds = frame.area();
    let buffer = frame.buffer_mut();
    let shadow_x = area.x + area.width;
    for row in (area.y + 1)..(area.y + area.height + 1) {
        if shadow_x < bounds.right() && row < bounds.bottom() {
            if let Some(cell) = buffer.cell_mut((shadow_x, row)) {
                cell.set_bg(Color::Black).set_fg(Color::DarkGray);
            }
        }
    }
    let shadow_y = area.y + area.height;
    for col in (area.x + 1)..(area.x + area.width + 1) {
        if col < bounds.right() && shadow_y < bounds.bottom() {
            if let Some(cell) = buffer.cell_mut((col, shadow_y)) {
                cell.set_bg(Color::Black).set_fg(Color::DarkGray);
            }
        }
    }
}

/// The track rect for a panel's scrollbar: the panel's right border column, vertically clamped
/// to the inner content area (inside the border AND any panel padding), so the bar stays within
/// the scrollable region and off the rounded corners — like a web scrollbar inside its box.
fn scrollbar_track(outer: Rect, inner: Rect) -> Rect {
    Rect { x: outer.x, y: inner.y, width: outer.width, height: inner.height }
}

/// Draw a vertical scrollbar on the right border of `area` when content overflows. `position` is
/// the scroll offset (0..=total-viewport). `highlighted` brightens the thumb (handle) while it's
/// being dragged, like the divider.
fn render_scrollbar(
    frame: &mut Frame,
    area: Rect,
    position: usize,
    total: usize,
    viewport: usize,
    highlighted: bool,
) {
    if total <= viewport {
        return;
    }
    // ratatui maps `position` over `content_length - 1` (its model = top-line index, max when the
    // last line is at the top). Our `position` maxes at `total - viewport` (last line at the
    // bottom), so set content_length accordingly for the thumb to reach the very bottom.
    let content = total - viewport + 1;
    let mut state = ScrollbarState::new(content)
        .position(position)
        .viewport_content_length(viewport);
    let thumb_style = if highlighted {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .thumb_style(thumb_style);
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

/// Repo-name spans for the list, underlining the chars that fuzzy-match the active filter (the same
/// nucleo matcher the list uses to rank). Consecutive matched / unmatched chars merge into runs.
/// The `@` status filter never highlights. Padded with trailing spaces to `width` chars.
fn highlight_name(name: &str, filter: Option<&str>, base: Style, width: usize) -> Vec<Span<'static>> {
    let name_chars: Vec<char> = name.chars().collect();
    let total = name_chars.len();
    let mut spans: Vec<Span<'static>> = Vec::new();

    let matched: std::collections::HashSet<usize> = filter
        .filter(|needle| !needle.is_empty() && !needle.starts_with('@'))
        .and_then(|needle| tui_pick::finder::fuzzy_match(name, needle).map(|(_, idx)| idx))
        .map(|idx| idx.into_iter().collect())
        .unwrap_or_default();

    if matched.is_empty() {
        spans.push(Span::styled(name.to_string(), base));
    } else {
        // Coalesce adjacent chars sharing the same matched/unmatched state into one span.
        let mut run = String::new();
        let mut run_matched = matched.contains(&0);
        for (index, ch) in name_chars.iter().enumerate() {
            let is_matched = matched.contains(&index);
            if is_matched != run_matched && !run.is_empty() {
                let style = if run_matched { base.add_modifier(Modifier::UNDERLINED) } else { base };
                spans.push(Span::styled(std::mem::take(&mut run), style));
            }
            run_matched = is_matched;
            run.push(*ch);
        }
        if !run.is_empty() {
            let style = if run_matched { base.add_modifier(Modifier::UNDERLINED) } else { base };
            spans.push(Span::styled(run, style));
        }
    }
    if width > total {
        spans.push(Span::styled(" ".repeat(width - total), base));
    }
    spans
}

