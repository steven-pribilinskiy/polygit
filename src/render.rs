
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
    AppState, ClickRegion, Column, ColumnFlags, Command, DiffFocus, DiffMode, DiffSource, HelpTab,
    HintClick, HintKey, IconSet, InfoAction, Leader, ListRow, PageRow, PageRowKind, RepoPageColumn,
    RepoPageSort, RepoState, RepoStatus, RightView, ScrollHit, ScrollKind, SortColumn, SortDir,
    StatusFilter,
};

/// The published documentation site (opened by the `D` hotkey and linked in the help modal).
pub const DOCS_URL: &str = "https://steven-pribilinskiy.github.io/polygit/";

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
fn render_tooltip(frame: &mut Frame, app: &AppState) {
    let Some(tip) = app.hover_tooltip.as_ref() else {
        return;
    };
    let area = frame.area();
    if area.width < 6 || area.height < 3 {
        return;
    }
    let text_width = UnicodeWidthStr::width(tip.text.as_str()) as u16;
    // border (2) + 1-cell horizontal padding (2) around the text.
    let width = (text_width + 4).min(area.width);
    let height = 3;
    let rect = tui_pick::position(
        tip.anchor,
        (width, height),
        area,
        tip.placement,
        tui_pick::PositionOptions { offset: 0, flip: true, shift: true },
    )
    .rect;
    let text = &tip.text;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    cast_shadow(frame, rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(text.clone()), inner);
}

/// Draw all widgets for the current state (colors still in the semantic ANSI palette).
fn render_widgets(frame: &mut Frame, app: &mut AppState, tick: u64) {
    let area = frame.area();
    // Draggable scrollbars and clickable hint regions are re-registered every frame by
    // whatever panels are visible (status bar, preview footer, …).
    app.scroll_hits.clear();
    app.clickable.clear();
    app.hint_click.clear();

    // The dedicated repo page is full-screen and replaces the normal layout — unless the user
    // docks it (then it falls through to render as a bottom panel below the two panes).
    if app.repo_page.is_some() && !app.dock_repo_panel {
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
    let dock_area = if app.repo_page.is_some() && app.dock_repo_panel {
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

    // Docked repo page: render the open repo page into the bottom panel (it captures its own
    // geometry from the area it's given, so selection/scroll/clicks work there too).
    if let Some((_, dock)) = dock_area {
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

fn render_list(frame: &mut Frame, app: &mut AppState, area: Rect, tick: u64) -> usize {
    app.hover_tooltips.clear();
    let rows = app.visible_rows();
    let total_repos = app.repos.len();
    let elapsed = app.finished_elapsed.unwrap_or_else(|| app.start.elapsed()).as_secs_f64();

    let done = app.done_count();
    // Live concurrency: active pulls / effective cap (e.g. `⇄ 8/16`). When the cap has been
    // reduced by throttle adaptation, show `running/eff↓configured`. Hidden once everything's done.
    let running = app.counts().1;
    let concurrency = if app.all_done {
        String::new()
    } else {
        let eff = app.effective_jobs();
        if eff < app.max_jobs {
            format!(" · ⇄ {running}/{eff}↓{}", app.max_jobs)
        } else {
            format!(" · ⇄ {running}/{eff}")
        }
    };
    let title = if !app.discovery_done {
        // Still crawling the tree — show a spinner and the running tally instead of done/total.
        let spin = spinner_frame(tick, app.icons());
        format!(" [1] polygit · {spin} scanning… {total_repos} found{concurrency} · {elapsed:.1}s ")
    } else {
        format!(" [1] polygit · {done}/{total_repos}{concurrency} · {elapsed:.1}s ")
    };

    let modal_open = app.any_modal_open();
    let block = Block::default()
        .title(title)
        .title_style(pane_title_style(modal_open))
        .borders(pane_borders(app))
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(!app.preview_focused, modal_open));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Compute column widths (the displayed name is the repo's path relative to the scan root).
    let max_name_len = app
        .repos
        .iter()
        .map(|repo| repo.lock().unwrap().rel_path.len())
        .max()
        .unwrap_or(10)
        .max(10);

    // icon + space + name + space + branch
    // Name column: max_name_len
    let name_col_width = max_name_len;
    // Emoji glyphs render 2 cells wide vs 1 for the Unicode set; reserve accordingly.
    let icon_width = if app.icon_style == crate::app::IconStyle::Emoji { 3 } else { 2 };
    let separator_width = 1; // space before branch

    // Reserve space for any enabled optional columns (rendered after the branch). Emoji glyphs
    // render 1 cell wider than the Unicode set, so the count columns get +1 each. Columns whose
    // data is fully loaded and trivially empty (e.g. no repo has a worktree) are hidden.
    let columns = app.effective_columns();
    let emoji = app.icon_style == crate::app::IconStyle::Emoji;
    let hide_zero = app.hide_zero_counts;
    let col_extra = usize::from(emoji);
    let dirty_w = 3 + col_extra; // glyph + up to 2 digits
    let count_w = 4 + col_extra; // glyph + count (worktrees / branches / stashes)
    let pr_w = 6; // `#NNNNN` — fits a 5-digit PR number
    let fav_w = 1; // star is a compact 1-cell symbol in both icon sets
    let columns_width = usize::from(columns.status) * (STATUS_COL_W + 1)
        + usize::from(columns.ahead_behind) * 10
        + (dirty_w + 1)
        + usize::from(columns.last_commit) * 15
        + usize::from(columns.worktrees) * (count_w + 1)
        + usize::from(columns.branches) * (count_w + 1)
        + usize::from(columns.stashes) * (count_w + 1)
        + usize::from(columns.pulled_commits) * (count_w + 1)
        + usize::from(columns.pulled_files) * (count_w + 1)
        + usize::from(columns.pull_request) * (pr_w + 1)
        + usize::from(columns.favorite) * (fav_w + 1);

    let inner_width = inner.width as usize;
    let branch_col_width = inner_width
        .saturating_sub(icon_width + name_col_width + separator_width + 2 + columns_width);

    let tree = app.tree_active();
    // Zero / still-loading count cells recede with the palette's `faint` tone (the same color
    // `Color::DarkGray` maps to, so they match the ahead/behind zeros). NOT a near-surface blend:
    // a too-faint, low-contrast gray trips terminal minimum-contrast correction (Tabby/xterm
    // darkens it on a painted background), so `faint` is the floor — recessed but rendered
    // consistently dim across terminals.
    let count_dim = app.palette().faint;
    let repo_item = |repo_idx: usize, depth: u16| -> ListItem<'static> {
            let state = app.repos[repo_idx].lock().unwrap();
            let icons = app.icons();
            // Post-change attention indicator on the cells whose value changed: pulse REVERSED
            // ("flash") and/or steady REVERSED for the whole window ("highlight") — each toggled
            // in settings. `flash_on` drives every flagged-cell style below.
            let flash_on = (app.changed_row_flash && state.flash_on())
                || (app.changed_row_highlight && state.flash_active());
            let flash = state.flash;
            let flash_style = |base: Style, flagged: bool| {
                if flash_on && flagged {
                    base.add_modifier(Modifier::REVERSED)
                } else {
                    base
                }
            };
            let mut glyph = status_glyph_colored(&state.status, tick, icons);
            if flash_on && flash.status {
                glyph.style = glyph.style.add_modifier(Modifier::REVERSED);
            }
            // Cached-but-not-pulled repos read dim: the status is last-known, not from this run.
            if state.stale {
                glyph.style = glyph.style.add_modifier(Modifier::DIM);
            }
            // Pad the glyph to `icon_width` display cells so the name column lines up
            // regardless of whether the glyph is a 1-cell Unicode char or a 2-cell emoji.
            let glyph_pad = icon_width.saturating_sub(glyph.width()).max(1);

            let branch_str = state
                .branch
                .as_deref()
                .unwrap_or("—")
                .to_string();
            let branch_truncated = truncate_str(&branch_str, branch_col_width.max(1));

            let mut name_style = match &state.status {
                RepoStatus::Failed => Style::default().fg(Color::Red),
                RepoStatus::Updated => Style::default().fg(Color::Green),
                RepoStatus::Throttled => Style::default().fg(Color::Magenta),
                RepoStatus::Skipped | RepoStatus::NoUpstream => Style::default().fg(Color::DarkGray),
                RepoStatus::Running { .. } => Style::default().fg(Color::Yellow),
                _ => Style::default(),
            };
            if state.stale {
                name_style = name_style.add_modifier(Modifier::DIM);
            }

            // In the tree view, show the indented basename (the folder hierarchy carries the
            // path); otherwise the full relative path. Truncate so deep indents never overflow
            // the name column and shift the trailing count columns out of alignment.
            let display = if tree {
                truncate_str(
                    &format!("{}{}", "  ".repeat(depth as usize), state.name),
                    name_col_width,
                )
            } else {
                state.rel_path.clone()
            };
            let mut spans = vec![glyph, Span::raw(" ".repeat(glyph_pad))];
            spans.extend(highlight_name(
                &display,
                app.filter.as_deref(),
                name_style,
                name_col_width,
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{branch_truncated:<branch_col_width$}"),
                Style::default().fg(Color::Cyan),
            ));

            if columns.status {
                // Cached repos show "<last status> <age>" dim; live repos show the bright label.
                let (text, mut style) = if state.stale {
                    let age = state.cached_at.map(crate::app::format_cache_age).unwrap_or_default();
                    let label = status_short(&state);
                    let text = if age.is_empty() { label.to_string() } else { format!("{label} {age}") };
                    (text, Style::default().fg(status_color(&state.status)).add_modifier(Modifier::DIM))
                } else {
                    (status_short(&state).to_string(), Style::default().fg(status_color(&state.status)))
                };
                style = flash_style(style, flash.status);
                spans.push(Span::styled(
                    format!(" {}", pad_display(&truncate_str(&text, STATUS_COL_W), STATUS_COL_W)),
                    style,
                ));
            }

            if columns.ahead_behind {
                spans.push(Span::raw(" "));
                match &state.details {
                    Some(details) => {
                        let mut ab = ahead_behind_spans(details.ahead, details.behind, 9, icons);
                        if flash_on && flash.ahead_behind {
                            for span in &mut ab {
                                span.style = span.style.add_modifier(Modifier::REVERSED);
                            }
                        }
                        spans.extend(ab);
                    }
                    None => spans.push(Span::styled(
                        format!("{:<9}", "…"),
                        Style::default().fg(Color::DarkGray),
                    )),
                }
            }
            {
                // Dirty slot is always shown: `•` when the repo has uncommitted changes, and
                // `•N` (the count) when the `t d` column is enabled. Skipped repos are dirty by
                // definition, so the dot shows immediately even before details load.
                let dirty_n = state.details.as_ref().map(|details| details.dirty_count);
                let is_dirty = dirty_n
                    .map(|count| count > 0)
                    .unwrap_or(matches!(state.status, RepoStatus::Skipped));
                let text = if !is_dirty {
                    String::new()
                } else if columns.dirty {
                    match dirty_n {
                        Some(count) if count > 0 => format!("{}{count}", icons.dirty),
                        _ => icons.dirty.to_string(),
                    }
                } else {
                    icons.dirty.to_string()
                };
                spans.push(Span::styled(
                    format!(" {}", pad_display(&text, dirty_w)),
                    flash_style(Style::default().fg(Color::Yellow), flash.dirty),
                ));
            }
            if columns.last_commit {
                let text = match &state.details {
                    Some(details) => truncate_str(&details.commit_rel_date, 14),
                    None => "…".to_string(),
                };
                spans.push(Span::styled(
                    format!(" {text:<14}"),
                    flash_style(Style::default().fg(Color::DarkGray), flash.last_commit),
                ));
            }
            // Count cells render a dim `0` (not a blank) once loaded, and a dim `…` while pending.
            let count_span = |glyph: &str, count: Option<u32>, color: Color, flagged: bool| {
                // Emoji mode (or the "hide zero values" setting) hides zero cells; Unicode
                // otherwise keeps a dim `0`.
                if count_cell_hidden(emoji, hide_zero, count) {
                    return Span::raw(format!(" {}", pad_display("", count_w)));
                }
                let (text, dim) = count_cell_text(glyph, count);
                let base = if dim { count_dim } else { color };
                Span::styled(
                    format!(" {}", pad_display(&text, count_w)),
                    flash_style(Style::default().fg(base), flagged),
                )
            };
            if columns.worktrees {
                // Worktree membership is known only after the discovery pass completes.
                let count = app
                    .worktrees_done
                    .then(|| app.worktrees.iter().filter(|entry| entry.repo == state.name).count() as u32);
                spans.push(count_span(icons.worktrees, count, Color::Cyan, flash.worktrees));
            }
            if columns.branches {
                let count = state.details.as_ref().map(|details| details.branch_count);
                spans.push(count_span(icons.branches, count, Color::Green, flash.branches));
            }
            if columns.stashes {
                let count = state.details.as_ref().map(|details| details.stash_count);
                spans.push(count_span(icons.stashes, count, Color::Magenta, flash.stashes));
            }
            // Pulled columns: the count from this run's pull. `…` while the repo is still in
            // flight; a dim `0` once it settles having pulled nothing (up-to-date / skipped).
            let pulled_count = |pick: fn(&crate::app::PullResult) -> u32| -> Option<u32> {
                match &state.pull_result {
                    Some(result) => Some(pick(result)),
                    None if state.status.is_terminal() => Some(0),
                    None => None,
                }
            };
            if columns.pulled_commits {
                spans.push(count_span(icons.pulled, pulled_count(|r| r.commits), Color::Green, false));
            }
            if columns.pulled_files {
                spans.push(count_span(icons.changed, pulled_count(|r| r.files), Color::Cyan, false));
            }
            if columns.pull_request {
                // `#N` (a clickable link, region registered post-render) when an open PR exists;
                // blank otherwise (unresolved or no PR).
                let text = state.pr.as_ref().map(|pr| format!("#{}", pr.number)).unwrap_or_default();
                let style = if state.pr.is_some() {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(format!(" {}", pad_display(&text, pr_w)), style));
            }
            if columns.favorite {
                // Already holding this repo's lock — read favorites by rel_path, don't re-lock.
                let favorited = app.favorites.contains(&state.rel_path);
                let (glyph, style) = if favorited {
                    (icons.fav_on, Style::default().fg(Color::Yellow))
                } else {
                    (icons.fav_off, Style::default().fg(Color::DarkGray))
                };
                spans.push(Span::raw(" "));
                spans.push(Span::styled(pad_display(glyph, fav_w), style));
            }

            ListItem::new(Line::from(spans))
    };

    let mut items: Vec<ListItem> = rows
        .iter()
        .map(|row| match *row {
            ListRow::Repo { repo_idx, depth } => repo_item(repo_idx, depth),
            ListRow::GroupHeader { group_idx, parent, collapsible, depth } => {
                group_header_item(app, group_idx, parent, collapsible, depth, inner_width, tick)
            }
            ListRow::FolderHeader { node_idx, depth } => {
                folder_header_item(app, node_idx, depth, inner_width, tick)
            }
            ListRow::FavoritesHeader => favorites_header_item(app, inner_width, tick),
            ListRow::Spacer => ListItem::new(Line::from("")),
        })
        .collect();

    // Add separator and Result item
    items.push(ListItem::new(Line::from(vec![Span::styled(
        "─".repeat(inner_width.saturating_sub(2)),
        Style::default().fg(Color::DarkGray),
    )])));

    let result_icons = app.icons();
    let result_glyph = if app.all_done {
        let (_, _, _, _, _, failed, _, _) = app.counts();
        if failed > 0 {
            Span::styled(result_icons.failed, Style::default().fg(Color::Red))
        } else {
            Span::styled(result_icons.ok, Style::default().fg(Color::Green))
        }
    } else {
        Span::styled("—", Style::default().fg(Color::DarkGray))
    };

    items.push(ListItem::new(Line::from(vec![
        result_glyph,
        Span::raw(" "),
        Span::raw("Result"),
    ])));

    // A dynamic Errors row, only when something failed — appears after Result.
    let has_errors = app.has_errors();
    if has_errors {
        let failed = app.counts().5;
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "─".repeat(inner_width.saturating_sub(2)),
            Style::default().fg(Color::DarkGray),
        )])));
        items.push(ListItem::new(Line::from(vec![
            Span::styled(result_icons.failed, Style::default().fg(Color::Red)),
            Span::raw(" "),
            Span::styled(format!("Errors ({failed})"), Style::default().fg(Color::Red)),
        ])));
    }

    // Trailing (non-selectable) empty-state hint once the scan finishes with nothing to show.
    if app.discovery_done && app.repos.is_empty() {
        items.push(ListItem::new(Line::from("")));
        items.push(ListItem::new(Line::from(Span::styled(
            "  no git repositories found — q to quit",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ))));
    }

    let mut list_state = ListState::default();
    // Physical list-item row of the selection (skipping the separator lines):
    //   list rows → same index; Result → rows.len()+1; Errors → rows.len()+3.
    let sel_item = if app.selected < rows.len() {
        app.selected
    } else if app.selected == rows.len() {
        rows.len() + 1
    } else {
        rows.len() + 3
    };

    // Split the inner area into a 2-row column header (titles + sort indicator) and the repo
    // rows beneath. Too short for a header → use the whole inner area for rows.
    let header_height: u16 = if inner.height >= 4 { 2 } else { 0 };
    let rows_area = Rect {
        x: inner.x,
        y: inner.y + header_height,
        width: inner.width,
        height: inner.height.saturating_sub(header_height),
    };
    let (header_lines, header_click, fav_range) = if header_height > 0 {
        build_list_header(
            inner,
            icon_width,
            name_col_width,
            branch_col_width,
            columns,
            count_w,
            dirty_w,
            pr_w,
            fav_w,
            app.sort_column,
            app.sort_dir,
        )
    } else {
        (Vec::new(), Vec::new(), None)
    };
    if header_height > 0 {
        let header_area = Rect { height: header_height, ..inner };
        frame.render_widget(Paragraph::new(header_lines), header_area);
        app.header_area = header_area;
    } else {
        app.header_area = Rect::default();
    }
    app.header_click = header_click;
    // Dwell tooltips for the column-header titles (the underline row is left bare).
    if header_height > 0 {
        let title_row = app.header_area.y;
        let header = app.header_area;
        for &(start, end, sort) in &app.header_click {
            app.hover_tooltips.push(crate::app::TooltipRegion {
                row: title_row,
                col_start: start,
                col_end: end,
                text: column_header_tooltip(sort).to_string(),
                // Anchor to the full-height header cell so the popup drops below the whole header,
                // left-aligned to the column, flipping above when there's no room below.
                anchor: Rect { x: start, y: header.y, width: end.saturating_sub(start), height: header.height },
                placement: tui_pick::Placement::bottom_start(),
            });
        }
    }

    let total_items = items.len();
    // Drive the scroll from the manual `list_scroll`, which the plain wheel moves independently of
    // the selection. ratatui's List treats `select(None)` as `select(0)` and would snap the offset
    // back to the top, so it can't give a selection-independent scroll via `ListState::offset` —
    // instead we drop the scrolled-past items and render from the top of the remainder.
    let viewport = rows_area.height as usize;
    let scroll = app.list_scroll.min(total_items.saturating_sub(viewport));
    app.list_scroll = scroll;
    // Highlight the selected row only when it falls within the scrolled viewport.
    if viewport > 0 && sel_item >= scroll && sel_item < scroll + viewport {
        list_state.select(Some(sel_item - scroll));
    } else {
        list_state.select(None);
    }
    let visible_items: Vec<ListItem> = items.into_iter().skip(scroll).collect();
    let list = List::new(visible_items)
        .highlight_style(selection_highlight_style(app))
        .highlight_symbol("");

    frame.render_stateful_widget(list, rows_area, &mut list_state);
    // Scrollbar on the pane's right border, aligned to the rows region (below the header).
    let scrollbar_area = Rect {
        x: area.x,
        y: rows_area.y,
        width: area.width,
        height: rows_area.height,
    };
    render_scrollbar(frame, scrollbar_area, scroll, total_items, viewport, false);

    // Capture clickable PR-cell regions: for each visible repo with an open PR, the `#N` cell's
    // screen row + the PR column's x-range (taken from the header) opens the PR in the browser.
    app.pr_cell_click.clear();
    if columns.pull_request {
        let pr_x = app
            .header_click
            .iter()
            .find(|&&(_, _, column)| column == SortColumn::PullRequest)
            .map(|&(start, end, _)| (start, end));
        if let Some((start, end)) = pr_x {
            let offset = scroll;
            let height = rows_area.height as usize;
            let mut clicks = Vec::new();
            for (visible, row) in rows.iter().skip(offset).take(height).enumerate() {
                if let ListRow::Repo { repo_idx, .. } = *row {
                    let url = app.repos[repo_idx].lock().unwrap().pr.as_ref().map(|pr| pr.url.clone());
                    if let Some(url) = url {
                        clicks.push((rows_area.y + visible as u16, start, end, url));
                    }
                }
            }
            app.pr_cell_click = clicks;
        }
    }

    // Capture clickable favorite-star regions: each visible repo's star cell toggles its favorite.
    app.fav_cell_click.clear();
    if let Some((start, end)) = fav_range.filter(|_| columns.favorite) {
        let offset = scroll;
        let height = rows_area.height as usize;
        let mut clicks = Vec::new();
        for (visible, row) in rows.iter().skip(offset).take(height).enumerate() {
            if let ListRow::Repo { repo_idx, .. } = *row {
                clicks.push((rows_area.y + visible as u16, start, end, repo_idx));
            }
        }
        app.fav_cell_click = clicks;
    }

    // Dwell tooltips for the group/folder count tails (right-aligned in each header row).
    {
        let offset = scroll;
        let height = rows_area.height as usize;
        let end = inner.x + inner.width.saturating_sub(2);
        for (visible, row) in rows.iter().skip(offset).take(height).enumerate() {
            let (repos, noun) = match *row {
                ListRow::GroupHeader { group_idx, .. } => {
                    (Some(app.group_visible_members(group_idx)), "group")
                }
                ListRow::FolderHeader { node_idx, .. } => {
                    (Some(app.tree_subtree_repos(node_idx)), "folder")
                }
                ListRow::FavoritesHeader => {
                    let favorites = (0..app.repos.len()).filter(|&idx| app.is_favorite(idx)).collect();
                    (Some(favorites), "favorites")
                }
                _ => (None, ""),
            };
            if let Some(repos) = repos {
                let total = repos.len();
                let icons = app.icons();
                let tail_w: usize =
                    status_tail_for(app, &repos, total, icons, tick).iter().map(|s| s.width()).sum();
                let screen_row = rows_area.y + visible as u16;
                let start = end.saturating_sub(tail_w as u16);
                let text = header_tail_tooltip(app, &repos, total, noun);
                app.hover_tooltips.push(crate::app::TooltipRegion {
                    row: screen_row,
                    col_start: start,
                    col_end: end,
                    text,
                    anchor: Rect { x: start, y: screen_row, width: end.saturating_sub(start), height: 1 },
                    placement: tui_pick::Placement::bottom_start(),
                });
            }
        }
    }

    app.list_rows_area = rows_area;
    scroll
}

/// `build_list_header` output: the 2 header lines, the clickable sort-cell regions
/// `(col_start, col_end, column)`, and the favorite column's x-range (when shown).
type ListHeader = (Vec<Line<'static>>, Vec<(u16, u16, SortColumn)>, Option<(u16, u16)>);

/// Build the 2-row repo-list column header: titles aligned to the row column widths with a
/// `▲`/`▼` indicator on the active sort column, plus the clickable sort-cell regions.
#[allow(clippy::too_many_arguments)]
fn build_list_header(
    inner: Rect,
    icon_width: usize,
    name_col_width: usize,
    branch_col_width: usize,
    columns: ColumnFlags,
    count_w: usize,
    dirty_w: usize,
    pr_w: usize,
    fav_w: usize,
    sort_column: SortColumn,
    sort_dir: SortDir,
) -> ListHeader {
    // (label, width, leading_space, sort, fav) — mirrors the exact widths the rows use.
    struct Cell {
        label: &'static str,
        width: usize,
        lead: bool,
        sort: Option<SortColumn>,
        fav: bool,
    }
    let cell = |label: &'static str, width: usize, lead: bool, sort: Option<SortColumn>| Cell {
        label,
        width,
        lead,
        sort,
        fav: false,
    };
    let mut cells = vec![
        cell("", icon_width, false, None),
        cell("name", name_col_width, false, Some(SortColumn::Name)),
        cell("", 1, false, None),
        cell("branch", branch_col_width, false, Some(SortColumn::Branch)),
    ];
    if columns.status {
        cells.push(cell("status", STATUS_COL_W, true, Some(SortColumn::Status)));
    }
    if columns.ahead_behind {
        cells.push(cell("↑↓", 9, true, Some(SortColumn::AheadBehind)));
    }
    // The dirty column is always present (the `t d` toggle controls the count, not visibility).
    cells.push(cell("Δ", dirty_w, true, Some(SortColumn::Dirty)));
    if columns.last_commit {
        cells.push(cell("age", 14, true, Some(SortColumn::LastCommit)));
    }
    if columns.worktrees {
        cells.push(cell("wt", count_w, true, Some(SortColumn::Worktrees)));
    }
    if columns.branches {
        cells.push(cell("br", count_w, true, Some(SortColumn::Branches)));
    }
    if columns.stashes {
        cells.push(cell("st", count_w, true, Some(SortColumn::Stashes)));
    }
    if columns.pulled_commits {
        cells.push(cell("pull", count_w, true, Some(SortColumn::PulledCommits)));
    }
    if columns.pulled_files {
        cells.push(cell("chg", count_w, true, Some(SortColumn::PulledFiles)));
    }
    if columns.pull_request {
        cells.push(cell("pr", pr_w, true, Some(SortColumn::PullRequest)));
    }
    if columns.favorite {
        // Not sortable (favorites-first handles ordering) — a plain title, no click-to-sort region.
        cells.push(Cell { label: "\u{2605}", width: fav_w, lead: true, sort: None, fav: true });
    }

    let active_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let title_style = Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD);
    let mut spans: Vec<Span> = Vec::new();
    let mut clicks: Vec<(u16, u16, SortColumn)> = Vec::new();
    let mut fav_range: Option<(u16, u16)> = None;
    let mut col = inner.x;
    for cell in &cells {
        if cell.lead {
            spans.push(Span::raw(" "));
            col += 1;
        }
        let active = cell.sort.is_some() && cell.sort == Some(sort_column);
        let mut text = cell.label.to_string();
        if active {
            text.push_str(sort_dir.arrow());
        }
        let text = truncate_str(&text, cell.width.max(1));
        let style = if active {
            active_style
        } else if cell.fav {
            Style::default().fg(Color::Yellow)
        } else if cell.sort.is_some() {
            title_style
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(pad_display(&text, cell.width), style));
        if let Some(sort) = cell.sort {
            clicks.push((col, col + cell.width as u16, sort));
        }
        if cell.fav {
            fav_range = Some((col, col + cell.width as u16));
        }
        col += cell.width as u16;
    }

    let underline = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(Color::DarkGray),
    ));
    (vec![Line::from(spans), underline], clicks, fav_range)
}

/// Build a group-header list row: a collapse marker (collapsible headers only), the group
/// name, a dash fill, then non-zero status counts and the member total. Headers (and the
/// spacer rows between sections) are real list rows, each exactly one row tall, so physical
/// rows == logical rows and hit-testing stays index-for-index.
#[allow(clippy::too_many_arguments)]
fn group_header_item(
    app: &AppState,
    group_idx: usize,
    parent: Option<usize>,
    collapsible: bool,
    depth: u16,
    inner_width: usize,
    tick: u64,
) -> ListItem<'static> {
    let icons = app.icons();
    let members = app.group_visible_members(group_idx);
    let tail = status_tail_for(app, &members, members.len(), icons, tick);

    let group = app.groups.get(group_idx);
    let collapsed = collapsible
        && app.collapsed_groups.contains(&app.group_collapse_key(group_idx, parent));
    let name = app.group_name(group_idx).to_string();
    let marker = header_marker(collapsible, collapsed);
    let name_style = if group.is_some() {
        Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut head: Vec<Span> = vec![Span::raw("  ".repeat(depth as usize))];
    head.push(Span::styled(marker, Style::default().fg(Color::DarkGray)));
    head.push(Span::styled(name, name_style));
    if let Some(group) = group {
        if group.resolving {
            head.push(Span::styled(
                format!(" {}", spinner_frame(tick, icons)),
                Style::default().fg(Color::Yellow),
            ));
        } else if group.error.is_some() {
            head.push(Span::styled(format!(" {}", icons.warning), Style::default().fg(Color::Red)));
        }
    }
    finish_header_line(head, tail, inner_width, !app.hide_folder_lines)
}

/// A directory-tree folder header: collapse marker, indented folder name, dash fill, then the
/// aggregated status counts + total over the folder's whole subtree.
fn folder_header_item(
    app: &AppState,
    node_idx: usize,
    depth: u16,
    inner_width: usize,
    tick: u64,
) -> ListItem<'static> {
    let icons = app.icons();
    let subtree = app.tree_subtree_repos(node_idx);
    let tail = status_tail_for(app, &subtree, subtree.len(), icons, tick);
    let collapsed = app
        .tree_nodes
        .get(node_idx)
        .is_some_and(|node| app.collapsed_folders.contains(&node.rel_path));
    let name = app.tree_nodes.get(node_idx).map(|node| node.name.clone()).unwrap_or_default();
    let marker = header_marker(true, collapsed);
    let mut head: Vec<Span> = vec![Span::raw("  ".repeat(depth as usize))];
    head.push(Span::styled(marker, Style::default().fg(Color::Cyan)));
    head.push(Span::styled(
        format!("{name}/"),
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ));
    finish_header_line(head, tail, inner_width, !app.hide_folder_lines)
}

/// The pinned "★ Favorites" section header (favorites-first mode): a star, the label, a dash
/// fill, then the aggregated status counts + total over the favorited repos.
fn favorites_header_item(app: &AppState, inner_width: usize, tick: u64) -> ListItem<'static> {
    let icons = app.icons();
    let favorites: Vec<usize> =
        (0..app.repos.len()).filter(|&idx| app.is_favorite(idx)).collect();
    let tail = status_tail_for(app, &favorites, favorites.len(), icons, tick);
    let head = vec![
        Span::raw("  "),
        Span::styled("\u{2605} Favorites", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ];
    finish_header_line(head, tail, inner_width, !app.hide_folder_lines)
}

/// The collapse marker for a header: two spaces (static), `▸ ` (collapsed), or `▾ ` (expanded).
fn header_marker(collapsible: bool, collapsed: bool) -> &'static str {
    if !collapsible {
        "  "
    } else if collapsed {
        "▸ "
    } else {
        "▾ "
    }
}

/// One-line description for a sortable column header (shown as a dwell tooltip).
fn column_header_tooltip(sort: SortColumn) -> &'static str {
    match sort {
        SortColumn::Name => "Repository name (relative to the scan root)",
        SortColumn::Branch => "Current branch",
        SortColumn::Status => "Pull status (and the failure/skip reason when known)",
        SortColumn::AheadBehind => "Commits ahead ↑ / behind ↓ the upstream",
        SortColumn::Dirty => "Δ — uncommitted (dirty) working-tree changes",
        SortColumn::LastCommit => "Age of the last commit on the current branch",
        SortColumn::Worktrees => "wt — linked git worktrees on this repo",
        SortColumn::Branches => "br — local branches",
        SortColumn::Stashes => "st — stash entries",
        SortColumn::PulledCommits => "pull — commits pulled in this session",
        SortColumn::PulledFiles => "chg — files changed by this session's pull",
        SortColumn::PullRequest => "pr — open pull request for the current branch (click to open)",
    }
}

/// Status tallies for a set of repos `(running, updated, failed, skipped, throttled)`. Shared by
/// `status_tail_for` (the rendered glyph tail) and the group/folder count-tail tooltip text.
fn header_status_counts(app: &AppState, repos: &[usize]) -> (usize, usize, usize, usize, usize) {
    let (mut running, mut updated, mut failed, mut skipped, mut throttled) = (0, 0, 0, 0, 0);
    for &repo_idx in repos {
        match app.repos[repo_idx].lock().unwrap().status {
            RepoStatus::Running { .. } => running += 1,
            RepoStatus::Updated => updated += 1,
            RepoStatus::Failed => failed += 1,
            RepoStatus::Skipped => skipped += 1,
            RepoStatus::Throttled => throttled += 1,
            _ => {}
        }
    }
    (running, updated, failed, skipped, throttled)
}

/// Tooltip text for a group/folder count tail (e.g. "27 repos in group · 3 running · 1 failed").
fn header_tail_tooltip(app: &AppState, repos: &[usize], total: usize, noun: &str) -> String {
    let (running, updated, failed, skipped, throttled) = header_status_counts(app, repos);
    let mut parts = vec![format!("{total} repos in {noun}")];
    let plural = |count: usize, word: &str| format!("{count} {word}");
    if running > 0 {
        parts.push(plural(running, "running"));
    }
    if updated > 0 {
        parts.push(plural(updated, "updated"));
    }
    if throttled > 0 {
        parts.push(plural(throttled, "throttled"));
    }
    if failed > 0 {
        parts.push(plural(failed, "failed"));
    }
    if skipped > 0 {
        parts.push(plural(skipped, "skipped"));
    }
    parts.join(" \u{b7} ")
}

/// Build the status-count tail for `repos` (the non-zero running/updated/failed/skipped tallies
/// plus the `(total)` count), in real colors.
fn status_tail_for(
    app: &AppState,
    repos: &[usize],
    total: usize,
    icons: &IconSet,
    tick: u64,
) -> Vec<Span<'static>> {
    let mut running = 0usize;
    let mut updated = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut throttled = 0usize;
    for &repo_idx in repos {
        match app.repos[repo_idx].lock().unwrap().status {
            RepoStatus::Running { .. } => running += 1,
            RepoStatus::Updated => updated += 1,
            RepoStatus::Failed => failed += 1,
            RepoStatus::Skipped => skipped += 1,
            RepoStatus::Throttled => throttled += 1,
            _ => {}
        }
    }
    // A space between glyph and count so ambiguous-width glyphs (e.g. `⊘`) don't collide with the
    // number next to them.
    let mut tail: Vec<Span> = Vec::new();
    if running > 0 {
        tail.push(Span::styled(
            format!(" {} {running}", spinner_frame(tick, icons)),
            Style::default().fg(Color::Yellow),
        ));
    }
    if updated > 0 {
        tail.push(Span::styled(format!(" {} {updated}", icons.updated), Style::default().fg(Color::Green)));
    }
    if throttled > 0 {
        tail.push(Span::styled(
            format!(" {} {throttled}", icons.throttled),
            Style::default().fg(Color::Magenta),
        ));
    }
    if failed > 0 {
        tail.push(Span::styled(format!(" {} {failed}", icons.failed), Style::default().fg(Color::Red)));
    }
    if skipped > 0 {
        tail.push(Span::styled(format!(" {} {skipped}", icons.skipped), Style::default().fg(Color::DarkGray)));
    }
    tail.push(Span::styled(format!(" ({total})"), Style::default().fg(Color::DarkGray)));
    tail
}

/// Join a header's `head` spans and `tail` spans so the tail is right-aligned. `draw_lines` fills
/// the gap with a dim dash leader (default); when off it uses blank space (the "hide folder lines"
/// setting) so the header reads cleaner.
fn finish_header_line(
    head: Vec<Span<'static>>,
    tail: Vec<Span<'static>>,
    inner_width: usize,
    draw_lines: bool,
) -> ListItem<'static> {
    let head_width: usize = head.iter().map(|span| span.width()).sum();
    let tail_width: usize = tail.iter().map(|span| span.width()).sum();
    let fill = inner_width.saturating_sub(head_width + tail_width + 3);
    let leader = if draw_lines { "\u{2500}".repeat(fill) } else { " ".repeat(fill) };
    let mut spans = head;
    spans.push(Span::styled(format!(" {leader}"), Style::default().fg(Color::DarkGray)));
    spans.extend(tail);
    ListItem::new(Line::from(spans))
}

/// Width of the optional status text column — fits the longest label ("no upstream").
// Wide enough for the longest label plus a compact cache age (e.g. "no upstream 2d" = 14).
const STATUS_COL_W: usize = 14;

/// Short status-column text: the recorded failure/skip qualifier when known ("not found",
/// "auth", "diverged", "ref gone", …), else the plain status label.
fn status_short(state: &RepoState) -> &'static str {
    match state.status {
        RepoStatus::Failed => state.status_note.unwrap_or("failed"),
        RepoStatus::Skipped => "dirty",
        RepoStatus::NoUpstream => state.status_note.unwrap_or("no upstream"),
        ref status => status_label(status),
    }
}

/// The semantic color a status renders with (same mapping as its glyph).
fn status_color(status: &RepoStatus) -> Color {
    match status {
        RepoStatus::Queued | RepoStatus::NoUpstream | RepoStatus::Skipped => Color::DarkGray,
        RepoStatus::Running { .. } => Color::Yellow,
        RepoStatus::UpToDate => Color::Gray,
        RepoStatus::Updated => Color::Green,
        RepoStatus::Throttled => Color::Magenta,
        RepoStatus::Failed => Color::Red,
    }
}

/// Human-readable label for a repo's status.
fn status_label(status: &RepoStatus) -> &'static str {
    match status {
        RepoStatus::Queued => "queued",
        RepoStatus::Running { .. } => "running",
        RepoStatus::UpToDate => "up-to-date",
        RepoStatus::Updated => "updated",
        RepoStatus::NoUpstream => "no upstream",
        RepoStatus::Skipped => "skipped",
        RepoStatus::Throttled => "throttled",
        RepoStatus::Failed => "failed",
    }
}

fn render_preview(frame: &mut Frame, app: &mut AppState, area: Rect, _tick: u64) {
    let rows = app.visible_rows();
    let selected_row = rows.get(app.selected).copied();

    // Which pane is showing: a repo's log/diff, a group summary, the Result summary, or the
    // Errors list. The Result overlay (Space) forces Result regardless of selection.
    let show_errors = !app.result_overlay && app.has_errors() && app.selected == rows.len() + 1;
    let show_result = app.result_overlay || (app.selected >= rows.len() && !show_errors);
    let overlay = show_result || show_errors;
    let selected_group = match selected_row {
        Some(ListRow::GroupHeader { group_idx, .. }) if !overlay => Some(group_idx),
        _ => None,
    };
    let selected_folder = match selected_row {
        Some(ListRow::FolderHeader { node_idx, .. }) if !overlay => Some(node_idx),
        _ => None,
    };
    let selected_repo = match selected_row {
        Some(ListRow::Repo { repo_idx, .. }) if !overlay => Some(repo_idx),
        _ => None,
    };

    // Clickable info-block regions are rebuilt each frame (and only the main view captures them).
    app.info_click.clear();

    // The preview pane stacks an info panel (`i`, top, repo-only) and the result/log panel (`I`,
    // bottom). Each hides independently; with both shown a draggable boundary splits them by
    // `preview_split_ratio`. Hidden result → info fills the pane (reads like the repo list).
    let info_visible = app.info_pinned && selected_repo.is_some();
    let result_visible = app.show_result_panel;
    app.preview_divider_row = None;
    let area = match (info_visible, result_visible) {
        (true, true) => {
            let repo_idx = selected_repo.unwrap();
            let info_h = ((f64::from(area.height)) * app.preview_split_ratio).round() as u16;
            let info_h = info_h.clamp(3, area.height.saturating_sub(3).max(3));
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(info_h), Constraint::Min(0)])
                .split(area);
            app.preview_split_area = area;
            app.preview_divider_row = Some(chunks[1].y);
            render_info_panel(frame, app, chunks[0], repo_idx);
            chunks[1]
        }
        (true, false) => {
            render_info_panel(frame, app, area, selected_repo.unwrap());
            app.preview_total = 0;
            app.preview_viewport = 0;
            app.preview_scroll_area = Rect::default();
            return;
        }
        (false, false) => {
            render_preview_hidden_hint(frame, app, area);
            app.preview_total = 0;
            app.preview_viewport = 0;
            app.preview_scroll_area = Rect::default();
            return;
        }
        (false, true) => area,
    };

    let (header_text, content_lines, scroll_offset) = if show_errors {
        (" Errors ".to_string(), build_error_summary(app), 0usize)
    } else if show_result {
        (" Result ".to_string(), build_result_summary(app), 0usize)
    } else if let Some(group_idx) = selected_group {
        (
            format!(" {} · group ", app.group_name(group_idx)),
            build_group_summary(app, group_idx),
            0usize,
        )
    } else if let Some(node_idx) = selected_folder {
        let label = app
            .tree_nodes
            .get(node_idx)
            .map(|node| node.rel_path.clone())
            .unwrap_or_default();
        (format!(" {label} · folder "), build_folder_summary(app, node_idx), 0usize)
    } else {
        let repo_idx = selected_repo.unwrap_or_default();
        let state = app.repos[repo_idx].lock().unwrap();
        if app.right_view == RightView::Diff {
            let lines = state
                .diff
                .clone()
                .unwrap_or_else(|| vec!["(loading…)".to_string()]);
            (format!(" {} · diff ", state.name), lines, state.preview_scroll)
        } else {
            let pid_str = match &state.status {
                RepoStatus::Running { pid } => format!("pid {pid}"),
                _ => "pid —".to_string(),
            };
            let elapsed_str = match state.elapsed {
                Some(elapsed) => format!(" · {:.2}s", elapsed.as_secs_f64()),
                None => match state.start {
                    Some(start) => format!(" · {:.2}s", start.elapsed().as_secs_f64()),
                    None => String::new(),
                },
            };
            let header = format!(
                " Command log · {} · {} · {}{} ",
                state.name,
                status_label(&state.status),
                pid_str,
                elapsed_str
            );
            let lines: Vec<String> = state.log.lines().iter().cloned().collect();
            (header, lines, state.preview_scroll)
        }
    };

    let modal_open = app.any_modal_open();
    let mut block = Block::default()
        .title(format!(" [2]{header_text}"))
        .title_style(pane_title_style(modal_open))
        .borders(pane_borders(app))
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.preview_focused, modal_open));

    // A `⧉` copy button on the top border copies the repo's log when it has output, otherwise the
    // repo path — so it's always useful (an up-to-date repo's log is empty). Same clipboard handler
    // as the info panel's Path copy.
    let showing_repo_log = selected_repo.is_some()
        && !overlay
        && selected_group.is_none()
        && selected_folder.is_none();
    if showing_repo_log {
        let log_text = content_lines.join("\n");
        let copy_text = if log_text.trim().is_empty() {
            selected_repo
                .map(|idx| app.repos[idx].lock().unwrap().path.display().to_string())
                .unwrap_or_default()
        } else {
            log_text
        };
        if !copy_text.is_empty() {
            let glyph = "⧉";
            // The right-aligned title renders just inside the border corner (area.x+width-1), so the
            // click region must end there too — sub(2) left it one cell left of the glyph, so every
            // click missed and nothing copied.
            let col_end = area.x + area.width.saturating_sub(1);
            let col_start = col_end.saturating_sub(UnicodeWidthStr::width(glyph) as u16);
            block = block.title_top(
                Line::from(Span::styled(
                    glyph,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ))
                .right_aligned(),
            );
            app.info_click.push((area.y, col_start, col_end, InfoAction::CopyText(copy_text)));
        }
    }

    // Group view: the key hints live in the pane chrome as styled, CLICKABLE segments (same
    // machinery as the status bar), not as plain content text.
    if let Some(group_idx) = selected_group {
        let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
        let hint = Style::default().fg(Color::DarkGray);
        let footer: Vec<(&str, Style, Option<Command>)> = vec![
            (" enter/space", key, Some(Command::ToggleGroupCollapsed(group_idx))),
            (" collapse/expand", hint, Some(Command::ToggleGroupCollapsed(group_idx))),
            (" · ", hint, None),
            ("z", key, Some(Command::GroupingToggle)),
            (" ungrouped view ", hint, Some(Command::GroupingToggle)),
        ];
        let footer_width: u16 = footer
            .iter()
            .map(|(text, _, _)| UnicodeWidthStr::width(*text) as u16)
            .sum();
        let footer_row = area.y + area.height.saturating_sub(1);
        let mut col = area.x + area.width.saturating_sub(footer_width + 1);
        let mut spans = Vec::new();
        for (text, style, command) in footer {
            let text_width = UnicodeWidthStr::width(text) as u16;
            if let Some(command) = command {
                app.clickable.push(ClickRegion {
                    row: footer_row,
                    col_start: col,
                    col_end: col + text_width,
                    command,
                });
            }
            col += text_width;
            spans.push(Span::styled(text, style));
        }
        block = block.title_bottom(Line::from(spans).right_aligned());
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let inner_height = inner.height as usize;
    let total_lines = content_lines.len();

    // Convert lines to ratatui Text with ANSI color support
    let text_lines: Vec<Line> = content_lines
        .iter()
        .map(|line| ansi_line_to_ratatui(line))
        .collect();

    let max_scroll = total_lines.saturating_sub(inner_height);
    let effective_scroll = scroll_offset.min(max_scroll);

    let text = Text::from(text_lines);
    let para = Paragraph::new(text)
        .scroll((effective_scroll as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
    let track = scrollbar_track(area, inner);
    render_scrollbar(
        frame,
        track,
        effective_scroll,
        total_lines,
        inner_height,
        app.scrollbar_dragging == Some(ScrollKind::Preview),
    );

    // Capture scroll geometry for the event loop's wheel/scrollbar hit-testing.
    app.preview_total = total_lines;
    app.preview_viewport = inner_height;
    app.preview_scroll_area = track;
    app.scroll_hits.push(ScrollHit {
        kind: ScrollKind::Preview,
        track,
        total: total_lines,
        viewport: inner_height,
    });
}

/// Render the per-repo info view (status, branch, ahead/behind, remote, last commit,
/// worktrees, changes, path) plus a command-hint footer, for the selected repo.
/// Build the per-repo info content lines (status, branch, ahead/behind, commit, changes,
/// remote, worktrees, path) — shared by the full info view and the pinned info section.
/// A browsable https base for a remote URL (strips a trailing `.git`), or None for non-web remotes.
fn web_remote(remote: &str) -> Option<String> {
    let trimmed = remote.trim().trim_end_matches('/');
    let base = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    base.starts_with("https://").then(|| base.to_string())
}

/// Split `text` into chunks of at most `width` display columns, on char boundaries.
fn wrap_chars(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    for ch in text.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if current_width + char_width > width && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push(ch);
        current_width += char_width;
    }
    if !current.is_empty() || chunks.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Wrap a link / URL across `width`-wide lines, preferring to break right AFTER a separator
/// (`/ - . : _ @`) so it splits at natural boundaries; falls back to a hard char break when no
/// separator fits on the line. Each returned segment is ≤ `width` display columns.
fn wrap_link(text: &str, width: usize) -> Vec<String> {
    const SEPS: [char; 6] = ['/', '-', '.', ':', '_', '@'];
    if width == 0 {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        // Greedily find how many chars fit in `width` display columns from `start`.
        let mut end = start;
        let mut used = 0;
        while end < chars.len() {
            let char_width = unicode_width::UnicodeWidthChar::width(chars[end]).unwrap_or(1);
            if used + char_width > width {
                break;
            }
            used += char_width;
            end += 1;
        }
        if end >= chars.len() {
            lines.push(chars[start..].iter().collect());
            break;
        }
        // Prefer to break right after the last separator that fits — keeps it at the line's end.
        let brk = (start + 1..end)
            .rev()
            .find(|&index| SEPS.contains(&chars[index]))
            .map_or(end, |index| index + 1);
        lines.push(chars[start..brk].iter().collect());
        start = brk;
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// The info block's wrapped display lines plus the clickable regions inside them
/// (`(line_index, start_col, end_col, action)`, columns relative to the inner content origin).
type InfoClick = (usize, u16, u16, InfoAction);

fn build_info_lines(
    app: &AppState,
    repo_idx: usize,
    content_width: usize,
) -> (Vec<Line<'static>>, Vec<InfoClick>) {
    let state = app.repos[repo_idx].lock().unwrap();

    const LABEL_W: usize = 13;
    let label = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
    let value = Style::default().fg(Color::Gray);
    let dim = Style::default().fg(Color::DarkGray);
    let link = Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
    let value_width = content_width.saturating_sub(LABEL_W).max(1);

    let mut lines: Vec<Line> = Vec::new();
    let mut clicks: Vec<InfoClick> = Vec::new();

    let plain = |name: &str, text: String| {
        Line::from(vec![
            Span::styled(format!("{name:<13}"), label),
            Span::styled(text, value),
        ])
    };

    // A clickable link field that WRAPS (rather than truncates) — Branch and Remote, where the
    // whole value is worth seeing. Each wrapped segment is its own clickable region (same URL),
    // continuations indent to the value column.
    let push_link = |lines: &mut Vec<Line<'static>>, clicks: &mut Vec<InfoClick>, name: &str, text: &str, url: &str| {
        for (index, segment) in wrap_link(text, value_width).into_iter().enumerate() {
            let line_idx = lines.len();
            let width = UnicodeWidthStr::width(segment.as_str()) as u16;
            clicks.push((line_idx, LABEL_W as u16, LABEL_W as u16 + width, InfoAction::OpenUrl(url.to_string())));
            let label_span = if index == 0 {
                Span::styled(format!("{name:<13}"), label)
            } else {
                Span::raw(format!("{:<13}", ""))
            };
            lines.push(Line::from(vec![label_span, Span::styled(segment, link)]));
        }
    };

    // Status — the live result + how long the pull took, or the cached last-known status + age.
    let status_value = if state.stale {
        let age = state.cached_at.map(crate::app::format_cache_age).unwrap_or_default();
        let when = if age == "now" || age.is_empty() {
            "just now".to_string()
        } else {
            format!("{age} ago")
        };
        format!("cached · {} · {when}", status_label(&state.status))
    } else {
        match state.elapsed {
            Some(elapsed) => {
                format!("{} · pull took {:.2}s", status_label(&state.status), elapsed.as_secs_f64())
            }
            None => status_label(&state.status).to_string(),
        }
    };
    lines.push(plain("Status", status_value));

    // Branch — clickable to its page on the remote when the remote is browsable.
    let branch = state.branch.clone().unwrap_or_else(|| "—".to_string());
    let branch_link = (branch != "—")
        .then(|| state.remote_url.as_deref())
        .flatten()
        .and_then(web_remote)
        .map(|base| format!("{base}/tree/{branch}"));
    match branch_link {
        Some(url) => push_link(&mut lines, &mut clicks, "Branch", &branch, &url),
        None => lines.push(plain("Branch", branch)),
    }

    // Pull Request — the open PR for the current branch (via `gh`), clickable to the PR on the
    // remote. Shown only when one exists; a dim "checking…" appears while the lookup is in flight.
    if let Some(pr) = state.pr.as_ref() {
        let text = format!("#{} {}", pr.number, pr.title);
        push_link(&mut lines, &mut clicks, "Pull Request", &text, &pr.url);
        // A dim "checked … ago" sub-line (per-entry cache timestamp).
        if let Some(checked_at) = state.pr_checked_at {
            let age = crate::app::format_cache_age(checked_at);
            let when = if age == "now" { "just now".to_string() } else { format!("{age} ago") };
            lines.push(Line::from(vec![
                Span::raw(format!("{:<13}", "")),
                Span::styled(format!("checked {when}"), dim),
            ]));
        }
    } else if state.pr_loading {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<13}", "Pull Request"), label),
            Span::styled("checking…", dim),
        ]));
    }

    // Pulled — what the most recent pull delivered: the old→new sha (the before/after the user
    // wants to see), commit/file counts, and best-effort new tags/branches. Shown only when a
    // pull updated this repo this session with a real delta.
    if let Some(pull) = state.pull_result.as_ref().filter(|result| result.has_delta()) {
        // Line 1: prev → new sha. The new sha links to the commit on the remote when browsable.
        let mut spans: Vec<Span> = vec![Span::styled(format!("{:<13}", "Pulled"), label)];
        if !pull.prev_head.is_empty() {
            spans.push(Span::styled(pull.prev_head.clone(), dim));
            spans.push(Span::styled(" → ", Style::default().fg(Color::Green)));
        }
        let new_link = state
            .remote_url
            .as_deref()
            .and_then(web_remote)
            .filter(|_| !pull.new_head.is_empty())
            .map(|base| format!("{base}/commit/{}", pull.new_head));
        if let Some(url) = &new_link {
            let prefix_w: usize = spans.iter().map(|span| span.width()).sum();
            let sha_w = UnicodeWidthStr::width(pull.new_head.as_str()) as u16;
            clicks.push((lines.len(), prefix_w as u16, prefix_w as u16 + sha_w, InfoAction::OpenUrl(url.clone())));
        }
        spans.push(Span::styled(pull.new_head.clone(), if new_link.is_some() { link } else { value }));
        lines.push(Line::from(spans));

        // Line 2: N commits · M files (+ins −del).
        let plural = |count: u32, word: &str| if count == 1 { word.to_string() } else { format!("{word}s") };
        lines.push(Line::from(vec![
            Span::raw(format!("{:<13}", "")),
            Span::styled(
                format!(
                    "{} {} · {} {} ",
                    pull.commits,
                    plural(pull.commits, "commit"),
                    pull.files,
                    plural(pull.files, "file"),
                ),
                value,
            ),
            Span::styled("(", dim),
            Span::styled(format!("+{}", pull.insertions), Style::default().fg(Color::Green)),
            Span::raw(" "),
            Span::styled(format!("−{}", pull.deletions), Style::default().fg(Color::Red)),
            Span::styled(")", dim),
        ]));

        // Line 3 (optional): best-effort new tags / branches.
        if pull.new_tags > 0 || pull.new_branches > 0 {
            let mut parts: Vec<String> = Vec::new();
            if pull.new_tags > 0 {
                parts.push(format!("{} new {}", pull.new_tags, plural(pull.new_tags, "tag")));
            }
            if pull.new_branches > 0 {
                let word = if pull.new_branches == 1 { "branch" } else { "branches" };
                parts.push(format!("{} new {word}", pull.new_branches));
            }
            lines.push(Line::from(vec![
                Span::raw(format!("{:<13}", "")),
                Span::styled(parts.join(" · "), dim),
            ]));
        }
    }

    if let Some(details) = &state.details {
        // Ahead/behind — hidden when there's nothing to report (both zero, or no upstream).
        if let (Some(ahead), Some(behind)) = (details.ahead, details.behind) {
            if ahead > 0 || behind > 0 {
                lines.push(plain("Ahead/behind", format!("↑{ahead}  ↓{behind}")));
            }
        }
        // Last commit — sha clickable to the commit on the remote, then subject (expandable) + meta.
        if !details.commit_hash.is_empty() {
            let sha = details.commit_hash.clone();
            let commit_link = state
                .remote_url
                .as_deref()
                .and_then(web_remote)
                .map(|base| format!("{base}/commit/{sha}"));
            match commit_link {
                Some(url) => {
                    let width = UnicodeWidthStr::width(sha.as_str()) as u16;
                    clicks.push((lines.len(), LABEL_W as u16, LABEL_W as u16 + width, InfoAction::OpenUrl(url)));
                    lines.push(Line::from(vec![
                        Span::styled(format!("{:<13}", "Last commit"), label),
                        Span::styled(sha, link),
                    ]));
                }
                None => lines.push(plain("Last commit", sha)),
            }
            // Subject: one truncated line (click to expand + wrap), or fully wrapped when expanded.
            let expanded = app.info_expanded.contains("commit");
            let subject_overflows = UnicodeWidthStr::width(details.commit_subject.as_str()) > value_width;
            if expanded && subject_overflows {
                for chunk in wrap_chars(&details.commit_subject, value_width) {
                    let width = UnicodeWidthStr::width(chunk.as_str()) as u16;
                    clicks.push((lines.len(), LABEL_W as u16, LABEL_W as u16 + width, InfoAction::ToggleExpand("commit".into())));
                    lines.push(Line::from(vec![
                        Span::raw(format!("{:<13}", "")),
                        Span::styled(chunk, value),
                    ]));
                }
            } else {
                let shown = truncate_str(&details.commit_subject, value_width);
                let subject_style = if subject_overflows { value.add_modifier(Modifier::UNDERLINED) } else { value };
                if subject_overflows {
                    let width = UnicodeWidthStr::width(shown.as_str()) as u16;
                    clicks.push((lines.len(), LABEL_W as u16, LABEL_W as u16 + width, InfoAction::ToggleExpand("commit".into())));
                }
                lines.push(Line::from(vec![
                    Span::raw(format!("{:<13}", "")),
                    Span::styled(shown, subject_style),
                ]));
            }
            lines.push(Line::from(vec![
                Span::raw(format!("{:<13}", "")),
                Span::styled(
                    truncate_str(
                        &format!("({}, {})", details.commit_rel_date, details.commit_author),
                        value_width,
                    ),
                    dim,
                ),
            ]));
        }
        // Changes — hidden when everything is zero; each part shown only when non-zero.
        if details.dirty_count > 0 || details.stash_count > 0 || details.branch_count > 0 {
            let mut parts: Vec<String> = Vec::new();
            if details.dirty_count > 0 {
                parts.push(format!("{} uncommitted", details.dirty_count));
            }
            if details.stash_count > 0 {
                parts.push(format!("{} stashed", details.stash_count));
            }
            if details.branch_count > 0 {
                parts.push(format!("{} feature branches", details.branch_count));
            }
            lines.push(plain("Changes", parts.join(" · ")));
        }
    } else {
        lines.push(plain("Ahead/behind", "(loading…)".to_string()));
        lines.push(plain("Last commit", "(loading…)".to_string()));
    }

    if let Some(url) = &state.remote_url {
        push_link(&mut lines, &mut clicks, "Remote", url, url);
    }

    // Worktrees — hidden when there are none.
    let worktrees: Vec<String> = app
        .worktrees
        .iter()
        .filter(|entry| entry.repo == state.name)
        .map(|entry| entry.branch.clone())
        .collect();
    if !worktrees.is_empty() {
        lines.push(plain("Worktrees", worktrees.join(", ")));
    }

    // Path — value left-truncated (keeps the filename tail), click to expand + wrap. A trailing
    // `⧉` copy button sits AFTER the value so the value column stays aligned with the other rows.
    let path = state.path.display().to_string();
    let path_expanded = app.info_expanded.contains("Path");
    let path_overflows = UnicodeWidthStr::width(path.as_str()) > value_width;
    // Reserve 2 cols on lines that carry the copy button (` ⧉`).
    let copy_avail = value_width.saturating_sub(2).max(1);
    let push_path_line =
        |lines: &mut Vec<Line<'static>>, clicks: &mut Vec<InfoClick>, first: bool, text: String, with_copy: bool| {
            let line_idx = lines.len();
            let value_w = UnicodeWidthStr::width(text.as_str()) as u16;
            if path_overflows {
                clicks.push((line_idx, LABEL_W as u16, LABEL_W as u16 + value_w, InfoAction::ToggleExpand("Path".into())));
            }
            let label_span = if first {
                Span::styled(format!("{:<13}", "Path"), label)
            } else {
                Span::raw(format!("{:<13}", ""))
            };
            let value_style = if path_overflows && !path_expanded {
                value.add_modifier(Modifier::UNDERLINED)
            } else {
                value
            };
            let mut spans = vec![label_span, Span::styled(text, value_style)];
            if with_copy {
                let copy_col = LABEL_W as u16 + value_w + 1;
                clicks.push((line_idx, copy_col, copy_col + 1, InfoAction::CopyText(path.clone())));
                spans.push(Span::raw(" "));
                // A copy button, not a link — cyan + bold (matching the pane-title `⧉`), no underline.
                spans.push(Span::styled(
                    "⧉".to_string(),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ));
            }
            lines.push(Line::from(spans));
        };
    if !path_overflows {
        push_path_line(&mut lines, &mut clicks, true, path.clone(), true);
    } else if path_expanded {
        for (index, chunk) in wrap_chars(&path, copy_avail).into_iter().enumerate() {
            push_path_line(&mut lines, &mut clicks, index == 0, chunk, index == 0);
        }
    } else {
        push_path_line(&mut lines, &mut clicks, true, truncate_left(&path, copy_avail), true);
    }

    (lines, clicks)
}

/// Render an info block (border + pre-wrapped lines + scrollbar) into `area`, and translate each
/// clickable region's in-line columns into absolute screen rects on `app.info_click`.
/// Render the pinned info panel for `repo_idx` into `area` (sized by the caller — full pane or the
/// top half of a split). Clips to fit; the info content is short.
fn render_info_panel(frame: &mut Frame, app: &mut AppState, area: Rect, repo_idx: usize) {
    let name = app.repos[repo_idx].lock().unwrap().name.clone();
    let info_width = area.width.saturating_sub(if app.panel_padding { 4 } else { 2 }) as usize;
    let (lines, clicks) = build_info_lines(app, repo_idx, info_width);
    render_info_block(frame, app, area, format!(" {name} · info "), lines, clicks);
}

/// Render the placeholder shown when the result/log panel is hidden and there's no info panel to
/// fill the pane — a bordered box with a centered hint on how to bring the panel back.
fn render_preview_hidden_hint(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let modal_open = app.any_modal_open();
    let block = Block::default()
        .title(" [2] ")
        .title_style(pane_title_style(modal_open))
        .borders(pane_borders(app))
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.preview_focused, modal_open));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height >= 1 {
        let hint = Line::from(Span::styled(
            "result panel hidden — I to show",
            Style::default().fg(Color::DarkGray),
        ))
        .centered();
        let mid = Rect { y: inner.y + inner.height / 2, height: 1, ..inner };
        frame.render_widget(Paragraph::new(hint), mid);
    }
}

fn render_info_block(
    frame: &mut Frame,
    app: &mut AppState,
    area: Rect,
    title: String,
    lines: Vec<Line<'static>>,
    clicks: Vec<InfoClick>,
) {
    let modal_open = app.any_modal_open();
    let block = Block::default()
        .title(title)
        .title_style(pane_title_style(modal_open))
        .borders(pane_borders(app))
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.preview_focused, modal_open));
    let inner = block.inner(area);
    let total = lines.len();
    frame.render_widget(block, area);
    // Lines are already wrapped to the inner width, so render them verbatim (no Paragraph wrap)
    // — that keeps line N at row inner.y + N, which the click translation below relies on.
    let visible = (inner.height as usize).min(lines.len());
    frame.render_widget(Paragraph::new(lines), inner);
    for (line_idx, start, end, action) in clicks {
        if line_idx < visible {
            app.info_click.push((
                inner.y + line_idx as u16,
                inner.x + start,
                inner.x + end,
                action,
            ));
        }
    }
    render_scrollbar(frame, scrollbar_track(area, inner), 0, total, inner.height as usize, false);
}

/// Convert a string that may contain ANSI escape codes to a ratatui Line.
/// We use a simple parser for the common SGR codes git produces.
fn ansi_line_to_ratatui(line: &str) -> Line<'static> {
    let mut spans = Vec::new();
    let mut current_style = Style::default();
    let mut current_text = String::new();

    // Iterate by char, not byte: SGR sequences are all ASCII, while log/commit text can hold
    // multi-byte UTF-8. Pushing raw bytes as chars corrupts those into mojibake + C1 controls.
    let chars: Vec<char> = line.chars().collect();
    let mut pos = 0;

    while pos < chars.len() {
        if chars[pos] == '\x1b' && pos + 1 < chars.len() && chars[pos + 1] == '[' {
            // ESC [ ... m — SGR sequence
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }
            pos += 2;
            let start = pos;
            while pos < chars.len() && chars[pos] != 'm' {
                pos += 1;
            }
            if pos < chars.len() {
                let code_str: String = chars[start..pos].iter().collect();
                current_style = apply_sgr(current_style, &code_str);
                pos += 1; // skip 'm'
            }
        } else {
            current_text.push(chars[pos]);
            pos += 1;
        }
    }

    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    Line::from(spans)
}

fn apply_sgr(style: Style, code_str: &str) -> Style {
    for code in code_str.split(';') {
        let code = code.trim().parse::<u8>().unwrap_or(0);
        match code {
            0 => return Style::default(),
            1 => return style.add_modifier(Modifier::BOLD),
            2 => return style.add_modifier(Modifier::DIM),
            4 => return style.add_modifier(Modifier::UNDERLINED),
            7 => return style.add_modifier(Modifier::REVERSED),
            30 => return style.fg(Color::Black),
            31 => return style.fg(Color::Red),
            32 => return style.fg(Color::Green),
            33 => return style.fg(Color::Yellow),
            34 => return style.fg(Color::Blue),
            35 => return style.fg(Color::Magenta),
            36 => return style.fg(Color::Cyan),
            37 => return style.fg(Color::White),
            90 => return style.fg(Color::DarkGray),
            91 => return style.fg(Color::LightRed),
            92 => return style.fg(Color::LightGreen),
            93 => return style.fg(Color::LightYellow),
            94 => return style.fg(Color::LightBlue),
            95 => return style.fg(Color::LightMagenta),
            96 => return style.fg(Color::LightCyan),
            97 => return style.fg(Color::Gray),
            _ => {}
        }
    }
    style
}

fn build_result_summary(app: &AppState) -> Vec<String> {
    let mut lines = Vec::new();

    let (
        idle_count,
        _,
        updated_count,
        up_to_date_count,
        skipped_count,
        failed_count,
        no_upstream_count,
        throttled_count,
    ) = app.counts();

    let total = idle_count
        + updated_count
        + up_to_date_count
        + skipped_count
        + failed_count
        + no_upstream_count
        + throttled_count;

    // When the launch skipped auto-pull, the run "completes" without pulling — say so.
    lines.push(if app.auto_pull_suppressed {
        "Ready — auto-pull off.".to_string()
    } else {
        "Pull completed!".to_string()
    });
    lines.push(String::new());

    if total == 0 {
        lines.push("   No git repositories found.".to_string());
        return lines;
    }

    let mut parts = Vec::new();
    if updated_count > 0 {
        parts.push(format!("{updated_count} updated"));
    }
    if up_to_date_count > 0 {
        parts.push(format!("{up_to_date_count} up-to-date"));
    }
    if skipped_count > 0 {
        parts.push(format!("{skipped_count} skipped"));
    }
    if no_upstream_count > 0 {
        parts.push(format!("{no_upstream_count} no-upstream"));
    }
    if throttled_count > 0 {
        parts.push(format!("{throttled_count} throttled"));
    }
    if idle_count > 0 {
        parts.push(format!("{idle_count} idle"));
    }
    if failed_count > 0 {
        parts.push(format!("{failed_count} failed"));
    }

    lines.push(format!("   {total} total: {}", parts.join(", ")));
    if app.auto_pull_suppressed {
        lines.push(String::new());
        lines.push("   showing last-known (cached) status — press E to pull all, e for the selected repo".to_string());
    }

    // Compute padding width — include worktree repo names too
    let mut pad = 0;
    for repo in &app.repos {
        let name_len = repo.lock().unwrap().name.len();
        if name_len > pad {
            pad = name_len;
        }
    }
    for wt in &app.worktrees {
        if wt.repo.len() > pad {
            pad = wt.repo.len();
        }
    }

    // Collect repos by status
    let collect_by_status = |status_fn: &dyn Fn(&RepoStatus) -> bool| -> Vec<(String, String)> {
        app.repos
            .iter()
            .filter(|repo| {
                let state = repo.lock().unwrap();
                status_fn(&state.status)
            })
            .map(|repo| {
                let state = repo.lock().unwrap();
                (
                    state.name.clone(),
                    state.branch.clone().unwrap_or_else(|| "?".to_string()),
                )
            })
            .collect()
    };

    let updated_repos = collect_by_status(&|status| matches!(status, RepoStatus::Updated));
    let up_to_date_repos =
        collect_by_status(&|status| matches!(status, RepoStatus::UpToDate));
    let skipped_repos = collect_by_status(&|status| matches!(status, RepoStatus::Skipped));
    let no_upstream_repos = collect_by_status(&|status| matches!(status, RepoStatus::NoUpstream));
    let throttled_repos = collect_by_status(&|status| matches!(status, RepoStatus::Throttled));
    let failed_repos = collect_by_status(&|status| matches!(status, RepoStatus::Failed));

    let print_section = |lines: &mut Vec<String>, header: &str, repos: &[(String, String)]| {
        if repos.is_empty() {
            return;
        }
        lines.push(String::new());
        lines.push(header.to_string());
        for (name, branch) in repos {
            lines.push(format!("   - {name:<pad$}  {branch}"));
        }
    };

    // Section markers: ASCII in Unicode mode, matching status glyphs in emoji mode.
    let icons = app.icons();
    let emoji = app.icon_style == crate::app::IconStyle::Emoji;
    let mark = |ascii: &'static str, glyph: &'static str| if emoji { glyph } else { ascii };
    print_section(
        &mut lines,
        &format!("{} Updated repositories:", mark("+", icons.updated)),
        &updated_repos,
    );
    print_section(
        &mut lines,
        &format!("{} Unchanged repositories:", mark("=", icons.up_to_date)),
        &up_to_date_repos,
    );
    print_section(
        &mut lines,
        &format!("{} Skipped repositories (uncommitted changes):", mark("!", icons.skipped)),
        &skipped_repos,
    );
    print_section(
        &mut lines,
        &format!("{} No-upstream repositories (nothing to pull):", mark("~", icons.no_upstream)),
        &no_upstream_repos,
    );
    print_section(
        &mut lines,
        &format!("{} Throttled repositories (rate-limited; retrying):", mark("!", icons.throttled)),
        &throttled_repos,
    );
    print_section(
        &mut lines,
        &format!("{} Failed repositories:", mark("x", icons.failed)),
        &failed_repos,
    );

    if !app.worktrees.is_empty() {
        lines.push(String::new());
        lines.push(format!("{} Active worktrees:", mark(">", icons.worktrees)));
        for wt in &app.worktrees {
            lines.push(format!("   - {:<pad$}  {}", wt.repo, wt.branch));
        }
    }

    lines
}

/// Right-pane content for the dynamic Errors row: each failed repo with the tail of its log
/// (the git stderr from the final failed attempt).
fn build_error_summary(app: &AppState) -> Vec<String> {
    const TAIL: usize = 15;
    let icons = app.icons();
    let mut lines = Vec::new();
    let failed_count = app.counts().5;
    lines.push(format!("{failed_count} repo(s) failed to pull:"));

    for repo in &app.repos {
        let state = repo.lock().unwrap();
        if !matches!(state.status, RepoStatus::Failed) {
            continue;
        }
        let branch = state.branch.clone().unwrap_or_else(|| "?".to_string());
        lines.push(String::new());
        lines.push(format!("{} {} ({branch})", icons.failed, state.name));
        let log: Vec<&String> = state.log.lines().iter().collect();
        let start = log.len().saturating_sub(TAIL);
        if start > 0 {
            lines.push(format!("   …{start} earlier line(s)"));
        }
        for line in &log[start..] {
            lines.push(format!("   {line}"));
        }
    }

    lines
}


/// Build one status-bar row from (text, style, optional command) segments, recording a
/// `ClickRegion` for each actionable segment at its screen columns.
/// Build the group preview shown when a group header is selected: source, membership,
/// per-status counts, cache age, and any resolution error.
/// The folder-node preview: its path, repo + subfolder counts, and the subtree status breakdown.
fn build_folder_summary(app: &AppState, node_idx: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let field = |name: &str, value: String| format!("{name:<13}{value}");
    let Some(node) = app.tree_nodes.get(node_idx) else {
        return lines;
    };
    lines.push(field("Folder", format!("{}/", node.rel_path)));
    let subtree = app.tree_subtree_repos(node_idx);
    lines.push(field("Repos", format!("{} (subtree)", subtree.len())));
    if !node.children.is_empty() {
        let names: Vec<String> =
            node.children.iter().filter_map(|&idx| app.tree_nodes.get(idx)).map(|child| child.name.clone()).collect();
        lines.push(field("Subfolders", format!("{} · {}", names.len(), names.join(", "))));
    }

    let mut parts = Vec::new();
    let mut counts = [0usize; 8];
    for &repo_idx in &subtree {
        let idx = match app.repos[repo_idx].lock().unwrap().status {
            RepoStatus::Running { .. } => 0,
            RepoStatus::Queued => 1,
            RepoStatus::Updated => 2,
            RepoStatus::UpToDate => 3,
            RepoStatus::Skipped => 4,
            RepoStatus::NoUpstream => 5,
            RepoStatus::Throttled => 6,
            RepoStatus::Failed => 7,
        };
        counts[idx] += 1;
    }
    for (count, label) in [
        (counts[0], "running"),
        (counts[1], "queued"),
        (counts[2], "updated"),
        (counts[3], "up-to-date"),
        (counts[4], "skipped"),
        (counts[5], "no-upstream"),
        (counts[6], "throttled"),
        (counts[7], "failed"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {label}"));
        }
    }
    if !parts.is_empty() {
        lines.push(field("Status", parts.join(", ")));
    }
    lines.push(String::new());
    lines.push("enter/space/←/→ to collapse or expand".to_string());
    lines
}

fn build_group_summary(app: &AppState, group_idx: usize) -> Vec<String> {
    let members = app.group_visible_members(group_idx);
    let mut queued = 0usize;
    let mut running = 0usize;
    let mut updated = 0usize;
    let mut up_to_date = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut no_upstream = 0usize;
    let mut throttled = 0usize;
    for &repo_idx in &members {
        match app.repos[repo_idx].lock().unwrap().status {
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

    let mut lines = Vec::new();
    let field = |name: &str, value: String| format!("{name:<13}{value}");
    match app.groups.get(group_idx) {
        Some(group) => {
            lines.push(field("Group", group.name.clone()));
            lines.push(field(
                "Source",
                format!("{} · {}", group.source.kind_label(), group.source.detail()),
            ));
            let membership = match &group.members {
                Some(members_total) => format!("{} ({} visible)", members_total.len(), members.len()),
                None if group.source.is_dynamic() => "(unresolved)".to_string(),
                None => format!("by pattern ({} visible)", members.len()),
            };
            lines.push(field("Members", membership));
            if group.source.is_dynamic() {
                let age = match group.resolved_at {
                    Some(at) => {
                        let minutes = crate::groups::now_unix().saturating_sub(at) / 60;
                        match minutes {
                            0 => "resolved just now".to_string(),
                            1..=119 => format!("resolved {minutes}m ago"),
                            _ => format!("resolved {}h ago", minutes / 60),
                        }
                    }
                    None => "never resolved".to_string(),
                };
                lines.push(field("Cache", age));
            }
            if group.resolving {
                lines.push(field("Refresh", "resolving…".to_string()));
            }
            if let Some(error) = &group.error {
                lines.push(String::new());
                lines.push(format!("\u{1b}[31mError: {error}\u{1b}[0m"));
            }
        }
        None => {
            lines.push(field("Group", "ungrouped".to_string()));
            lines.push(field("Source", "repos matching no configured group".to_string()));
            lines.push(field("Members", format!("{} visible", members.len())));
        }
    }

    let mut parts = Vec::new();
    for (count, label) in [
        (running, "running"),
        (queued, "queued"),
        (updated, "updated"),
        (up_to_date, "up-to-date"),
        (skipped, "skipped"),
        (no_upstream, "no-upstream"),
        (throttled, "throttled"),
        (failed, "failed"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {label}"));
        }
    }
    if !parts.is_empty() {
        lines.push(field("Status", parts.join(", ")));
    }
    lines
}

fn build_status_row(
    segments: Vec<(String, Style, Option<Command>)>,
    start_col: u16,
    row: u16,
    clickable: &mut Vec<ClickRegion>,
) -> Line<'static> {
    let mut spans = Vec::with_capacity(segments.len());
    let mut col = start_col;
    for (text, style, command) in segments {
        let width = UnicodeWidthStr::width(text.as_str()) as u16;
        if let Some(command) = command {
            clickable.push(ClickRegion {
                row,
                col_start: col,
                col_end: col + width,
                command,
            });
        }
        col = col.saturating_add(width);
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// Build a styled, clickable hint footer (the root status-bar look: bold accent keys, dim
/// labels, `·` separators) from `(text, style, Option<HintKey>)` segments, laid out left to
/// right from `start_col` on `row`. Keyed segments register a `HintClick`; clicking one injects
/// that key, so the hint runs the exact same handler as the real keypress.
fn build_hint_footer(
    segments: Vec<(String, Style, Option<HintKey>)>,
    start_col: u16,
    row: u16,
    hint_click: &mut Vec<HintClick>,
) -> Line<'static> {
    let mut spans = Vec::with_capacity(segments.len());
    let mut col = start_col;
    for (text, style, key) in segments {
        let width = UnicodeWidthStr::width(text.as_str()) as u16;
        if let Some(key) = key {
            hint_click.push(HintClick { row, col_start: col, col_end: col + width, key });
        }
        col = col.saturating_add(width);
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// Build a clickable hint footer for a modal's **bottom border**: lays the styled hint out
/// left-to-right just inside the left corner and registers each keyed segment's `HintClick` at the
/// border row (so a click injects the key and runs the same handler as the keypress). Attach the
/// returned `Line` via `.title_bottom(...)`. This is what makes every modal's footer clickable +
/// hover-highlighted, instead of a plain `title_bottom(Line::from("…"))`.
fn modal_border_footer(
    segments: Vec<(String, Style, Option<HintKey>)>,
    modal_area: Rect,
    hint_click: &mut Vec<HintClick>,
) -> Line<'static> {
    let footer_row = modal_area.y + modal_area.height.saturating_sub(1);
    build_hint_footer(segments, modal_area.x + 1, footer_row, hint_click)
}

/// A `key`-styled / `hint`-styled `[key, label]` segment pair for `modal_border_footer`, both
/// clickable as `key`. The common shape for footer chips like `esc close` / `r restart`.
fn footer_chip(key_text: &str, label: &str, key: HintKey) -> [(String, Style, Option<HintKey>); 2] {
    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(Color::DarkGray);
    [
        (key_text.to_string(), key_style, Some(key)),
        (label.to_string(), hint_style, Some(key)),
    ]
}

/// A non-clickable ` · ` separator segment for footer chips.
fn footer_sep() -> (String, Style, Option<HintKey>) {
    (" · ".to_string(), Style::default().fg(Color::DarkGray), None)
}

/// Pack `chips` (each an indivisible segment group) into as many rows as needed so each fits
/// `area.width`, separated by ` · `. Row 0 starts with `prefix`; later rows start flush left.
/// Click regions are registered per row at `base_y + row`. Used by the column picker, which has
/// more chips than fit on one status row.
fn pack_chips_into_rows(
    prefix: Vec<(String, Style, Option<Command>)>,
    chips: Vec<Vec<(String, Style, Option<Command>)>>,
    area: Rect,
    base_y: u16,
    clickable: &mut Vec<ClickRegion>,
    hint: Style,
) -> Vec<Line<'static>> {
    let sep_w = UnicodeWidthStr::width(" · ") as u16;
    let group_w = |group: &[(String, Style, Option<Command>)]| -> u16 {
        group.iter().map(|(text, _, _)| UnicodeWidthStr::width(text.as_str()) as u16).sum()
    };
    let mut rows: Vec<Vec<(String, Style, Option<Command>)>> = Vec::new();
    let mut current = prefix;
    let mut current_w = group_w(&current);
    // The prefix carries its own trailing space, so the first chip joins flush (no ` · `).
    let mut first_in_row = true;
    for chip in chips {
        let chip_w = group_w(&chip);
        if !first_in_row && current_w + sep_w + chip_w > area.width {
            rows.push(std::mem::take(&mut current));
            current_w = 0;
            first_in_row = true;
        }
        if !first_in_row {
            current.push((" · ".to_string(), hint, None));
            current_w += sep_w;
        }
        current.extend(chip);
        current_w += chip_w;
        first_in_row = false;
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows.into_iter()
        .enumerate()
        .map(|(index, segments)| build_status_row(segments, area.x, base_y + index as u16, clickable))
        .collect()
}

/// Clip a string to at most `max` display cells (no ellipsis appended).
fn clip_to_width(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + char_width > max {
            break;
        }
        width += char_width;
        out.push(ch);
    }
    out
}

/// Clip a span list to `max` display cells, truncating the span that straddles the boundary.
fn clip_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0usize;
    for span in spans {
        if used >= max {
            break;
        }
        let width = UnicodeWidthStr::width(span.content.as_ref());
        if used + width <= max {
            used += width;
            out.push(span);
        } else {
            let style = span.style;
            out.push(Span::styled(clip_to_width(span.content.as_ref(), max - used), style));
            break;
        }
    }
    out
}

/// Build a footer row from clickable left segments plus right-aligned segments (justify-
/// between); the right side is clickable too. When the two sides have room, the gap is plain
/// spaces; when they'd touch or overlap, the left is truncated with `…` and a `·` separator.
fn compose_status_row(
    segments: Vec<(String, Style, Option<Command>)>,
    right: Vec<(String, Style, Option<Command>)>,
    area: Rect,
    row_y: u16,
    clickable: &mut Vec<ClickRegion>,
    hint: Style,
) -> Line<'static> {
    let left_width: usize = segments
        .iter()
        .map(|(text, _, _)| UnicodeWidthStr::width(text.as_str()))
        .sum();
    let right_width: usize = right
        .iter()
        .map(|(text, _, _)| UnicodeWidthStr::width(text.as_str()))
        .sum();
    let mut line = build_status_row(segments, area.x, row_y, clickable);
    let avail = area.width as usize;
    if right_width == 0 || avail == 0 {
        return line;
    }
    if left_width + right_width + 3 <= avail {
        line.spans.push(Span::raw(" ".repeat(avail - left_width - right_width)));
    } else {
        let keep = avail.saturating_sub(right_width + 4);
        line.spans = clip_spans(std::mem::take(&mut line.spans), keep);
        line.spans.push(Span::styled("… · ".to_string(), hint));
    }
    let right_start = area.x + (avail - right_width) as u16;
    let right_line = build_status_row(right, right_start, row_y, clickable);
    line.spans.extend(right_line.spans);
    line
}

/// Style a footer segment list for the current footer state, returning inert (`None`-command),
/// dimmed segments where a command can't run right now. When a **modal** is open everything goes
/// inert except `settings`/`help`/`quit` (which stay live); when a **leader** menu is armed
/// everything goes inert except the leader's trigger (which gets a highlight pill); otherwise each
/// command dims when `command_applicable` is false. Non-command separators recede with the row only
/// under a modal/leader, so a single disabled command doesn't dim its neighbors' separators.
fn style_footer(
    app: &AppState,
    segments: Vec<(String, Style, Option<Command>)>,
    modal_open: bool,
    leader_active: bool,
    leader_trigger: Option<Command>,
    dim: Style,
) -> Vec<(String, Style, Option<Command>)> {
    let pill = Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD);
    segments
        .into_iter()
        .map(|(text, style, command)| match command {
            Some(cmd) if modal_open => {
                if matches!(cmd, Command::Settings | Command::Help | Command::Quit) {
                    (text, style, Some(cmd))
                } else {
                    (text, dim, None)
                }
            }
            Some(cmd) if leader_active => {
                if Some(cmd) == leader_trigger {
                    (text, pill, Some(cmd))
                } else {
                    (text, dim, None)
                }
            }
            Some(cmd) if !app.command_applicable(cmd) => (text, dim, None),
            Some(cmd) => (text, style, Some(cmd)),
            None if modal_open || leader_active => (text, dim, None),
            None => (text, style, None),
        })
        .collect()
}

fn render_status_bar(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let hint = Style::default().fg(Color::DarkGray);
    let active = Style::default().fg(Color::Gray);
    // Keycaps: accent + bold when the action is available; `style_footer` fades them to `dim_style`
    // and makes them inert when the command can't run (no-op, leader armed, or a modal is open).
    let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);

    let filtering = app.filter_input_mode;
    let filter_text = app.filter.clone().unwrap_or_default();
    let leader = app.pending_leader;
    let modal_open = app.any_modal_open();
    let leader_active = leader.is_some();
    let leader_trigger = match leader {
        Some(Leader::Filter) => Some(Command::FilterLeader),
        Some(Leader::Sort) => Some(Command::SortLeader),
        Some(Leader::Toggle) => Some(Command::ToggleLeader),
        _ => None,
    };
    let columns = app.columns;
    let avail = (
        app.column_available(Column::Worktrees),
        app.column_available(Column::Branches),
        app.column_available(Column::Stashes),
        app.column_available(Column::PulledCommits),
        app.column_available(Column::PulledFiles),
    );
    // Which sort options the `s` menu offers — only columns currently visible on screen.
    let sort_vis = |column: SortColumn| app.sort_column_visible(column);
    let status_filter = app.status_filter;
    let sort_column = app.sort_column;
    let sort_dir = app.sort_dir;
    let grouping_on = app.grouping_active();
    let tree_on = app.tree_active();

    // Right-aligned fragments (justify-between): the list title already shows done/elapsed,
    // so the right side carries the version, the binary's build age, and the meta actions.
    let right_version: Vec<(String, Style, Option<Command>)> =
        vec![(concat!("v", env!("CARGO_PKG_VERSION")).to_string(), hint, None)];
    let right_built: Vec<(String, Style, Option<Command>)> = app
        .binary_built
        .and_then(|built| built.elapsed().ok())
        .map(|age| {
            vec![(
                format!("built {}", crate::app::format_ago(age.as_secs())),
                hint,
                Some(Command::ShowBuildInfo),
            )]
        })
        .unwrap_or_default();
    let right_meta: Vec<(String, Style, Option<Command>)> = vec![
        (",".to_string(), key, Some(Command::Settings)),
        (" settings".to_string(), hint, Some(Command::Settings)),
        (" · ".to_string(), hint, None),
        ("?".to_string(), key, Some(Command::Help)),
        (" help".to_string(), hint, Some(Command::Help)),
        (" · ".to_string(), hint, None),
        ("q".to_string(), key, Some(Command::Quit)),
        // Inside a modal, `q` closes the modal rather than quitting — label it dynamically.
        (if modal_open { " close" } else { " quit" }.to_string(), hint, Some(Command::Quit)),
    ];

    let mut clickable: Vec<ClickRegion> = Vec::new();
    let mark = |on: bool| if on { "[x]" } else { "[ ]" };
    // A leader-menu item as three segments so its hotkey letter pops in the key color.
    let leader_item = |prefix: String,
                       letter: &str,
                       label: String,
                       command: Command|
     -> [(String, Style, Option<Command>); 3] {
        [
            (prefix, active, Some(command)),
            (letter.to_string(), key, Some(command)),
            (format!(" {label}"), active, Some(command)),
        ]
    };

    // The column picker (`t`) has more chips than fit one row, so pack it across as many status
    // rows as needed; when it wraps it takes over the find row (row 2) while open.
    let toggle_lines: Option<Vec<Line>> = if leader == Some(Leader::Toggle) {
        let toggle_item = |on: bool, letter: &str, label: &str, column: Column| {
            leader_item(
                format!("{} ", mark(on)),
                letter,
                label.to_string(),
                Command::ToggleColumn(column),
            )
        };
        // An unavailable column (no repo has any) renders dim and inert — visible but disabled.
        let disabled_item = |letter: &str, label: &str| {
            [
                ("[ ] ".to_string(), hint, None),
                (letter.to_string(), hint, None),
                (format!(" {label} (none)"), hint, None),
            ]
        };
        let entries = [
            toggle_item(columns.status, "u", "status", Column::Status),
            toggle_item(columns.ahead_behind, "a", "ahead/behind", Column::AheadBehind),
            toggle_item(columns.dirty, "d", "dirty", Column::Dirty),
            toggle_item(columns.last_commit, "l", "last-commit", Column::LastCommit),
            if avail.0 {
                toggle_item(columns.worktrees, "w", "worktrees", Column::Worktrees)
            } else {
                disabled_item("w", "worktrees")
            },
            if avail.1 {
                toggle_item(columns.branches, "b", "branches", Column::Branches)
            } else {
                disabled_item("b", "branches")
            },
            if avail.2 {
                toggle_item(columns.stashes, "s", "stashes", Column::Stashes)
            } else {
                disabled_item("s", "stashes")
            },
            if avail.3 {
                toggle_item(columns.pulled_commits, "p", "pulled", Column::PulledCommits)
            } else {
                disabled_item("p", "pulled")
            },
            if avail.4 {
                toggle_item(columns.pulled_files, "c", "changed", Column::PulledFiles)
            } else {
                disabled_item("c", "changed")
            },
            toggle_item(columns.pull_request, "r", "pull request", Column::PullRequest),
        ];
        let mut chips: Vec<Vec<(String, Style, Option<Command>)>> =
            entries.into_iter().map(|entry| entry.to_vec()).collect();
        chips.push(vec![("esc".to_string(), key, Some(Command::LeaderCancel))]);
        let prefix = vec![("cols: ".to_string(), hint, None)];
        Some(pack_chips_into_rows(prefix, chips, area, area.y, &mut clickable, hint))
    } else {
        None
    };

    // Row 1: the column picker's first row, the filter prompt, an active leader menu (`f` status /
    // `s` sort), or the normal navigation/filter/sort/layout hints.
    let row1 = if let Some(lines) = toggle_lines.as_ref() {
        lines.first().cloned().unwrap_or_default()
    } else if filtering {
        // `@` switches name-matching to status/attribute matching; hint at it inline.
        let in_status = filter_text.starts_with('@');
        let label = if in_status { "Filter by status: " } else { "Filter: " };
        let hint_text = if in_status {
            "  (e.g. @failed · @dirty · @ahead · @behind)"
        } else {
            "  (prepend @ to filter by status)"
        };
        Line::from(vec![
            Span::styled(label, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(format!("{filter_text}\u{2588}")),
            Span::styled(hint_text, Style::default().fg(Color::DarkGray)),
        ])
    } else if leader == Some(Leader::Filter) {
        let pick = |on: bool| if on { "●" } else { "○" };
        let filter_item = |letter: &str, label: &str, filter: StatusFilter| {
            leader_item(
                format!("{} ", pick(status_filter == filter)),
                letter,
                label.to_string(),
                Command::SetFilter(filter),
            )
        };
        let mut segments: Vec<(String, Style, Option<Command>)> =
            vec![("filter: ".to_string(), hint, None)];
        let entries = [
            filter_item("a", "all", StatusFilter::All),
            filter_item("u", "updated", StatusFilter::Updated),
            filter_item("c", "up-to-date", StatusFilter::UpToDate),
            filter_item("s", "skipped", StatusFilter::Skipped),
            filter_item("f", "failed", StatusFilter::Failed),
            filter_item("i", "issues", StatusFilter::Issues),
        ];
        for (index, entry) in entries.into_iter().enumerate() {
            if index > 0 {
                segments.push((" · ".to_string(), hint, None));
            }
            segments.extend(entry);
        }
        segments.push((" · ".to_string(), hint, None));
        segments.push(("esc".to_string(), key, Some(Command::LeaderCancel)));
        // No right fragment while a leader menu is up — the menu needs the full row width.
        compose_status_row(segments, Vec::new(), area, area.y, &mut clickable, hint)
    } else if leader == Some(Leader::Sort) {
        let sort_item = |letter: &str, name: &str, column: SortColumn| {
            let chosen = sort_column == column;
            let dot = if chosen { "●" } else { "○" };
            let arrow = if chosen { sort_dir.arrow() } else { "" };
            leader_item(
                format!("{dot} "),
                letter,
                format!("{name}{arrow}"),
                Command::SetSort(column),
            )
        };
        let mut segments: Vec<(String, Style, Option<Command>)> =
            vec![("sort: ".to_string(), hint, None)];
        // Only offer sorts whose column is actually on screen — name/branch/status/dirty are
        // always visible; the rest follow their effective column.
        let mut entries: Vec<[(String, Style, Option<Command>); 3]> = vec![
            sort_item("n", "name", SortColumn::Name),
            sort_item("c", "branch", SortColumn::Branch),
            sort_item("s", "status", SortColumn::Status),
            sort_item("d", "dirty", SortColumn::Dirty),
        ];
        if sort_vis(SortColumn::AheadBehind) {
            entries.push(sort_item("a", "ahead/behind", SortColumn::AheadBehind));
        }
        if sort_vis(SortColumn::LastCommit) {
            entries.push(sort_item("l", "last-commit", SortColumn::LastCommit));
        }
        if sort_vis(SortColumn::Worktrees) {
            entries.push(sort_item("w", "worktrees", SortColumn::Worktrees));
        }
        if sort_vis(SortColumn::Branches) {
            entries.push(sort_item("b", "branches", SortColumn::Branches));
        }
        if sort_vis(SortColumn::Stashes) {
            entries.push(sort_item("k", "stashes", SortColumn::Stashes));
        }
        if sort_vis(SortColumn::PulledCommits) {
            entries.push(sort_item("p", "pulled", SortColumn::PulledCommits));
        }
        if sort_vis(SortColumn::PulledFiles) {
            entries.push(sort_item("g", "changed", SortColumn::PulledFiles));
        }
        if sort_vis(SortColumn::PullRequest) {
            entries.push(sort_item("r", "pull request", SortColumn::PullRequest));
        }
        for (index, entry) in entries.into_iter().enumerate() {
            if index > 0 {
                segments.push((" · ".to_string(), hint, None));
            }
            segments.extend(entry);
        }
        segments.push((" · ".to_string(), hint, None));
        segments.push(("esc".to_string(), key, Some(Command::LeaderCancel)));
        // No right fragment while a leader menu is up — the menu needs the full row width.
        compose_status_row(segments, Vec::new(), area, area.y, &mut clickable, hint)
    } else if leader == Some(Leader::View) {
        let pick = |on: bool| if on { "●" } else { "○" };
        let mut segments: Vec<(String, Style, Option<Command>)> =
            vec![("view: ".to_string(), hint, None)];
        segments.extend(leader_item(
            format!("{} ", pick(grouping_on)),
            "g",
            "grouped".to_string(),
            Command::GroupingToggle,
        ));
        segments.push((" · ".to_string(), hint, None));
        segments.extend(leader_item(
            format!("{} ", pick(tree_on)),
            "t",
            "tree".to_string(),
            Command::TreeToggle,
        ));
        segments.push((" · ".to_string(), hint, None));
        segments.push(("esc".to_string(), key, Some(Command::LeaderCancel)));
        compose_status_row(segments, Vec::new(), area, area.y, &mut clickable, hint)
    } else if leader == Some(Leader::Fold) {
        let item = |letter: &str, label: &str, command: Command| {
            leader_item(String::new(), letter, label.to_string(), command)
        };
        let mut segments: Vec<(String, Style, Option<Command>)> =
            vec![("fold: ".to_string(), hint, None)];
        let entries = [
            item("-", "collapse all", Command::FoldCollapseAll),
            item("+", "expand all", Command::FoldExpandAll),
            item("*", "expand subtree", Command::FoldExpandSubtree),
        ];
        for (index, entry) in entries.into_iter().enumerate() {
            if index > 0 {
                segments.push((" · ".to_string(), hint, None));
            }
            segments.extend(entry);
        }
        segments.push((" · ".to_string(), hint, None));
        segments.push(("esc".to_string(), key, Some(Command::LeaderCancel)));
        compose_status_row(segments, Vec::new(), area, area.y, &mut clickable, hint)
    } else {
        // Row 1 — move & view. The label words are clickable too, not just the keys; the
        // info/diff labels brighten while their view is active.
        let info_label = if app.info_pinned { active } else { hint };
        let diff_label = if app.right_view == RightView::Diff { active } else { hint };
        let mut row1_segments: Vec<(String, Style, Option<Command>)> = vec![
            // `[j/]` moves down, `[k move]` moves up — both halves clickable.
            ("j".to_string(), key, Some(Command::NavDown)),
            ("/".to_string(), hint, Some(Command::NavDown)),
            ("k".to_string(), key, Some(Command::NavUp)),
            (" move".to_string(), hint, Some(Command::NavUp)),
            (" · ".to_string(), hint, None),
            ("space".to_string(), key, Some(Command::ResultOverlay)),
            (" result".to_string(), hint, Some(Command::ResultOverlay)),
            (" · ".to_string(), hint, None),
            ("i".to_string(), key, Some(Command::Info)),
            (" info".to_string(), info_label, Some(Command::Info)),
            (" · ".to_string(), hint, None),
            ("I".to_string(), key, Some(Command::ToggleResultPanel)),
            (
                " log".to_string(),
                if app.show_result_panel { active } else { hint },
                Some(Command::ToggleResultPanel),
            ),
            (" · ".to_string(), hint, None),
            ("d".to_string(), key, Some(Command::DiffView)),
            (" diff".to_string(), diff_label, Some(Command::DiffView)),
            (" · ".to_string(), hint, None),
            ("tab".to_string(), key, Some(Command::FocusToggle)),
            (" focus".to_string(), hint, Some(Command::FocusToggle)),
        ];
        // Fold hints are always shown; `style_footer` dims+inerts them when nothing is foldable
        // (no tree or groups active).
        row1_segments.extend([
            (" · ".to_string(), hint, None),
            ("←/".to_string(), key, Some(Command::NavLeft)),
            ("→".to_string(), key, Some(Command::NavRight)),
            (" fold".to_string(), hint, Some(Command::NavRight)),
            (" · ".to_string(), hint, None),
            // Two bracketed hotspots so each click target is unambiguous: `[-/]` collapse all,
            // `[+ all]` expand all.
            ("[".to_string(), hint, Some(Command::FoldCollapseAll)),
            ("-".to_string(), key, Some(Command::FoldCollapseAll)),
            ("/]".to_string(), hint, Some(Command::FoldCollapseAll)),
            ("[".to_string(), hint, Some(Command::FoldExpandAll)),
            ("+".to_string(), key, Some(Command::FoldExpandAll)),
            (" all]".to_string(), hint, Some(Command::FoldExpandAll)),
            (" · ".to_string(), hint, None),
            ("*".to_string(), key, Some(Command::FoldExpandSubtree)),
            (" subtree".to_string(), hint, Some(Command::FoldExpandSubtree)),
        ]);
        compose_status_row(
            style_footer(app, row1_segments, modal_open, leader_active, leader_trigger, dim_style),
            style_footer(app, right_version.clone(), modal_open, leader_active, leader_trigger, dim_style),
            area,
            area.y,
            &mut clickable,
            hint,
        )
    };

    // `style_footer(app, …)` makes the footer recede + inert under a modal or an armed leader menu
    // (row 1 shows the menu then), and dims per-command when an action would be a no-op. Called
    // directly (not via a closure) so it doesn't hold an `&app` borrow across `app.clickable.extend`.

    // Row 2 — find & layout. Each active tag sits right after its hint and is clickable:
    // `[needle]` clears the name filter, `{status}` resets to all, `⟪column ▲⟫` flips direction.
    let mut row2_segments: Vec<(String, Style, Option<Command>)> = vec![
        ("/".to_string(), key, Some(Command::NameFilter)),
        (" filter".to_string(), hint, Some(Command::NameFilter)),
    ];
    if !filter_text.is_empty() {
        row2_segments.push((" ".to_string(), hint, None));
        row2_segments.push((format!("[{filter_text}]"), active, Some(Command::ClearNameFilter)));
    }
    row2_segments.push((" · ".to_string(), hint, None));
    row2_segments.push(("f".to_string(), key, Some(Command::FilterLeader)));
    row2_segments.push((" by-status".to_string(), hint, Some(Command::FilterLeader)));
    if let Some(tag) = status_filter.tag() {
        row2_segments.push((" ".to_string(), hint, None));
        row2_segments.push((
            format!("{{{tag}}}"),
            active,
            Some(Command::SetFilter(StatusFilter::All)),
        ));
    }
    row2_segments.push((" · ".to_string(), hint, None));
    row2_segments.push(("s".to_string(), key, Some(Command::SortLeader)));
    row2_segments.push((" sort".to_string(), hint, Some(Command::SortLeader)));
    row2_segments.push((" ".to_string(), hint, None));
    row2_segments.push((
        format!("⟪{} {}⟫", sort_column.label(), sort_dir.arrow()),
        active,
        Some(Command::FlipSort),
    ));
    row2_segments.extend([
        (" · ".to_string(), hint, None),
        ("t".to_string(), key, Some(Command::ToggleLeader)),
        (" cols".to_string(), hint, Some(Command::ToggleLeader)),
    ]);
    // View toggles: `v g` grouped + `v t` tree, always shown — `style_footer` dims+inerts them when
    // not applicable (no groups.json / no nested folders). Each label brightens while its view is on.
    {
        let groups_label = if app.grouping_active() { active } else { hint };
        row2_segments.push((" · ".to_string(), hint, None));
        row2_segments.push(("vg".to_string(), key, Some(Command::GroupingToggle)));
        row2_segments.push((" groups".to_string(), groups_label, Some(Command::GroupingToggle)));
        let tree_label = if app.tree_active() { active } else { hint };
        row2_segments.push((" · ".to_string(), hint, None));
        row2_segments.push(("vt".to_string(), key, Some(Command::TreeToggle)));
        row2_segments.push((" tree".to_string(), tree_label, Some(Command::TreeToggle)));
        // Favorites-first toggle — only meaningful (and only shown) once a repo is favorited.
        if app.has_favorites() {
            let fav_label = if app.favorites_first { active } else { hint };
            row2_segments.push((" · ".to_string(), hint, None));
            row2_segments.push(("M".to_string(), key, Some(Command::FavoritesFirst)));
            row2_segments.push((" \u{2605}favs".to_string(), fav_label, Some(Command::FavoritesFirst)));
        }
    }
    row2_segments.extend([
        (" · ".to_string(), hint, None),
        // `[` narrows, `]` widens the split; the `resize` label joins the widen hotspot.
        ("[ ".to_string(), key, Some(Command::SplitNarrow)),
        ("] ".to_string(), key, Some(Command::SplitWiden)),
        ("resize".to_string(), hint, Some(Command::SplitWiden)),
        (" · ".to_string(), hint, None),
        ("b".to_string(), key, Some(Command::ToggleDock)),
        (
            if app.dock_repo_panel { " docked".to_string() } else { " dock".to_string() },
            if app.dock_repo_panel { active } else { hint },
            Some(Command::ToggleDock),
        ),
    ]);
    // When the column picker wrapped to a second row, it owns row 2 — render its second line
    // there instead of the find row (whose hidden clicks must not register).
    let row2 = if let Some(second) = toggle_lines.as_ref().filter(|lines| lines.len() > 1) {
        second[1].clone()
    } else {
        compose_status_row(
            style_footer(app, row2_segments, modal_open, leader_active, leader_trigger, dim_style),
            style_footer(app, right_built, modal_open, leader_active, leader_trigger, dim_style),
            area,
            area.y + 1,
            &mut clickable,
            hint,
        )
    };

    // Row 3 — actions. The keys + label words are all clickable; clicking "refetch"/"retry" runs
    // the all-repos (capital) variant. `style_footer` dims+inerts each command when it'd be a no-op
    // (e.g. the repo-only page/claude/lazygit/open/copy actions when no repo is selected).
    let row3_segments: Vec<(String, Style, Option<Command>)> = vec![
        ("e/".to_string(), key, Some(Command::Refetch)),
        ("E".to_string(), key, Some(Command::RefetchAll)),
        (" refetch".to_string(), hint, Some(Command::RefetchAll)),
        (" · ".to_string(), hint, None),
        ("r/".to_string(), key, Some(Command::Retry)),
        ("R".to_string(), key, Some(Command::RetryAll)),
        (" retry".to_string(), hint, Some(Command::RetryAll)),
        (" · ".to_string(), hint, None),
        ("enter".to_string(), key, Some(Command::OpenPage)),
        (" page".to_string(), hint, Some(Command::OpenPage)),
        (" · ".to_string(), hint, None),
        ("c".to_string(), key, Some(Command::Claude)),
        (" claude".to_string(), hint, Some(Command::Claude)),
        (" · ".to_string(), hint, None),
        ("l".to_string(), key, Some(Command::Lazygit)),
        (" lazygit".to_string(), hint, Some(Command::Lazygit)),
        (" · ".to_string(), hint, None),
        ("o".to_string(), key, Some(Command::OpenRemote)),
        (" open".to_string(), hint, Some(Command::OpenRemote)),
        (" · ".to_string(), hint, None),
        ("y/".to_string(), key, Some(Command::CopyPath)),
        ("Y ".to_string(), key, Some(Command::CopyRemote)),
        ("copy".to_string(), hint, Some(Command::CopyPath)),
    ];
    let row3 = compose_status_row(
        style_footer(app, row3_segments, modal_open, leader_active, leader_trigger, dim_style),
        style_footer(app, right_meta, modal_open, leader_active, leader_trigger, dim_style),
        area,
        area.y + 2,
        &mut clickable,
        hint,
    );

    app.clickable.extend(clickable);

    let text = Text::from(vec![row1, row2, row3]);
    let para = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(para, area);
}

/// The `[x]` close-button title line + its click region for a modal's top-right border corner.
/// Render with `Block::title_top`; hit-test the returned `(row, col_start, col_end)`.
fn modal_close_button(modal: Rect) -> (Line<'static>, Option<(u16, u16, u16)>) {
    let text = "[x]";
    let width = text.len() as u16;
    let line = Line::from(Span::styled(
        text,
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    ))
    .right_aligned();
    let col_end = modal.x + modal.width.saturating_sub(1);
    let col_start = col_end.saturating_sub(width);
    (line, Some((modal.y, col_start, col_end)))
}

/// A centered rect of the given size within `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

/// Render the `?` help modal: clickable links, subcommands, flags/env, grouped hotkeys,
/// exit codes, and the repo list (each row clickable to open its remote). Records the
/// screen row of every clickable line into `app.help_links` for mouse hit-testing.
/// Sentinel "url" for the collapsible Notes group header — `render_help` recognizes it and
/// records the toggle row instead of treating it as an openable link.
const TOGGLE_NOTES: &str = "\u{1f}toggle:notes";

/// The content of the help modal's "About" tab — what polygit is, plus grouped, title-only links
/// (the URL shows on hover). `notes_expanded` controls the collapsible Notes group.
fn help_items_about(notes_expanded: bool) -> Vec<(Line<'static>, Option<String>)> {
    const GITHUB_URL: &str = "https://github.com/steven-pribilinskiy/polygit";
    const LAZYGIT_URL: &str = "https://github.com/jesseduffield/lazygit";
    const NOTES_BAKEOFF: &str =
        "https://notes.lvh.me/library/default/devtools/pull-all-tui-bake-off-2026.md";
    const NOTES_FEATURES: &str =
        "https://notes.lvh.me/library/default/devtools/pull-all-tui-interaction-features-2026.md";

    let title_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let group_style = Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD);
    let link_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);

    let mut items: Vec<(Line<'static>, Option<String>)> = Vec::new();
    let plain = |text: &str| (Line::from(text.to_string()), None);
    let group = |text: &str| (Line::from(Span::styled(text.to_string(), group_style)), None);
    // A title-only link, indented under its group. The URL rides along for hover/click but is not
    // shown inline (browser-style: hover to see where it goes).
    let link = |title: &str, url: &str| {
        (
            Line::from(vec![
                Span::raw("  "),
                Span::styled(title.to_string(), link_style),
            ]),
            Some(url.to_string()),
        )
    };

    items.push((
        Line::from(Span::styled(
            "polygit — interactive polyrepo git dashboard".to_string(),
            title_style,
        )),
        None,
    ));
    items.push(plain(""));
    items.push(plain("Pull every git repo in a directory in parallel, with live per-repo logs,"));
    items.push(plain("branch / worktree / stash management, inline diffs, and a jump into lazygit."));
    items.push(plain("Built with Rust · ratatui · tokio."));
    items.push(plain(""));
    items.push(group("polygit"));
    items.push(link("Docs", DOCS_URL));
    items.push(link("GitHub", GITHUB_URL));
    items.push(plain(""));
    items.push(group("lazygit"));
    items.push(link("GitHub repo", LAZYGIT_URL));
    items.push(plain(""));
    // Collapsible Notes group — the header toggles; the entries appear only when expanded.
    let caret = if notes_expanded { "▾" } else { "▸" };
    items.push((
        Line::from(Span::styled(format!("{caret} Notes (2)"), group_style)),
        Some(TOGGLE_NOTES.to_string()),
    ));
    if notes_expanded {
        items.push(link("Three-way bake-off: Go vs Rust vs Bun", NOTES_BAKEOFF));
        items.push(link("Interaction & features", NOTES_FEATURES));
    }
    items
}

/// Sentinel carried in a Design System radio row's URL slot: `…designradio:{settings_row_idx}`.
/// `render_help` re-runs the row at the real screen position to register its click regions.
const DESIGN_RADIO_PREFIX: &str = "\u{1f}designradio:";

/// The label + options (text, is-active) for a Design System radio, by the **settings** global row
/// index it mirrors (Theme=5 · Background=6 · Contrast=7 · Selection=8). Owned/`'static` so the
/// `&AppState` read ends before the caller mutably borrows `app.help_design_click`.
fn design_radio_data(app: &AppState, row_idx: usize) -> (&'static str, Vec<(&'static str, bool)>) {
    use crate::app::{Background, Contrast, SelectionStyle, Theme};
    match row_idx {
        5 => (
            "Theme",
            vec![
                ("auto", app.theme == Theme::Auto),
                ("dark", app.theme == Theme::Dark),
                ("light", app.theme == Theme::Light),
            ],
        ),
        6 => (
            "Background",
            vec![
                ("normal", app.background == Background::Normal),
                ("soft", app.background == Background::Soft),
                ("terminal", app.background == Background::Terminal),
            ],
        ),
        7 => (
            "Contrast",
            vec![
                ("normal", app.contrast == Contrast::Normal),
                ("soft", app.contrast == Contrast::Soft),
            ],
        ),
        _ => (
            "Selection",
            vec![
                ("blue", app.selection_style == crate::app::SelectionStyle::Blue),
                ("subtle", app.selection_style == SelectionStyle::Subtle),
            ],
        ),
    }
}

/// The content of the help modal's "Design System" tab: the theming radios (Theme / Background /
/// Contrast / Selection — Icons live in the Legend tab) reusing `settings_row_line`, plus a live
/// swatch showcase of the palette's semantic colors (drawn in their semantic ANSI colors so
/// `apply_palette` themes them, updating as the radios change).
fn help_items_design_system(app: &AppState) -> Vec<(Line<'static>, Option<String>)> {
    let title_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let group_style = Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let mut items: Vec<(Line<'static>, Option<String>)> = Vec::new();
    let mut throwaway: Vec<(u16, u16, u16, usize, Option<usize>)> = Vec::new();

    items.push((Line::from(Span::styled("DESIGN SYSTEM".to_string(), title_style)), None));
    items.push((Line::from(""), None));
    items.push((Line::from(Span::styled("Theming".to_string(), group_style)), None));
    for row_idx in [5usize, 6, 7, 8] {
        let (label, options) = design_radio_data(app, row_idx);
        // The Line is position-independent; real click regions are registered by render_help at the
        // row's actual screen position via the sentinel. Discard the dummy-position clicks here.
        let underline_idx = radio_underline_idx(app, row_idx);
        let line = settings_row_line(
            row_idx, false, label, &options, (0, 0), false, underline_idx, false, None, &mut throwaway,
        );
        items.push((line, Some(format!("{DESIGN_RADIO_PREFIX}{row_idx}"))));
    }
    items.push((Line::from(""), None));
    items.push((Line::from(Span::styled("Palette — semantic colors (live)".to_string(), group_style)), None));
    let swatch = |name: &str, color: Color, purpose: &str| {
        (
            Line::from(vec![
                Span::raw("  "),
                Span::styled("███ ".to_string(), Style::default().fg(color)),
                Span::styled(
                    format!("{name:<8}"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {purpose}"), dim),
            ]),
            None,
        )
    };
    items.push(swatch("accent", Color::Cyan, "primary accent · links · active option"));
    items.push(swatch("ok", Color::Green, "success · up-to-date · pulled / added"));
    items.push(swatch("warn", Color::Yellow, "warning · running · dirty marker"));
    items.push(swatch("error", Color::Red, "failure · deleted"));
    items.push(swatch("info", Color::Magenta, "secondary accent · stashes · throttled"));
    items.push(swatch("blue", Color::Blue, "tertiary accent"));
    items.push(swatch("muted", Color::Gray, "secondary text"));
    items.push(swatch("faint", Color::DarkGray, "tertiary text · dim zero counts"));
    items.push(swatch("bright", Color::White, "strongest text"));

    // Components showcase — the reusable tui-pick primitives drawn live in every interaction state
    // (a storybook). Buttons under both hover effects, list rows under both selection effects, and
    // the radio glyph. All semantic-ANSI so `apply_palette` themes them with the live theme.
    use tui_pick::components::{
        button, list_item, radio, ButtonStyle, HoverEffect, Interaction, ListItemStyle,
        SelectionEffect,
    };
    let button_style = |hover: HoverEffect| ButtonStyle {
        label: Color::Gray,
        accent: Color::Cyan,
        on_accent: Color::Black,
        disabled: Color::DarkGray,
        hover,
        brackets: true,
    };
    let list_style = |selection: SelectionEffect| ListItemStyle {
        label: Color::Gray,
        accent: Color::Blue,
        on_accent: Color::White,
        muted_bg: Color::DarkGray,
        disabled: Color::DarkGray,
        selection,
    };
    // The user's current choices, marked so the showcase doubles as a live preview of the settings.
    let active_hover = match app.button_hover_style {
        crate::app::ButtonHoverStyle::Inverted => HoverEffect::Inverted,
        crate::app::ButtonHoverStyle::Subtle => HoverEffect::Subtle,
    };
    let active_sel = match app.selection_style {
        crate::app::SelectionStyle::Blue => SelectionEffect::Accent,
        crate::app::SelectionStyle::Subtle => SelectionEffect::Subtle,
    };
    let mark = |on: bool| if on { " ◀ active" } else { "" };

    items.push((Line::from(""), None));
    items.push((
        Line::from(Span::styled("Components — buttons (live)".to_string(), group_style)),
        None,
    ));
    items.push((
        Line::from(Span::styled(
            format!("  {:<11}{:<10}{:<10}", "state", "inverted", "subtle"),
            dim,
        )),
        None,
    ));
    for state in Interaction::ALL {
        let mut spans = vec![Span::styled(format!("  {:<11}", state.label()), dim)];
        spans.extend(button("Save", state, &button_style(HoverEffect::Inverted)).spans);
        spans.push(Span::raw("   "));
        spans.extend(button("Save", state, &button_style(HoverEffect::Subtle)).spans);
        items.push((Line::from(spans), None));
    }
    items.push((
        Line::from(Span::styled(
            format!(
                "  hover effect: inverted{} · subtle{}",
                mark(active_hover == HoverEffect::Inverted),
                mark(active_hover == HoverEffect::Subtle)
            ),
            dim,
        )),
        None,
    ));

    items.push((Line::from(""), None));
    items.push((
        Line::from(Span::styled("Components — list rows (live)".to_string(), group_style)),
        None,
    ));
    items.push((
        Line::from(Span::styled(format!("  {:<11}{:<20}{}", "state", "blue", "subtle"), dim)),
        None,
    ));
    for state in [
        Interaction::Normal,
        Interaction::Hover,
        Interaction::Selected,
        Interaction::Focused,
        Interaction::Disabled,
    ] {
        let mut spans = vec![Span::styled(format!("  {:<11}", state.label()), dim)];
        spans.extend(list_item(None, "repo-name", None, state, &list_style(SelectionEffect::Accent), 18).spans);
        spans.push(Span::raw("  "));
        spans.extend(list_item(None, "repo-name", None, state, &list_style(SelectionEffect::Subtle), 18).spans);
        items.push((Line::from(spans), None));
    }
    items.push((
        Line::from(Span::styled(
            format!(
                "  list selection: blue{} · subtle{}",
                mark(active_sel == SelectionEffect::Accent),
                mark(active_sel == SelectionEffect::Subtle)
            ),
            dim,
        )),
        None,
    ));

    items.push((Line::from(""), None));
    items.push((Line::from(Span::styled("Components — radios".to_string(), group_style)), None));
    let radio_style = button_style(active_hover);
    let mut radio_spans = vec![Span::styled("  ".to_string(), dim)];
    radio_spans.extend(radio("selected", true, &radio_style).spans);
    radio_spans.push(Span::raw("   "));
    radio_spans.extend(radio("option", false, &radio_style).spans);
    items.push((Line::from(radio_spans), None));

    items
}

/// Click sentinels carried in the CLI tab's URL slot (recognized by `render_help`).
const CLI_FLAG_PREFIX: &str = "\u{1f}cliflag:";
const CLI_COPY: &str = "\u{1f}clicopy";

/// The help modal's "CLI & Flags" tab — an interactive command builder. Each flag is a row you
/// toggle (boolean) or fill in (value); the constructed `polygit …` command + a `[Copy]` button
/// sit below the exit codes. Rows carry click sentinels so `render_help` can hit-test them.
fn help_items_cli(builder: &crate::app::CliBuilder) -> Vec<(Line<'static>, Option<String>)> {
    use crate::app::{CliFlagKind, CLI_FLAGS};
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(Color::Cyan);
    let meta_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
    let on_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
    let mut items: Vec<(Line<'static>, Option<String>)> = Vec::new();
    let header = |text: &str| (Line::from(Span::styled(text.to_string(), header_style)), None);
    let plain = |text: &str| (Line::from(text.to_string()), None);

    items.push(header("BUILD A COMMAND"));
    items.push((
        Line::from(Span::styled(
            "  ↑↓ move · space/enter toggle or edit · type a value, enter to set".to_string(),
            meta_style,
        )),
        None,
    ));
    for (idx, flag) in CLI_FLAGS.iter().enumerate() {
        let selected = idx == builder.selected;
        let cursor = if selected { "> " } else { "  " };
        let on = builder.on.get(idx).copied().unwrap_or(false);
        let value = builder.values.get(idx).cloned().unwrap_or_default();
        let editing = selected && builder.editing.is_some();
        let mut spans = vec![Span::styled(cursor.to_string(), key_style)];
        match flag.kind {
            CliFlagKind::Toggle => {
                spans.push(Span::styled(
                    format!("{} ", if on { "[x]" } else { "[ ]" }),
                    if on { on_style } else { meta_style },
                ));
                spans.push(Span::styled(format!("{:<22}", flag.flag), key_style));
            }
            CliFlagKind::Value(placeholder) | CliFlagKind::Positional(placeholder) => {
                let shown = if editing {
                    format!("{}\u{2588}", builder.editing.clone().unwrap_or_default())
                } else if value.is_empty() {
                    placeholder.to_string()
                } else {
                    value.clone()
                };
                let label = match flag.kind {
                    CliFlagKind::Positional(_) => "[DIR]".to_string(),
                    _ => flag.flag.to_string(),
                };
                spans.push(Span::raw("    "));
                spans.push(Span::styled(format!("{label} "), key_style));
                let value_style = if editing || !value.is_empty() {
                    on_style
                } else {
                    meta_style
                };
                spans.push(Span::styled(format!("{:<18}", format!("= {shown}")), value_style));
            }
        }
        spans.push(Span::styled(flag.help.to_string(), meta_style));
        items.push((Line::from(spans), Some(format!("{CLI_FLAG_PREFIX}{idx}"))));
    }
    items.push(plain(""));

    // The constructed command + a copy button.
    items.push(header("COMMAND"));
    items.push((
        Line::from(Span::styled(
            format!("  {}", builder.command()),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        None,
    ));
    items.push((
        Line::from(Span::styled(
            "  [ copy ]".to_string(),
            Style::default().fg(Color::Black).bg(Color::LightCyan).add_modifier(Modifier::BOLD),
        )),
        Some(CLI_COPY.to_string()),
    ));
    items.push(plain(""));

    items.push(header("EXIT CODES"));
    let code = |value: &str, color: Color, desc: &str| {
        (
            Line::from(vec![
                Span::styled(
                    format!("  {value:<6}"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(desc.to_string()),
            ]),
            None,
        )
    };
    items.push(code("0", Color::Green, "all ok"));
    items.push(code("1", Color::Red, "any failed"));
    items.push(code("2", Color::Yellow, "quit mid-run"));
    items.push(code("130", Color::DarkGray, "Ctrl-C"));
    items
}

/// The content of the help modal's "Legend" tab: every glyph the app draws, in both icon
/// sets side by side (Unicode · emoji — switchable in Settings), in their real colors.
fn help_items_legend() -> Vec<(Line<'static>, Option<String>)> {
    use crate::app::{EMOJI_ICONS as EMOJI, UNICODE_ICONS as UNI};
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let subhead_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let note_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
    let mut items: Vec<(Line<'static>, Option<String>)> = Vec::new();
    let header = |text: &str| (Line::from(Span::styled(text.to_string(), header_style)), None);
    let subhead = |text: &str| (Line::from(Span::styled(text.to_string(), subhead_style)), None);
    let plain = |text: &str| (Line::from(text.to_string()), None);
    // One glyph in each set, in its real on-screen color, then the meaning.
    let row = |uni: &str, emoji: &str, color: Color, meaning: &str| {
        (
            Line::from(vec![
                Span::raw("    "),
                Span::styled(pad_display(uni, 5), Style::default().fg(color)),
                Span::styled(pad_display(emoji, 7), Style::default().fg(color)),
                Span::raw(meaning.to_string()),
            ]),
            None,
        )
    };
    // Structural glyphs are the same in both sets — show them once across both columns.
    let fixed = |glyph: &str, color: Color, meaning: &str| {
        (
            Line::from(vec![
                Span::raw("    "),
                Span::styled(pad_display(glyph, 12), Style::default().fg(color)),
                Span::raw(meaning.to_string()),
            ]),
            None,
        )
    };

    items.push(header("LEGEND — every glyph, in both icon sets"));
    items.push((
        Line::from(Span::styled(
            "    left: Unicode · right: emoji — switch via Settings (,) → Icons",
            note_style,
        )),
        None,
    ));
    items.push(plain(""));
    items.push(subhead("  Status"));
    items.push(row(UNI.queued, EMOJI.queued, Color::DarkGray, "queued — waiting for a worker"));
    items.push(row(
        UNI.spinner[0],
        EMOJI.spinner[0],
        Color::Yellow,
        "running — pull in progress (spins)",
    ));
    items.push(row(UNI.up_to_date, EMOJI.up_to_date, Color::Gray, "up-to-date — nothing new"));
    items.push(row(UNI.updated, EMOJI.updated, Color::Green, "updated — pulled new commits"));
    items.push(row(
        UNI.no_upstream,
        EMOJI.no_upstream,
        Color::DarkGray,
        "no upstream — nothing to pull (not an error)",
    ));
    items.push(row(
        UNI.skipped,
        EMOJI.skipped,
        Color::DarkGray,
        "skipped — uncommitted changes in the way",
    ));
    items.push(row(
        UNI.throttled,
        EMOJI.throttled,
        Color::Magenta,
        "throttled — rate-limited; concurrency drops + auto-retry",
    ));
    items.push(row(UNI.failed, EMOJI.failed, Color::Red, "failed — pull error (see Errors)"));
    items.push(row(UNI.ok, EMOJI.ok, Color::Green, "all-ok marker on the Result row"));
    items.push(plain(""));
    items.push(subhead("  Columns & markers"));
    items.push(row(
        UNI.dirty,
        EMOJI.dirty,
        Color::Red,
        "uncommitted changes (count with the Δ column on)",
    ));
    items.push(row(UNI.ahead, EMOJI.ahead, Color::Gray, "commits ahead of upstream (↑N)"));
    items.push(row(UNI.behind, EMOJI.behind, Color::Gray, "commits behind upstream (↓N)"));
    items.push(row(UNI.worktrees, EMOJI.worktrees, Color::Cyan, "worktree count (wt column)"));
    items.push(row(
        UNI.branches,
        EMOJI.branches,
        Color::Green,
        "feature-branch count (br column; local minus main/dev)",
    ));
    items.push(row(UNI.stashes, EMOJI.stashes, Color::Magenta, "stash count (st column)"));
    items.push(plain(""));
    items.push(subhead("  Log & notices"));
    items.push(row(UNI.warning, EMOJI.warning, Color::Red, "warning (e.g. group resolve failed)"));
    items.push(row(UNI.skip_log, EMOJI.skip_log, Color::DarkGray, "skipped marker in the log"));
    items.push(row(UNI.retry_log, EMOJI.retry_log, Color::Yellow, "automatic retry marker in the log"));
    items.push(plain(""));
    items.push(subhead("  Structural (same in both sets)"));
    items.push(fixed("▾ / ▸", Color::DarkGray, "group expanded / collapsed (collapsible header)"));
    items.push(fixed("▲ / ▼", Color::Yellow, "sort direction (column header + ⟪tag⟫)"));
    items.push(fixed("● / ○", Color::Green, "active / inactive option (settings, menus)"));
    items.push(fixed("▒ → █", Color::Gray, "divider grip (fills solid while dragging)"));
    items.push(fixed("…", Color::DarkGray, "still loading"));
    items
}

/// Which underlying view the help modal is over — drives the contextual Hotkeys tab.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HelpView {
    List,
    RepoPage,
    DiffModal,
}

impl HelpView {
    fn label(self) -> &'static str {
        match self {
            HelpView::List => "repo list",
            HelpView::RepoPage => "repo page",
            HelpView::DiffModal => "diff modal",
        }
    }
}

/// The "Hotkeys" tab content for the current view — only the bindings that apply here.
/// Filter help-tab items by a search `query` (case-insensitive substring). In `hotkeys_mode` the
/// match replicates lazygit's keybinding search — the key column AND the description both match, and
/// a leading `@` restricts the match to the key column (the leading 18 display cells). Blank rows and
/// non-matching lines drop out.
fn filter_help_items(
    items: &[(Line<'static>, Option<String>)],
    query: &str,
    hotkeys_mode: bool,
) -> Vec<(Line<'static>, Option<String>)> {
    let (needle, keys_only) = match query.strip_prefix('@') {
        Some(rest) if hotkeys_mode => (rest.to_lowercase(), true),
        _ => (query.to_lowercase(), false),
    };
    items
        .iter()
        .filter(|(line, _)| {
            let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
            if text.trim().is_empty() {
                return false;
            }
            let haystack =
                if keys_only { text.chars().take(18).collect::<String>() } else { text };
            haystack.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect()
}

fn help_items_hotkeys(view: HelpView) -> Vec<(Line<'static>, Option<String>)> {
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let subhead_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(Color::Cyan);
    let mut items: Vec<(Line<'static>, Option<String>)> = Vec::new();
    let header = |text: &str| (Line::from(Span::styled(text.to_string(), header_style)), None);
    let plain = |text: &str| (Line::from(text.to_string()), None);
    // A `keys` column (padded) followed by a description — one binding per line.
    let kb = |keys: &str, desc: &str| {
        (
            Line::from(vec![
                Span::styled(format!("    {keys:<14}"), key_style),
                Span::raw(format!(" {desc}")),
            ]),
            None,
        )
    };

    match view {
        HelpView::List => {
            items.push(header("HOTKEYS — repo list"));
            // Short sections are laid out side-by-side (two whole sections per row block);
            // long-description sections (Find & sort, Groups, Pull / retry) span the width.
            type Sec<'a> = (&'a str, &'a [(&'a str, &'a str)]);
            let navigate: Sec = (
                "Navigate",
                &[
                    ("j/k  ↑/↓", "move"),
                    ("g / G", "jump to top / end"),
                    ("Home / End", "jump to top / bottom"),
                    ("PgUp / PgDn", "page up / down"),
                    ("wheel · click", "select a row"),
                ],
            );
            let views: Sec = (
                "Views & panes",
                &[
                    ("space", "Result / Errors overlay"),
                    ("tab · 1/2", "focus list ⇄ preview"),
                    ("i", "info panel"),
                    ("d", "diff view"),
                    ("End", "resume autoscroll"),
                ],
            );
            let find_sort: Sec = (
                "Find & sort",
                &[
                    ("/", "filter by name"),
                    ("f", "filter by status: a/u/c/s/f/i"),
                    ("s", "sort: n/s/a/d/l/w/b/k/o (re-pick flips ▲▼); or click a header"),
                    ("t", "toggle columns: a/d/l/w/b/s"),
                ],
            );
            let groups: Sec = (
                "Views & folding",
                &[
                    ("v g · v t", "toggle grouped view · tree view"),
                    ("Z", "refresh dynamic group memberships"),
                    ("- / + / *", "collapse all · expand all · expand subtree"),
                    ("za", "fold: toggle the folder/group"),
                    ("zo · zc", "fold: open · close"),
                    ("zO", "fold: expand subtree"),
                    ("zM · zR", "fold: collapse all · expand all"),
                    ("← / →", "collapse + jump to parent / expand"),
                    ("enter · space", "collapse/expand (on a folder/group header)"),
                ],
            );
            let pull_retry: Sec = (
                "Pull / retry",
                &[
                    ("r / R", "retry selected / all (failed or skipped)"),
                    ("e / E", "refetch selected / all (re-pull anything)"),
                ],
            );
            let clipboard: Sec = (
                "Clipboard & open",
                &[
                    ("y", "copy absolute path"),
                    ("Y", "copy remote (origin) url"),
                    ("o", "open remote in browser"),
                    ("x", "clear this repo's log buffer"),
                ],
            );
            let run: Sec = ("Run", &[("c", "claude in repo dir"), ("l", "lazygit in repo dir")]);
            let other: Sec = (
                "Other",
                &[
                    (",", "settings"),
                    ("D", "open docs site"),
                    ("?", "help"),
                    ("q", "quit"),
                    ("^C", "exit"),
                ],
            );
            let layout: Sec = (
                "Layout",
                &[("[ ]", "resize panes"), ("drag divider", "resize with the mouse")],
            );

            // A section's lines: subhead title, then one `keys  description` line per entry.
            let section_lines = |(title, entries): Sec| -> Vec<(Vec<Span<'static>>, usize)> {
                let mut out = vec![(
                    vec![Span::styled(format!("  {title}"), subhead_style)],
                    2 + UnicodeWidthStr::width(title),
                )];
                for &(keys, desc) in entries {
                    let key_text = format!("    {keys:<14}");
                    let desc_text = format!(" {desc}");
                    let width = UnicodeWidthStr::width(key_text.as_str())
                        + UnicodeWidthStr::width(desc_text.as_str());
                    out.push((
                        vec![Span::styled(key_text, key_style), Span::raw(desc_text)],
                        width,
                    ));
                }
                out
            };
            enum HelpBlock<'a> {
                Side(Sec<'a>, Sec<'a>),
                Wide(Sec<'a>),
            }
            let blocks = [
                HelpBlock::Side(navigate, views),
                HelpBlock::Wide(find_sort),
                HelpBlock::Wide(groups),
                HelpBlock::Wide(pull_retry),
                HelpBlock::Side(clipboard, run),
                HelpBlock::Side(other, layout),
            ];
            for block in blocks {
                items.push(plain(""));
                match block {
                    HelpBlock::Wide(section) => {
                        for (spans, _) in section_lines(section) {
                            items.push((Line::from(spans), None));
                        }
                    }
                    HelpBlock::Side(left, right) => {
                        let left_lines = section_lines(left);
                        let right_lines = section_lines(right);
                        let column = left_lines.iter().map(|(_, w)| *w).max().unwrap_or(0) + 4;
                        for row in 0..left_lines.len().max(right_lines.len()) {
                            let mut spans = Vec::new();
                            let mut width = 0;
                            if let Some((left_spans, left_width)) = left_lines.get(row) {
                                spans.extend(left_spans.clone());
                                width = *left_width;
                            }
                            if let Some((right_spans, _)) = right_lines.get(row) {
                                spans.push(Span::raw(" ".repeat(column - width)));
                                spans.extend(right_spans.clone());
                            }
                            items.push((Line::from(spans), None));
                        }
                    }
                }
            }
        }
        HelpView::RepoPage => {
            items.push(header("HOTKEYS — repo page"));
            items.push(kb("↑↓ · j/k", "move"));
            items.push(kb("g/G · Home/End", "jump to top / bottom"));
            items.push(kb("enter", "open diff (stash or dirty row)"));
            items.push(kb("shift+enter", "checkout (clean, non-current branch)"));
            items.push(kb("p / P", "pull branch / all branches"));
            items.push(kb("d", "delete branch · drop stash · remove worktree · discard (confirm)"));
            items.push(kb("t", "column menu — b/y/a/m/d/c/u/g/s toggle, esc closes"));
            items.push(kb("i", "toggle the info panel"));
            items.push(kb("c", "claude in the row's path"));
            items.push(kb("l", "lazygit in the row's path"));
            items.push(kb("o", "open the branch on the remote (e.g. GitHub) in your browser"));
            items.push(kb("y", "copy menu — path / branch / both"));
            items.push(kb(",", "settings"));
            items.push(kb("esc · q", "back to the repo list"));
            items.push(plain(""));
            items.push(plain("    ● marks branches/worktrees with uncommitted changes"));
        }
        HelpView::DiffModal => {
            items.push(header("HOTKEYS — diff modal"));
            items.push(kb("tab", "switch file list ⇄ diff focus"));
            items.push(kb("↑↓ · j/k", "pick a file / scroll the diff"));
            items.push(kb("g / G", "first / last file · diff top / bottom"));
            items.push(kb("PgUp/PgDn", "scroll the diff"));
            items.push(kb("⇧/⌥ PgUp/PgDn", "page the file list"));
            items.push(kb("⇧/⌥ wheel", "scroll the file list"));
            items.push(kb("f", "filter by status (>10 files)"));
            items.push(kb("t", "toggle uncommitted ⇄ base branch"));
            items.push(kb("d", "discard / remove / drop (confirm)"));
            items.push(kb("esc · q", "close"));
        }
    }
    items
}

fn render_help(frame: &mut Frame, app: &mut AppState, area: Rect) {
    // The Hotkeys tab is contextual to whatever view the help was opened over.
    let view = if app.diff_modal.is_some() {
        HelpView::DiffModal
    } else if app.repo_page.is_some() {
        HelpView::RepoPage
    } else {
        HelpView::List
    };
    // Build all tabs so the modal size stays stable when switching; show only the active one.
    let hotkeys = help_items_hotkeys(view);
    let cli = help_items_cli(&app.cli_builder);
    let legend = help_items_legend();
    let about = help_items_about(app.help_notes_expanded);
    let design = help_items_design_system(app);
    // `/` search applies to every tab: the Hotkeys tab matches like lazygit (the key column AND the
    // description; a leading `@` restricts to keys), the others are a plain text filter over the
    // content lines. Section headers/blanks drop out of the filtered view.
    let unfiltered = match app.help_tab {
        HelpTab::Hotkeys => &hotkeys,
        HelpTab::CliFlags => &cli,
        HelpTab::Legend => &legend,
        HelpTab::About => &about,
        HelpTab::DesignSystem => &design,
    };
    let filtering = app.help_filter.as_deref().is_some_and(|query| !query.is_empty());
    let filtered: Vec<(Line<'static>, Option<String>)> = if filtering {
        let query = app.help_filter.as_deref().unwrap_or_default();
        filter_help_items(unfiltered, query, app.help_tab == HelpTab::Hotkeys)
    } else {
        Vec::new()
    };
    let items = if filtering { &filtered } else { unfiltered };

    // Size the box to the widest/tallest tab (capped to the screen) so switching doesn't resize it.
    let pad = if app.panel_padding { 2 } else { 0 };
    let widest = hotkeys
        .iter()
        .chain(cli.iter())
        .chain(legend.iter())
        .chain(about.iter())
        .chain(design.iter())
        .map(|(line, _)| line.width())
        .max()
        .unwrap_or(0) as u16;
    let tallest = hotkeys
        .len()
        .max(cli.len())
        .max(legend.len())
        .max(about.len())
        .max(design.len()) as u16
        + 1; // +1 tab bar
    let max_width = area.width.saturating_sub(2);
    let max_height = area.height.saturating_sub(2);
    let (modal_width, modal_height) = if app.help_maximized {
        // ~90% of the viewport.
        (area.width.saturating_mul(9) / 10, area.height.saturating_mul(9) / 10)
    } else {
        (
            (widest + 4 + pad).min(max_width).max(40.min(max_width)),
            (tallest + 2 + pad).min(max_height).max(8.min(max_height)),
        )
    };
    let modal_area = centered_rect(modal_width, modal_height, area);
    app.help_area = modal_area;

    // Clickable hint footer on the bottom border (tab + esc inject their keys; ↑/↓ and "click a
    // link" are informational).
    let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let hint = Style::default().fg(Color::DarkGray);
    let mut footer: Vec<(String, Style, Option<HintKey>)> = Vec::new();
    footer.extend(footer_chip("tab", " switch", HintKey::Tab));
    footer.push(footer_sep());
    footer.push(("↑/↓".to_string(), key, None));
    footer.push((" scroll".to_string(), hint, None));
    footer.push(footer_sep());
    footer.push(("click a link".to_string(), hint, None));
    footer.push(footer_sep());
    footer.extend(footer_chip("?/esc", " close", HintKey::Esc));
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" polygit — help · {} ", view.label()))
        .title_bottom(modal_border_footer(footer, modal_area, &mut app.hint_click));
    // Browser-style: while hovering a link, show its URL at the bottom-right of the modal.
    if let Some(url) = app.status_hint.as_deref().filter(|_| app.help_tab == HelpTab::About) {
        block = block.title_bottom(
            Line::from(Span::styled(format!(" {url} "), Style::default().fg(Color::DarkGray)))
                .right_aligned(),
        );
    }
    // Search prompt at the bottom-right (browser-style; on Hotkeys the `@` prefix matches keys).
    if let Some(query) = app.help_filter.as_deref() {
        let hint = if app.help_tab == HelpTab::Hotkeys {
            "  (prepend @ to match keys, esc clears) "
        } else {
            "  (esc clears) "
        };
        block = block.title_bottom(
            Line::from(Span::styled(
                format!(" search: {query}\u{2588}{hint}"),
                Style::default().fg(Color::Cyan),
            ))
            .right_aligned(),
        );
    }
    let inner = block.inner(modal_area);

    // Reserve the top inner row for a fixed (non-scrolling) tab bar, then a blank row, then the
    // scrolling content beneath.
    let tab_bar_area = Rect { height: 1, ..inner };
    let content_area = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };

    // Tab bar: clickable chips on the left, a clickable [esc] close on the right. Track the
    // column of each so the mouse handler can hit-test them.
    app.help_tab_click.clear();
    let tabs = [
        ("Hotkeys", HelpTab::Hotkeys),
        ("CLI", HelpTab::CliFlags),
        ("Legend", HelpTab::Legend),
        ("About", HelpTab::About),
        ("Design", HelpTab::DesignSystem),
    ];
    let mut tab_spans: Vec<Span> = Vec::new();
    let mut tab_col = tab_bar_area.x;
    for (label, tab) in tabs {
        let chip = format!(" {label} ");
        let chip_w = UnicodeWidthStr::width(chip.as_str()) as u16;
        let style = if app.help_tab == tab {
            Style::default().fg(Color::Black).bg(Color::LightCyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        app.help_tab_click.push((tab_bar_area.y, tab_col, tab_col + chip_w, tab));
        tab_spans.push(Span::styled(chip, style));
        tab_spans.push(Span::raw(" "));
        tab_col += chip_w + 1;
    }
    // On the Hotkeys tab, offer a clickable button that pops the interactive keyboard viewer.
    app.help_keyboard_click = None;
    let kbd_btn = if app.help_tab == HelpTab::Hotkeys { "[K ⌨ keyboard]" } else { "" };
    let kbd_w = UnicodeWidthStr::width(kbd_btn) as u16;
    // Right-aligned buttons, laid out right→left: [esc], then maximize/restore, then (Hotkeys
    // only) the keyboard viewer.
    let esc = "[esc]";
    let esc_w = esc.len() as u16;
    let max_btn = if app.help_maximized { "[m restore]" } else { "[m maximize]" };
    let max_w = max_btn.len() as u16;
    let esc_col = tab_bar_area.x + tab_bar_area.width.saturating_sub(esc_w);
    let max_col = esc_col.saturating_sub(max_w + 1);
    let kbd_col = max_col.saturating_sub(if kbd_w > 0 { kbd_w + 1 } else { 0 });
    if kbd_col > tab_col {
        tab_spans.push(Span::raw(" ".repeat((kbd_col - tab_col) as usize)));
    }
    if kbd_w > 0 {
        app.help_keyboard_click = Some((tab_bar_area.y, kbd_col, kbd_col + kbd_w));
        tab_spans
            .push(Span::styled(kbd_btn.to_string(), Style::default().fg(Color::LightMagenta)));
        tab_spans.push(Span::raw(" "));
    }
    app.help_maximize_click = Some((tab_bar_area.y, max_col, max_col + max_w));
    tab_spans.push(Span::styled(max_btn.to_string(), Style::default().fg(Color::DarkGray)));
    tab_spans.push(Span::raw(" "));
    app.help_close_click = Some((tab_bar_area.y, esc_col, esc_col + esc_w));
    tab_spans.push(Span::styled(esc.to_string(), Style::default().fg(Color::DarkGray)));
    let tab_bar = Line::from(tab_spans);

    // Clamp scroll to the active tab's content, then window the visible slice.
    let content_height = content_area.height as usize;
    let max_scroll = items.len().saturating_sub(content_height);
    if app.help_scroll > max_scroll {
        app.help_scroll = max_scroll;
    }
    let start = app.help_scroll;
    let end = (start + content_height).min(items.len());

    app.help_links.clear();
    app.help_notes_toggle_row = None;
    app.cli_flag_click.clear();
    app.cli_copy_click = None;
    app.help_design_click.clear();
    let mut lines: Vec<Line> = Vec::with_capacity(end.saturating_sub(start));
    for (offset, (line, url)) in items[start..end].iter().enumerate() {
        let row = content_area.y + offset as u16;
        match url.as_deref() {
            Some(sentinel) if sentinel.starts_with(DESIGN_RADIO_PREFIX) => {
                // Re-run the radio at its real screen row to capture the chip click regions (the
                // pre-built Line is position-independent; only the click columns need the row).
                if let Ok(row_idx) = sentinel[DESIGN_RADIO_PREFIX.len()..].parse::<usize>() {
                    let (label, options) = design_radio_data(app, row_idx);
                    let underline_idx = radio_underline_idx(app, row_idx);
                    let _ = settings_row_line(
                        row_idx,
                        false,
                        label,
                        &options,
                        (content_area.x, row),
                        true,
                        underline_idx,
                        false,
                        None,
                        &mut app.help_design_click,
                    );
                }
            }
            Some(TOGGLE_NOTES) => app.help_notes_toggle_row = Some(row),
            Some(CLI_COPY) => {
                app.cli_copy_click = Some((row, content_area.x, content_area.x + content_area.width));
            }
            Some(sentinel) if sentinel.starts_with(CLI_FLAG_PREFIX) => {
                if let Ok(idx) = sentinel[CLI_FLAG_PREFIX.len()..].parse::<usize>() {
                    app.cli_flag_click.push((row, idx));
                }
            }
            Some(url) => app.help_links.push((row, url.to_string())),
            None => {}
        }
        lines.push(line.clone());
    }

    cast_shadow(frame, modal_area);
    frame.render_widget(Clear, modal_area);
    frame.render_widget(block, modal_area);
    frame.render_widget(Paragraph::new(tab_bar), tab_bar_area);
    frame.render_widget(Paragraph::new(lines), content_area);
    let track = scrollbar_track(modal_area, content_area);
    render_scrollbar(
        frame,
        track,
        app.help_scroll,
        items.len(),
        content_height,
        app.scrollbar_dragging == Some(ScrollKind::Help),
    );
    app.scroll_hits.push(ScrollHit {
        kind: ScrollKind::Help,
        track,
        total: items.len(),
        viewport: content_height,
    });
}

/// Pad `label` with spaces so it occupies exactly `width` display cells, centered.
fn center_cell(label: &str, width: u16) -> String {
    let used = UnicodeWidthStr::width(label) as u16;
    if used >= width {
        return label.to_string();
    }
    let total = (width - used) as usize;
    let left = total / 2;
    format!("{}{}{}", " ".repeat(left), label, " ".repeat(total - left))
}

/// The interactive keyboard viewer: an on-screen keyboard (same data as the docs viewer) where
/// bound keys are highlighted; pressing or clicking a key fills a scrollable panel below with
/// every action that key drives. Esc closes it. Mirrors `KeyboardModal.astro`.
fn render_keyboard_modal(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let modal_width = (area.width * 9 / 10).max(20);
    let modal_height = (area.height * 9 / 10).max(8);
    let modal_area = centered_rect(modal_width, modal_height, area);
    app.keyboard_area = modal_area;

    let (close_line, close_click) = modal_close_button(modal_area);
    app.keyboard_close_click = close_click;
    // Clickable bottom-border footer — only `esc` injects a key; the rest is informational.
    let hint = Style::default().fg(Color::DarkGray);
    let mut footer: Vec<(String, Style, Option<HintKey>)> = vec![
        ("press / click any key".to_string(), hint, None),
        footer_sep(),
        ("highlighted keys are bound".to_string(), hint, None),
        footer_sep(),
    ];
    footer.extend(footer_chip("esc", " close", HintKey::Esc));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(" keyboard — press any key to inspect it ")
        .title_bottom(modal_border_footer(footer, modal_area, &mut app.hint_click));
    let inner = block.inner(modal_area);

    cast_shadow(frame, modal_area);
    frame.render_widget(Clear, modal_area);
    frame.render_widget(&block, modal_area);
    frame.render_widget(Paragraph::new(close_line), Rect { height: 1, ..modal_area });

    let uses = crate::keymap::key_uses();
    let selected = app.keyboard_selected;

    // Build the board: the main block plus the nav/arrow cluster. The cluster sits to the right
    // (bottom-aligned, mirroring the docs viewer) when there's width for it, else below as a
    // fallback. When the modal is large the keys grow into tall bordered boxes ("full-blown");
    // otherwise they stay the compact single-row strip.
    app.keyboard_key_click.clear();
    let mut clicks: Vec<(u16, u16, u16, &'static str)> = Vec::new();
    let os = crate::keymap::Os::current();
    let main_rows = crate::keymap::layout(os);
    let cluster_rows = crate::keymap::cluster();

    const GAP: u16 = 3; // columns between the main block and the right-hand cluster
    const FULL_ROWS: u16 = 3; // screen lines per key in the full (boxed) tier
    const FULL_MIN_PANEL: u16 = 8; // lines reserved for the action panel before going full-size

    let cell_width = |width: u16, scale: u16, boxed: bool| width * scale + if boxed { 2 } else { 0 };
    let row_width = |row: &[crate::keymap::KeyDef], scale: u16, boxed: bool| -> u16 {
        row.iter().map(|key| cell_width(key.width, scale, boxed)).sum::<u16>()
            + row.len().saturating_sub(1) as u16
    };
    let board_width_at =
        |scale: u16, boxed: bool| main_rows.iter().map(|row| row_width(row, scale, boxed)).max().unwrap_or(0);
    let cluster_width_at =
        |scale: u16, boxed: bool| cluster_rows.iter().map(|row| row_width(row, scale, boxed)).max().unwrap_or(0);
    let combined_at =
        |scale: u16, boxed: bool| board_width_at(scale, boxed) + GAP + cluster_width_at(scale, boxed);

    // Largest full-tier (boxed) scale that fits side-by-side, given enough height; else compact.
    let full_scale = if inner.height >= FULL_ROWS * main_rows.len() as u16 + FULL_MIN_PANEL {
        (1..=3u16).rev().find(|&scale| combined_at(scale, true) <= inner.width)
    } else {
        None
    };
    let (boxed, scale, cluster_right) = match full_scale {
        Some(scale) => (true, scale, true),
        None => (false, 1, combined_at(1, false) <= inner.width),
    };
    let lines_per_key = if boxed { FULL_ROWS as usize } else { 1 };

    let board_width = board_width_at(scale, boxed);
    let cluster_width = cluster_width_at(scale, boxed);
    let combined_width = if cluster_right {
        board_width + GAP + cluster_width
    } else {
        board_width.max(cluster_width)
    };
    let left_pad = inner.width.saturating_sub(combined_width) / 2;
    let board_x = inner.x + left_pad;

    let sel_style = Style::default().fg(Color::Black).bg(Color::LightMagenta).add_modifier(Modifier::BOLD);
    let bound_style = Style::default().fg(Color::Black).bg(Color::LightCyan).add_modifier(Modifier::BOLD);
    let unbound_style = Style::default().fg(Color::DarkGray).bg(Color::Rgb(40, 42, 54));

    // Render one keyboard row's `sub`-th screen line starting at absolute column `start_col`,
    // appending spans and a click region per key cell. Returns the column it ended at.
    let place_row = |row: &[crate::keymap::KeyDef],
                     sub: usize,
                     start_col: u16,
                     screen_row: u16,
                     spans: &mut Vec<Span<'static>>,
                     clicks: &mut Vec<(u16, u16, u16, &'static str)>|
     -> u16 {
        let mut col = start_col;
        for (index, key) in row.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw(" "));
                col += 1;
            }
            let interior = key.width * scale;
            let outer = interior + if boxed { 2 } else { 0 };
            if key.code == "__gap" {
                spans.push(Span::raw(" ".repeat(outer as usize)));
                col += outer;
                continue;
            }
            let style = if selected == Some(key.code) {
                sel_style
            } else if uses.contains_key(key.code) {
                bound_style
            } else {
                unbound_style
            };
            let text = if boxed {
                match sub {
                    0 => format!("╭{}╮", "─".repeat(interior as usize)),
                    2 => format!("╰{}╯", "─".repeat(interior as usize)),
                    _ => format!("│{}│", center_cell(key.label, interior)),
                }
            } else {
                center_cell(key.label, interior)
            };
            spans.push(Span::styled(text, style));
            clicks.push((screen_row, col, col + outer, key.code));
            col += outer;
        }
        col
    };

    let mut kb_lines: Vec<Line> = Vec::new();
    if cluster_right {
        // The cluster's 4 rows sit beside the bottom 4 of the 5 main rows (bottom-aligned).
        let cluster_offset = main_rows.len().saturating_sub(cluster_rows.len());
        let cluster_x = board_x + board_width + GAP;
        for (main_index, main_row) in main_rows.iter().enumerate() {
            for sub in 0..lines_per_key {
                let screen_row = inner.y + kb_lines.len() as u16;
                let mut spans: Vec<Span<'static>> = vec![Span::raw(" ".repeat(left_pad as usize))];
                let end_col = place_row(main_row, sub, board_x, screen_row, &mut spans, &mut clicks);
                if main_index >= cluster_offset {
                    if cluster_x > end_col {
                        spans.push(Span::raw(" ".repeat((cluster_x - end_col) as usize)));
                    }
                    place_row(&cluster_rows[main_index - cluster_offset], sub, cluster_x, screen_row, &mut spans, &mut clicks);
                }
                kb_lines.push(Line::from(spans));
            }
        }
    } else {
        for main_row in &main_rows {
            for sub in 0..lines_per_key {
                let screen_row = inner.y + kb_lines.len() as u16;
                let mut spans: Vec<Span<'static>> = vec![Span::raw(" ".repeat(left_pad as usize))];
                place_row(main_row, sub, board_x, screen_row, &mut spans, &mut clicks);
                kb_lines.push(Line::from(spans));
            }
        }
        kb_lines.push(Line::from("")); // visual gap before the stacked cluster
        for cluster_row in &cluster_rows {
            for sub in 0..lines_per_key {
                let screen_row = inner.y + kb_lines.len() as u16;
                let mut spans: Vec<Span<'static>> = vec![Span::raw(" ".repeat(left_pad as usize))];
                place_row(cluster_row, sub, board_x, screen_row, &mut spans, &mut clicks);
                kb_lines.push(Line::from(spans));
            }
        }
    }
    app.keyboard_key_click = clicks;

    let kb_height = kb_lines.len() as u16;
    let kb_area = Rect { height: kb_height.min(inner.height), ..inner };
    frame.render_widget(Paragraph::new(kb_lines), kb_area);

    // Divider + the actions panel below the board.
    let divider_y = inner.y + kb_height + 1;
    if divider_y >= inner.y + inner.height {
        app.keyboard_panel_area = Rect { height: 0, ..inner };
        return;
    }
    let panel_area = Rect {
        y: divider_y,
        height: inner.height.saturating_sub(kb_height + 1),
        ..inner
    };
    app.keyboard_panel_area = panel_area;

    // Header line for the panel.
    let header = match selected {
        Some(code) => {
            let label = crate::keymap::layout(crate::keymap::Os::current())
                .iter()
                .chain(crate::keymap::cluster().iter())
                .flatten()
                .find(|key| key.code == code)
                .map(|key| key.label)
                .unwrap_or(code);
            format!(" {} ", label.trim())
        }
        None => String::new(),
    };

    let mut panel_lines: Vec<Line> = Vec::new();
    match selected {
        None => {
            panel_lines.push(Line::from(Span::styled(
                "Press any key (or click one above) to see what it does.",
                Style::default().fg(Color::Gray),
            )));
            panel_lines.push(Line::from(""));
            panel_lines.push(Line::from(Span::styled(
                "Esc closes this viewer.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        Some(code) => match uses.get(code) {
            None => {
                panel_lines.push(Line::from(vec![
                    Span::styled(header.clone(), Style::default().fg(Color::Black).bg(Color::LightMagenta).add_modifier(Modifier::BOLD)),
                    Span::raw("  "),
                    Span::styled("no polygit action bound to this key", Style::default().fg(Color::DarkGray)),
                ]));
            }
            Some(list) => {
                panel_lines.push(Line::from(vec![
                    Span::styled(header.clone(), Style::default().fg(Color::Black).bg(Color::LightMagenta).add_modifier(Modifier::BOLD)),
                    Span::raw("  "),
                    Span::styled(format!("{} action{}", list.len(), if list.len() == 1 { "" } else { "s" }), Style::default().fg(Color::Gray)),
                ]));
                panel_lines.push(Line::from(""));
                // Size the keys column to the longest combo *currently shown*, so it tightens for
                // keys with only short combos and stays aligned when one combo is long.
                let combo_width = list
                    .iter()
                    .map(|use_| UnicodeWidthStr::width(use_.combo.as_str()))
                    .max()
                    .unwrap_or(0);
                for use_ in list {
                    panel_lines.push(Line::from(vec![
                        Span::styled(pad_display(&use_.combo, combo_width), Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                        Span::raw("  "),
                        Span::styled(use_.action.clone(), Style::default().fg(Color::White)),
                        Span::raw("  "),
                        Span::styled(format!("· {}", use_.section), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        },
    }

    // Clamp + window the panel scroll.
    let panel_height = panel_area.height as usize;
    let max_scroll = panel_lines.len().saturating_sub(panel_height);
    if app.keyboard_scroll > max_scroll {
        app.keyboard_scroll = max_scroll;
    }
    let start = app.keyboard_scroll;
    let end = (start + panel_height).min(panel_lines.len());
    let windowed: Vec<Line> = panel_lines[start..end].to_vec();
    frame.render_widget(Paragraph::new(windowed), panel_area);

    let track = scrollbar_track(modal_area, panel_area);
    render_scrollbar(
        frame,
        track,
        app.keyboard_scroll,
        panel_lines.len(),
        panel_height,
        false,
    );
    app.scroll_hits.push(ScrollHit {
        kind: ScrollKind::Keyboard,
        track,
        total: panel_lines.len(),
        viewport: panel_height,
    });
}

/// The accent color for a file's git status char in the diff-modal file list.
fn diff_status_color(status: &str) -> Color {
    match status {
        "A" | "?" => Color::Green,
        "D" => Color::Red,
        "R" | "C" => Color::Cyan,
        _ => Color::Yellow,
    }
}

/// Render the 90%-of-screen diff modal: a scrollable file-list panel (top, ≤40% height) over
/// the selected file's diff (bottom). Clicking or `j`/`k` selects a file.
/// The diff-modal footer hint line, dependent on the focused pane (file list vs diff) and the
/// source's available verbs. Shows `f filter` only when the status chips are active.
fn diff_modal_footer(
    source: &DiffSource,
    focus: DiffFocus,
    chips: bool,
) -> Vec<(String, Style, Option<HintKey>)> {
    let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let hint = Style::default().fg(Color::DarkGray);
    let mut seg: Vec<(String, Style, Option<HintKey>)> = Vec::new();
    let sep = (" · ".to_string(), hint, None);
    // Navigation hints aren't single keys, so they're shown but not clickable.
    match focus {
        DiffFocus::Files => seg.extend([
            ("j/k".to_string(), key, None),
            (" pick · ".to_string(), hint, None),
            ("⇧PgUp/PgDn".to_string(), key, None),
            (" page · ".to_string(), hint, None),
            ("⌥/⇧wheel".to_string(), key, None),
            (" scroll".to_string(), hint, None),
            sep.clone(),
            ("tab".to_string(), key, Some(HintKey::Tab)),
            (" → diff".to_string(), hint, Some(HintKey::Tab)),
        ]),
        DiffFocus::Diff => seg.extend([
            ("j/k".to_string(), key, None),
            (" scroll · ".to_string(), hint, None),
            ("PgUp/PgDn".to_string(), key, None),
            (" page · ".to_string(), hint, None),
            ("g/G".to_string(), key, None),
            (" top/end".to_string(), hint, None),
            sep.clone(),
            ("tab".to_string(), key, Some(HintKey::Tab)),
            (" → files".to_string(), hint, Some(HintKey::Tab)),
        ]),
    }
    if chips {
        seg.push(sep.clone());
        seg.push(("f".to_string(), key, Some(HintKey::Char('f'))));
        seg.push((" filter".to_string(), hint, Some(HintKey::Char('f'))));
    }
    if matches!(source, DiffSource::Dirty { .. }) {
        seg.push(sep.clone());
        seg.push(("t".to_string(), key, Some(HintKey::Char('t'))));
        seg.push((" toggle".to_string(), hint, Some(HintKey::Char('t'))));
    }
    let delete_label = match source {
        DiffSource::Stash { .. } => Some(" drop"),
        DiffSource::Dirty { .. } => Some(" discard/remove"),
        DiffSource::Branch { .. } => None,
    };
    if let Some(label) = delete_label {
        seg.push(sep.clone());
        seg.push(("d".to_string(), key, Some(HintKey::Char('d'))));
        seg.push((label.to_string(), hint, Some(HintKey::Char('d'))));
    }
    seg.push(sep);
    seg.push(("esc".to_string(), key, Some(HintKey::Esc)));
    seg
}

fn render_diff_modal(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let modal_width = (area.width * 9 / 10).max(20);
    let modal_height = (area.height * 9 / 10).max(8);
    let modal_area = centered_rect(modal_width, modal_height, area);

    // Owned snapshot so the immutable borrow ends before we write scroll/areas back.
    let (
        title,
        footer,
        files,
        selected,
        diff_lines,
        diff_scroll_req,
        file_scroll_in,
        focus,
        visible,
        chips,
        chips_active,
        status_filter,
    ) = {
        let Some(modal) = app.diff_modal.as_ref() else {
            return;
        };
        let title = match &modal.source {
            DiffSource::Stash { index, label, .. } => {
                format!(" stash@{{{index}}} · {} ", truncate_str(label, 50))
            }
            DiffSource::Dirty { name, .. } => {
                let mode = match modal.mode {
                    DiffMode::Uncommitted => "uncommitted",
                    DiffMode::BaseBranch => "vs base branch",
                };
                format!(" {name} · {mode} ")
            }
            DiffSource::Branch { name, .. } => format!(" {name} · vs base branch "),
        };
        let footer = diff_modal_footer(&modal.source, modal.focus, modal.chips_active());
        (
            title,
            footer,
            modal.files.clone(),
            modal.selected,
            modal.lines.clone(),
            modal.scroll,
            modal.file_scroll,
            modal.focus,
            modal.visible_file_indices(),
            modal.status_chips(),
            modal.chips_active(),
            modal.status_filter,
        )
    };

    let (close_line, close_click) = modal_close_button(modal_area);
    // Styled, clickable footer on the bottom border (left-aligned so its click columns line up).
    let footer_row = modal_area.y + modal_area.height.saturating_sub(1);
    let footer_line = build_hint_footer(footer, modal_area.x + 1, footer_row, &mut app.hint_click);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(title)
        .title_top(close_line)
        .title_bottom(footer_line);
    let inner = block.inner(modal_area);
    cast_shadow(frame, modal_area);
    frame.render_widget(Clear, modal_area);
    frame.render_widget(block, modal_area);
    app.diff_modal_area = modal_area;
    app.diff_modal_close_click = close_click;

    // Two bordered sub-panels floating inside the modal: a file-list panel (≤40% height) over the
    // diff panel. Inset from the modal border with a 1-row gap between them so their borders and
    // scrollbars don't collide with the modal border. The focused panel (Tab) gets a bright border.
    let panels = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    let panel_chrome = if app.panel_padding { 4 } else { 2 };
    let max_file_box = (panels.height * 4 / 10).max(3);
    // Reserve a row for the status-chip line when it's shown.
    let chip_rows = u16::from(chips_active);
    let wanted_file_box = visible.len() as u16 + panel_chrome + chip_rows;
    let file_box_height = wanted_file_box.clamp(3 + chip_rows, max_file_box);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(file_box_height),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .split(panels);
    let file_box = chunks[0];
    let diff_box = chunks[2];
    let focus_color = |active: bool| if active { Color::Cyan } else { Color::DarkGray };

    // ---- File-list panel ----
    let file_title = if status_filter.is_some() {
        format!(" files ({}/{}) ", visible.len(), files.len())
    } else {
        format!(" files ({}) ", files.len())
    };
    let file_panel = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(focus_color(focus == DiffFocus::Files)))
        .title(file_title);
    let file_inner = file_panel.inner(file_box);
    frame.render_widget(file_panel, file_box);

    // The chip row (when active) takes the panel's first inner row; the file list fills the rest.
    app.diff_chips_click.clear();
    let list_inner = if chips_active {
        let chip_area = Rect { height: 1, ..file_inner };
        let mut chip_specs: Vec<(String, Option<char>, Color, bool)> =
            vec![(format!(" all {} ", files.len()), None, Color::Gray, status_filter.is_none())];
        for (bucket, count) in &chips {
            chip_specs.push((
                format!(" {bucket} {count} "),
                Some(*bucket),
                diff_status_color(&bucket.to_string()),
                status_filter == Some(*bucket),
            ));
        }
        let mut spans: Vec<Span> = Vec::new();
        let mut col = chip_area.x;
        for (label, bucket, fg, active) in chip_specs {
            let chip = format!("[{label}]");
            let chip_width = UnicodeWidthStr::width(chip.as_str()) as u16;
            let style = if active {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(fg)
            };
            app.diff_chips_click.push((chip_area.y, col, col + chip_width, bucket));
            spans.push(Span::styled(chip, style));
            spans.push(Span::raw(" "));
            col += chip_width + 1;
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), chip_area);
        Rect { y: file_inner.y + 1, height: file_inner.height.saturating_sub(1), ..file_inner }
    } else {
        file_inner
    };
    // Reserve the inner's right column for the scrollbar so the rounded border corners stay intact.
    let file_content = Rect { width: list_inner.width.saturating_sub(1), ..list_inner };

    let view_rows = file_content.height as usize;
    // File-list scroll is independent of the selection — just clamp it to the valid range.
    let file_scroll = file_scroll_in.min(visible.len().saturating_sub(view_rows));

    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "(no changed files)",
                Style::default().fg(Color::DarkGray),
            ))),
            file_content,
        );
    } else {
        let path_width = file_content.width.saturating_sub(5) as usize;
        let sel_style = selection_highlight_style(app);
        let rows: Vec<Line> = visible
            .iter()
            .skip(file_scroll)
            .take(view_rows)
            .map(|&abs| {
                let file = &files[abs];
                let status = Span::styled(
                    format!(" {} ", file.status),
                    Style::default().fg(diff_status_color(&file.status)),
                );
                let path = Span::raw(truncate_str(&file.path, path_width.max(1)));
                let line = Line::from(vec![status, path]);
                if abs == selected {
                    line.style(sel_style)
                } else {
                    line
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(rows), file_content);
        // Scrollbar inside the panel (on the inner's right column), not on the border.
        render_scrollbar(
            frame,
            list_inner,
            file_scroll,
            visible.len(),
            view_rows,
            app.scrollbar_dragging == Some(ScrollKind::DiffFiles),
        );
        app.scroll_hits.push(ScrollHit {
            kind: ScrollKind::DiffFiles,
            track: list_inner,
            total: visible.len(),
            viewport: view_rows,
        });
    }

    // ---- Diff panel ----
    let diff_title = if visible.is_empty() {
        " diff ".to_string()
    } else {
        let position = visible.iter().position(|&index| index == selected).unwrap_or(0);
        let prefix = format!(" file {}/{} — ", position + 1, visible.len());
        // Truncate the path only when it doesn't fit the title line (corners + prefix + a space).
        let budget = (diff_box.width as usize)
            .saturating_sub(2 + UnicodeWidthStr::width(prefix.as_str()) + 1)
            .max(8);
        format!("{prefix}{} ", truncate_left(&files[selected].path, budget))
    };
    let diff_panel = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(focus_color(focus == DiffFocus::Diff)))
        .title(diff_title);
    let diff_inner = diff_panel.inner(diff_box);
    frame.render_widget(diff_panel, diff_box);
    // Reserve the inner's right column for the scrollbar (keeps the rounded border corners).
    let diff_content = Rect { width: diff_inner.width.saturating_sub(1), ..diff_inner };

    let diff_view_h = diff_content.height as usize;
    let diff_total = diff_lines.len();
    let diff_scroll = diff_scroll_req.min(diff_total.saturating_sub(diff_view_h));
    let diff_view: Vec<Line> = diff_lines[diff_scroll..(diff_scroll + diff_view_h).min(diff_total)]
        .iter()
        .map(|line| ansi_line_to_ratatui(line))
        .collect();
    frame.render_widget(Paragraph::new(diff_view), diff_content);
    render_scrollbar(
        frame,
        diff_inner,
        diff_scroll,
        diff_total,
        diff_view_h,
        app.scrollbar_dragging == Some(ScrollKind::DiffBody),
    );
    app.scroll_hits.push(ScrollHit {
        kind: ScrollKind::DiffBody,
        track: diff_inner,
        total: diff_total,
        viewport: diff_view_h,
    });

    if let Some(modal) = app.diff_modal.as_mut() {
        modal.scroll = diff_scroll;
        modal.file_scroll = file_scroll;
    }
    app.diff_modal_viewport = diff_view_h;
    app.diff_files_viewport = view_rows;
    app.diff_files_area = file_content;
    app.diff_body_area = diff_content;
}

/// Fixed-width ahead/behind spans (`↑a ↓b`), each arrow colored by its own count: a zero
/// count is dim gray, a positive ahead is yellow, a positive behind is cyan. No upstream
/// renders a dim `—`. Padded with trailing spaces to `width` (counted in chars).
fn ahead_behind_spans(
    ahead: Option<u32>,
    behind: Option<u32>,
    width: usize,
    icons: &IconSet,
) -> Vec<Span<'static>> {
    let gray = Style::default().fg(Color::DarkGray);
    match (ahead, behind) {
        (Some(ahead), Some(behind)) => {
            let up = format!("{}{ahead}", icons.ahead);
            let down = format!("{}{behind}", icons.behind);
            // Pad by display width so double-width emoji arrows don't desync the column.
            let used = UnicodeWidthStr::width(up.as_str()) + 1 + UnicodeWidthStr::width(down.as_str());
            let pad = width.saturating_sub(used);
            let up_style = if ahead > 0 {
                Style::default().fg(Color::Yellow)
            } else {
                gray
            };
            let down_style = if behind > 0 {
                Style::default().fg(Color::Cyan)
            } else {
                gray
            };
            vec![
                Span::styled(up, up_style),
                Span::raw(" "),
                Span::styled(down, down_style),
                Span::raw(" ".repeat(pad)),
            ]
        }
        _ => vec![Span::styled(format!("{:<width$}", "no-up"), gray)],
    }
}

/// Build the repo-page info panel lines for the selected row: branch/upstream/base, ahead-behind,
/// change stats, last commit, and worktree/stash specifics. Pure (returns owned lines).
fn build_repo_page_info_lines(
    row: &PageRow,
    base_branch: Option<&str>,
    pr: Option<&crate::app::PrInfo>,
) -> Vec<Line<'static>> {
    let key = Style::default().fg(Color::DarkGray);
    let val = Style::default().fg(Color::Gray);
    let pair = |label: &str, value: String| {
        Line::from(vec![
            Span::styled(format!("{label:<13}"), key),
            Span::styled(value, val),
        ])
    };
    let mut lines: Vec<Line> = Vec::new();
    match row.kind {
        PageRowKind::Stash => {
            let stash_ref = format!("stash@{{{}}}", row.stash_index.unwrap_or(0));
            lines.push(pair("stash", stash_ref));
            lines.push(pair("label", row.branch.clone()));
        }
        PageRowKind::Branch | PageRowKind::Worktree => {
            let head = if row.is_head { "  (HEAD)" } else { "" };
            lines.push(pair("branch", format!("{}{head}", row.branch)));
            lines.push(pair("upstream", row.upstream.clone().unwrap_or_else(|| "(none)".to_string())));
            // The open PR (resolved for the repo's current branch) shows on the HEAD row only.
            if row.is_head {
                if let Some(pr) = pr {
                    lines.push(pair("pull request", format!("#{} {}", pr.number, pr.title)));
                }
            }
            let base = match (base_branch, row.merge_base_short.as_deref()) {
                (Some(base), Some(point)) => format!("{base} @ {point}"),
                (Some(base), None) => base.to_string(),
                _ => "(unknown)".to_string(),
            };
            lines.push(pair("base", base));
            if let (Some(ahead), Some(behind)) = (row.ahead, row.behind) {
                lines.push(pair("ahead/behind", format!("↑{ahead} ↓{behind}")));
            }
            let changes = match row.stats {
                Some(stats) => format!(
                    "+{} ~{} -{}  (Σ {})",
                    stats.added, stats.modified, stats.deleted, stats.total()
                ),
                None => "computing…".to_string(),
            };
            lines.push(pair("changes", changes));
            if !row.commit_sha.is_empty() || !row.author.is_empty() {
                let mut commit = Vec::new();
                if !row.commit_sha.is_empty() {
                    commit.push(row.commit_sha.clone());
                }
                if !row.author.is_empty() {
                    commit.push(row.author.clone());
                }
                if !row.last_commit_rel.is_empty() {
                    commit.push(row.last_commit_rel.clone());
                }
                lines.push(pair("commit", commit.join(" · ")));
            }
            if !row.subject.is_empty() {
                lines.push(pair("subject", truncate_str(&row.subject, 60)));
            }
            if row.kind == PageRowKind::Worktree {
                lines.push(pair("path", row.path.display().to_string()));
            }
            if row.dirty_count > 0 {
                lines.push(pair("uncommitted", format!("{} file(s)", row.dirty_count)));
            }
        }
    }
    lines
}

/// Render the full-screen dedicated repo page: branches + worktrees + fresh ahead/behind.
fn render_repo_page(frame: &mut Frame, app: &mut AppState, area: Rect, tick: u64) {
    let tabbed = app.repo_page_tabbed();
    let active_tab = app.repo_page_tab;
    let (full_branches, full_worktrees, full_stashes, full_commits) =
        app.repo_page_section_counts();
    let rows = app.repo_page_rows();
    let Some(idx) = app.repo_page else {
        return;
    };
    let selected = app.repo_page_selected.min(rows.len().saturating_sub(1));

    let (name, path, loading, fetched, fetch_error, pulling) = {
        let state = app.repos[idx].lock().unwrap();
        let (fetched, fetch_error) = match &state.page {
            Some(page) => (page.fetched, page.fetch_error.clone()),
            None => (false, None),
        };
        (
            state.name.clone(),
            state.path.display().to_string(),
            state.page_loading,
            fetched,
            fetch_error,
            state.pull_loading,
        )
    };
    let head_branch = rows
        .iter()
        .find(|row| row.is_head)
        .map(|row| row.branch.clone())
        .unwrap_or_else(|| "—".to_string());

    // Animated spinner in the title while a pull runs or the page (re)fetches branches.
    let icons = app.icons();
    let mut title = format!(" {name} · {head_branch} · {path} ");
    if pulling {
        title.push_str(&format!("· {} pulling… ", spinner_frame(tick, icons)));
    } else if loading || !fetched {
        title.push_str(&format!("· {} fetching… ", spinner_frame(tick, icons)));
    }
    // A styled, clickable footer matching the root status bar: bold accent keys, dim labels,
    // each key/label clickable (it injects the same key). The `d` verb is dynamic to the selected
    // row, and `?` opens the full keys. `↑↓ move` is a hint only — rows are selected by clicking.
    let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let hint = Style::default().fg(Color::DarkGray);
    let sep = || (" · ".to_string(), hint, None);
    let mut footer_segments: Vec<(String, Style, Option<HintKey>)> = vec![
        ("↑↓".to_string(), key, None),
        (" move".to_string(), hint, None),
        sep(),
        ("enter".to_string(), key, Some(HintKey::Enter)),
        (" diff".to_string(), hint, Some(HintKey::Enter)),
        sep(),
        ("⇧enter".to_string(), key, Some(HintKey::ShiftEnter)),
        (" checkout".to_string(), hint, Some(HintKey::ShiftEnter)),
        sep(),
        ("p".to_string(), key, Some(HintKey::Char('p'))),
        (" pull".to_string(), hint, Some(HintKey::Char('p'))),
    ];
    if let Some(action) = rows.get(selected).and_then(|row| row.delete_action()) {
        footer_segments.push(sep());
        footer_segments.push(("d".to_string(), key, Some(HintKey::Char('d'))));
        footer_segments.push((format!(" {action}"), hint, Some(HintKey::Char('d'))));
    }
    footer_segments.extend([
        sep(),
        ("t".to_string(), key, Some(HintKey::Char('t'))),
        (" cols".to_string(), hint, Some(HintKey::Char('t'))),
        sep(),
        ("i".to_string(), key, Some(HintKey::Char('i'))),
        (" info".to_string(), hint, Some(HintKey::Char('i'))),
        sep(),
        ("y".to_string(), key, Some(HintKey::Char('y'))),
        (" copy".to_string(), hint, Some(HintKey::Char('y'))),
        sep(),
        ("?".to_string(), key, Some(HintKey::Char('?'))),
        (" help".to_string(), hint, Some(HintKey::Char('?'))),
        sep(),
        ("esc".to_string(), key, Some(HintKey::Esc)),
        (" back".to_string(), hint, Some(HintKey::Esc)),
    ]);
    // The footer sits on the bottom border, left-aligned (starts one cell in from the corner) so
    // its click columns are predictable.
    let footer_row = area.y + area.height.saturating_sub(1);
    let footer_line = build_hint_footer(footer_segments, area.x + 1, footer_row, &mut app.hint_click);
    // The top-border `[esc back]` button stays as a redundant always-visible affordance.
    let back_text = "[esc back]";
    let back_line = Line::from(Span::styled(
        back_text,
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    ))
    .right_aligned();
    let back_end = area.x + area.width.saturating_sub(1);
    let back_start = back_end.saturating_sub(back_text.len() as u16);
    app.repo_page_back_click = Some((area.y, back_start, back_end));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(title)
        .title_top(back_line)
        .title_bottom(footer_line);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let label = Style::default().fg(Color::DarkGray);
    let head_style = Style::default().fg(Color::Green);
    let value = Style::default().fg(Color::Gray);
    let cyan = Style::default().fg(Color::Cyan);
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    // The detected fork-parent shows blue; a user override shows magenta + bold with a `*` marker.
    let base_style = Style::default().fg(Color::Blue);
    let base_override_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);

    let branch_count = rows.iter().filter(|row| row.kind == PageRowKind::Branch).count();
    let worktree_count = rows.iter().filter(|row| row.kind == PageRowKind::Worktree).count();
    let stash_count = rows.iter().filter(|row| row.kind == PageRowKind::Stash).count();
    let columns = app.effective_repo_page_columns();

    // The optional columns after the name, in a fixed order. The header row and every data row
    // are built from the same widths so they stay aligned. Count cells render a dim zero.
    let count_w = 5usize;

    // Width allocator: distribute the inner width across the visible columns. The fixed-width
    // optional columns take their share; whatever's left flows to `branch` (uncapped up to the
    // available space) and `subject` (the remainder), so hiding columns reclaims that space for
    // the text columns instead of leaving them truncated.
    let inner_w = inner.width as usize;
    let count_cell_w = 1 + count_w; // count_cell prefixes a single space
    let fixed_after_branch = if columns.ahead_behind { 12 } else { 0 } // "  " + 10
        + usize::from(columns.dirty) * count_cell_w
        + usize::from(columns.added) * count_cell_w
        + usize::from(columns.modified) * count_cell_w
        + usize::from(columns.deleted) * count_cell_w
        + usize::from(columns.total) * count_cell_w
        + if columns.upstream { 30 } else { 0 } // "  " + 28
        + if columns.base { 30 } else { 0 } // "  " + 28
        + if columns.age { 16 } else { 0 }; // "  " + 14
    let branch_floor = "branch".len() + 1; // +1 so the sort ▲/▼ never overflows the header
    let natural_branch =
        rows.iter().map(|row| row.branch.chars().count()).max().unwrap_or(8).max(branch_floor);
    const MIN_SUBJECT: usize = 24;
    // Branch may grow to its natural width, but leaves room for a readable subject when shown.
    let branch_budget = inner_w
        .saturating_sub(2 + fixed_after_branch + if columns.subject { 2 + MIN_SUBJECT } else { 0 })
        .max(branch_floor);
    let name_pad = natural_branch.min(branch_budget).max(branch_floor);
    // Subject takes whatever's left after branch + the fixed columns (its own "  " prefix aside).
    let subject_w = if columns.subject {
        inner_w.saturating_sub(2 + name_pad + fixed_after_branch + 2).max(10)
    } else {
        0
    };
    // Returns the row's optional-column spans plus the index of the `base` span within them (so
    // the caller can compute that cell's screen-column range for click hit-testing).
    let data_cells = |ahead: Option<u32>,
                      behind: Option<u32>,
                      stats: Option<crate::app::BranchStats>,
                      dirty_count: u32,
                      upstream: &str,
                      base: &str,
                      base_override: bool,
                      age: &str,
                      subject: &str|
     -> (Vec<Span<'static>>, Option<usize>) {
        let mut spans: Vec<Span> = Vec::new();
        let mut base_index = None;
        if columns.ahead_behind {
            spans.push(Span::raw("  "));
            spans.extend(ahead_behind_spans(ahead, behind, 10, icons));
        }
        if columns.dirty {
            spans.push(count_cell(icons.dirty, Some(dirty_count), count_w, Color::Yellow));
        }
        if columns.added {
            spans.push(count_cell("+", stats.map(|stat| stat.added), count_w, Color::Green));
        }
        if columns.modified {
            spans.push(count_cell("~", stats.map(|stat| stat.modified), count_w, Color::Yellow));
        }
        if columns.deleted {
            spans.push(count_cell("-", stats.map(|stat| stat.deleted), count_w, Color::Red));
        }
        if columns.total {
            spans.push(count_cell("Σ", stats.map(|stat| stat.total()), count_w, Color::Gray));
        }
        if columns.upstream {
            // Pad to the header's fixed 28-cell width so the following `base` column starts at a
            // stable screen column regardless of upstream length (or its absence on no-up rows).
            spans.push(Span::styled(format!("  {:<28}", truncate_str(upstream, 28)), label));
        }
        if columns.base {
            base_index = Some(spans.len());
            let inner = if base.is_empty() {
                "…".to_string()
            } else if base_override {
                format!("{}*", truncate_str(base, 27))
            } else {
                truncate_str(base, 28)
            };
            // Pad to the header's fixed 28-cell width so `age`/`subject` stay aligned.
            let text = format!("  {inner:<28}");
            let style = if base.is_empty() {
                label
            } else if base_override {
                base_override_style
            } else {
                base_style
            };
            spans.push(Span::styled(text, style));
        }
        if columns.age {
            spans.push(Span::styled(format!("  {:<14}", truncate_str(age, 14)), label));
        }
        if columns.subject {
            spans.push(Span::styled(format!("  {}", truncate_str(subject, subject_w)), label));
        }
        (spans, base_index)
    };

    // The column-header line, aligned to the data columns and clickable to sort. `count_cell`
    // prefixes a single space, so each count header is ` {label:<5}` to match. The active sort
    // column shows ▲/▼ (inside its fixed width) and renders bold; header_cells records each cell's
    // column range (relative to line start) so the render loop can register click targets.
    let active_sort = app.repo_page_sort;
    let sort_dir = app.repo_page_sort_dir;
    let column_header = || -> (Line<'static>, Vec<(u16, u16, RepoPageSort)>) {
        let mut spans: Vec<Span> = Vec::new();
        let mut cells: Vec<(u16, u16, RepoPageSort)> = Vec::new();
        let mut hcol: u16 = 0;
        let mut cell =
            |spans: &mut Vec<Span<'static>>, hcol: &mut u16, prefix: &str, text: &str, width: usize, sort: RepoPageSort| {
                spans.push(Span::raw(prefix.to_string()));
                *hcol += UnicodeWidthStr::width(prefix) as u16;
                let arrow = if active_sort == Some(sort) { sort_dir.arrow() } else { "" };
                let style = if active_sort == Some(sort) {
                    label.add_modifier(Modifier::BOLD)
                } else {
                    label
                };
                let body = format!("{:<width$}", format!("{text}{arrow}"));
                let body_w = UnicodeWidthStr::width(body.as_str()) as u16;
                cells.push((*hcol, *hcol + body_w, sort));
                spans.push(Span::styled(body, style));
                *hcol += body_w;
            };
        cell(&mut spans, &mut hcol, "  ", "branch", name_pad, RepoPageSort::Name);
        if columns.ahead_behind {
            cell(&mut spans, &mut hcol, "  ", "↑↓", 10, RepoPageSort::AheadBehind);
        }
        if columns.dirty {
            cell(&mut spans, &mut hcol, " ", "Δ", count_w, RepoPageSort::Dirty);
        }
        if columns.added {
            cell(&mut spans, &mut hcol, " ", "+a", count_w, RepoPageSort::Added);
        }
        if columns.modified {
            cell(&mut spans, &mut hcol, " ", "~m", count_w, RepoPageSort::Modified);
        }
        if columns.deleted {
            cell(&mut spans, &mut hcol, " ", "-d", count_w, RepoPageSort::Deleted);
        }
        if columns.total {
            cell(&mut spans, &mut hcol, " ", "Σ", count_w, RepoPageSort::Total);
        }
        if columns.upstream {
            cell(&mut spans, &mut hcol, "  ", "upstream", 28, RepoPageSort::Upstream);
        }
        if columns.base {
            cell(&mut spans, &mut hcol, "  ", "base", 28, RepoPageSort::Base);
        }
        if columns.age {
            cell(&mut spans, &mut hcol, "  ", "age", 14, RepoPageSort::Age);
        }
        if columns.subject {
            cell(&mut spans, &mut hcol, "  ", "subject", 0, RepoPageSort::Subject);
        }
        (Line::from(spans), cells)
    };

    // Section header: a colored type icon for quick recognition, then the yellow label.
    let section_header = |icon: &'static str, icon_color: Color, text: String| {
        (
            Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
                Span::styled(text, header_style),
            ]),
            None,
            None,
        )
    };

    // `PageItem` = (Line, Option<selectable index>, Option<(base_start, base_end)>) — the trailing
    // pair is the `base` cell's column range (relative to the line start) for click hit-testing;
    // None for headers/blanks/stash rows. The banner / fetch error render in a fixed bottom row.
    let mut items: Vec<PageItem> = Vec::new();

    // Tabbed mode: a clickable tab bar (Branches/Worktrees/Stashes/Commits) replaces the section
    // headers, and only the active tab's rows render (rows are already filtered to it).
    app.repo_page_tab_click.clear();
    if tabbed {
        let tabs = [
            (crate::app::RepoTab::Branches, icons.branches, "Branches", full_branches),
            (crate::app::RepoTab::Worktrees, icons.worktrees, "Worktrees", full_worktrees),
            (crate::app::RepoTab::Stashes, icons.stashes, "Stashes", full_stashes),
            (crate::app::RepoTab::Commits, "◴", "Commits", full_commits),
        ];
        let mut spans: Vec<Span> = Vec::new();
        let mut col = inner.x;
        for (tab, icon, label, count) in tabs {
            if count == 0 {
                continue;
            }
            let chip = format!(" {icon} {label} ({count}) ");
            let chip_w = UnicodeWidthStr::width(chip.as_str()) as u16;
            let style = if tab == active_tab {
                Style::default().fg(Color::Black).bg(Color::LightCyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            app.repo_page_tab_click.push((inner.y, col, col + chip_w, tab));
            spans.push(Span::styled(chip, style));
            spans.push(Span::raw(" "));
            col += chip_w + 1;
        }
        items.push((Line::from(spans), None, None));
        items.push((Line::from(String::new()), None, None));
    }

    let render_branches = !tabbed || active_tab == crate::app::RepoTab::Branches;
    let mut header_item_index = usize::MAX;
    let mut header_cells: Vec<(u16, u16, RepoPageSort)> = Vec::new();
    if render_branches {
        if !tabbed {
            items.push(section_header(
                icons.branches,
                Color::Green,
                format!("BRANCHES ({branch_count})"),
            ));
        }
        let (header_line, cells) = column_header();
        header_cells = cells;
        header_item_index = items.len();
        items.push((header_line, None, None));
    }
    for (sel_index, row) in rows.iter().enumerate() {
        if row.kind != PageRowKind::Branch {
            continue;
        }
        let marker = if row.is_head {
            Span::styled("* ", head_style)
        } else {
            Span::raw("  ")
        };
        let name_span = Span::styled(
            format!("{:<name_pad$}", truncate_str(&row.branch, name_pad)),
            if row.is_head { head_style } else { value },
        );
        let mut line_spans = vec![marker, name_span];
        let prefix_width: usize = line_spans.iter().map(|span| span.width()).sum();
        let (cells, base_index) = data_cells(
            row.ahead,
            row.behind,
            row.stats,
            row.dirty_count,
            &row.upstream.clone().unwrap_or_default(),
            &row.base.clone().unwrap_or_default(),
            row.base_is_override,
            &row.last_commit_rel,
            &row.subject,
        );
        let base_range = base_index.map(|index| {
            let start = prefix_width + cells[..index].iter().map(|span| span.width()).sum::<usize>();
            (start as u16, (start + cells[index].width()) as u16)
        });
        line_spans.extend(cells);
        items.push((Line::from(line_spans), Some(sel_index), base_range));
    }

    // Worktrees / stashes sections only appear when there's something to show.
    if worktree_count > 0 {
        if !tabbed {
            items.push((Line::from(String::new()), None, None));
            items.push(section_header(
                icons.worktrees,
                Color::Cyan,
                format!("WORKTREES ({worktree_count})"),
            ));
        }
        for (sel_index, row) in rows.iter().enumerate() {
            if row.kind != PageRowKind::Worktree {
                continue;
            }
            let name_span =
                Span::styled(format!("  {:<name_pad$}", truncate_str(&row.branch, name_pad)), cyan);
            let mut line_spans = vec![name_span];
            let prefix_width: usize = line_spans.iter().map(|span| span.width()).sum();
            let (cells, base_index) = data_cells(
                row.ahead,
                row.behind,
                row.stats,
                row.dirty_count,
                &row.upstream.clone().unwrap_or_default(),
                &row.base.clone().unwrap_or_default(),
                row.base_is_override,
                &row.last_commit_rel,
                &row.path.display().to_string(),
            );
            let base_range = base_index.map(|index| {
                let start =
                    prefix_width + cells[..index].iter().map(|span| span.width()).sum::<usize>();
                (start as u16, (start + cells[index].width()) as u16)
            });
            line_spans.extend(cells);
            items.push((Line::from(line_spans), Some(sel_index), base_range));
        }
    }

    if stash_count > 0 {
        if !tabbed {
            items.push((Line::from(String::new()), None, None));
            items.push(section_header(
                icons.stashes,
                Color::Magenta,
                format!("STASHES ({stash_count})"),
            ));
        }
        for (sel_index, row) in rows.iter().enumerate() {
            if row.kind != PageRowKind::Stash {
                continue;
            }
            let stash_ref = format!("stash@{{{}}}", row.stash_index.unwrap_or(0));
            items.push((
                Line::from(vec![
                    Span::styled(format!("  {stash_ref:<10}"), Style::default().fg(Color::Magenta)),
                    Span::styled(format!("  {}", truncate_str(&row.branch, 70)), value),
                ]),
                Some(sel_index),
                None,
            ));
        }
    }

    // Commits tab: a read-only list of recent commits (sha · date · author · subject). Rendered
    // here (not via the row machinery — commits aren't PageRows).
    if tabbed && active_tab == crate::app::RepoTab::Commits {
        let commits = app
            .repos
            .get(idx)
            .and_then(|repo| repo.lock().unwrap().page.as_ref().map(|page| page.commits.clone()))
            .unwrap_or_default();
        for commit in &commits {
            items.push((
                Line::from(vec![
                    Span::styled(format!("  {:<10}", commit.sha), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{:<16}", truncate_str(&commit.rel_date, 15)), label),
                    Span::styled(format!("{:<18}", truncate_str(&commit.author, 17)), cyan),
                    Span::raw(truncate_str(&commit.subject, 60)),
                ]),
                None,
                None,
            ));
        }
    }

    // Carve fixed rows off the bottom of `inner`, bottom-up: banner, toggle menu, info panel.
    let banner = app
        .repo_page_message
        .clone()
        .map(|message| (format!(" {message}"), Color::Yellow))
        .or_else(|| fetch_error.as_ref().map(|error| (format!(" fetch: {error}"), Color::Red)));
    let selected_row = rows.get(selected);
    let info_lines = if app.repo_page_info {
        selected_row.map(|row| {
            let (base, pr) = {
                let state = app.repos[idx].lock().unwrap();
                let base = state.page.as_ref().and_then(|page| page.base_branch.clone());
                (base, state.pr.clone())
            };
            build_repo_page_info_lines(row, base.as_deref(), pr.as_ref())
        })
    } else {
        None
    };

    let mut body = inner;
    let mut take_bottom = |height: u16| -> Rect {
        let height = height.min(body.height);
        let area = Rect { y: body.y + body.height - height, height, ..body };
        body.height -= height;
        area
    };
    let banner_area = banner.as_ref().map(|_| take_bottom(1));
    let toggle_area = app.repo_page_toggle.then(|| take_bottom(1));
    let info_area = info_lines.as_ref().map(|lines| take_bottom(lines.len() as u16 + 2));
    let inner = body;
    app.repo_page_inner = inner;

    let inner_height = inner.height as usize;
    let max_scroll = items.len().saturating_sub(inner_height);
    if app.repo_page_scroll > max_scroll {
        app.repo_page_scroll = max_scroll;
    }
    let start = app.repo_page_scroll;
    let end = (start + inner_height).min(items.len());

    let selection_fg = app.palette().selection_fg;
    let sel_style = selection_highlight_style(app);
    let selection_is_blue = app.selection_style == crate::app::SelectionStyle::Blue;
    app.repo_page_click.clear();
    app.base_cell_click.clear();
    app.repo_page_sort_click.clear();
    let mut lines: Vec<Line> = Vec::with_capacity(end.saturating_sub(start));
    for (offset, (line, sel, base_range)) in items[start..end].iter().enumerate() {
        let mut line = line.clone();
        // Register the clickable sort headers when the header row is on screen.
        if start + offset == header_item_index {
            let screen_row = inner.y + offset as u16;
            for (col_start, col_end, sort) in &header_cells {
                app.repo_page_sort_click.push((
                    screen_row,
                    inner.x + col_start,
                    inner.x + col_end,
                    *sort,
                ));
            }
        }
        if let Some(sel_index) = sel {
            let screen_row = inner.y + offset as u16;
            app.repo_page_click.push((screen_row, *sel_index));
            if let Some((start_col, end_col)) = base_range {
                app.base_cell_click.push((
                    screen_row,
                    inner.x + *start_col,
                    inner.x + *end_col,
                    *sel_index,
                ));
            }
            if *sel_index == selected {
                line.style = sel_style;
                // Blue style: force every span to white + bold so the row reads uniformly over the
                // solid bar (column colors would otherwise win over the line style). Subtle style:
                // keep each column's own color, just bold.
                for span in &mut line.spans {
                    span.style = if selection_is_blue {
                        span.style.fg(selection_fg).add_modifier(Modifier::BOLD)
                    } else {
                        span.style.add_modifier(Modifier::BOLD)
                    };
                }
            }
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
    let track = scrollbar_track(area, inner);
    render_scrollbar(
        frame,
        track,
        app.repo_page_scroll,
        items.len(),
        inner_height,
        app.scrollbar_dragging == Some(ScrollKind::RepoPage),
    );
    app.scroll_hits.push(ScrollHit {
        kind: ScrollKind::RepoPage,
        track,
        total: items.len(),
        viewport: inner_height,
    });

    // Info panel: a bordered box showing details of the selected row.
    if let (Some(area), Some(info_lines)) = (info_area, info_lines) {
        let info_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(label)
            .title(" info ");
        let info_inner = info_block.inner(area);
        frame.render_widget(info_block, area);
        frame.render_widget(Paragraph::new(info_lines), info_inner);
    }

    // Column-toggle menu: a chip row (active ●, off ○, unavailable dim & inert), captured for clicks.
    app.repo_page_toggle_click.clear();
    if let Some(area) = toggle_area {
        // Unavailable columns: pre-blend `faint` hard toward the resolved background so they
        // clearly recede. The generic DIM materialization leaves them too close to the off
        // columns (and doesn't always fire), so set the dim color explicitly here. On a terminal
        // background the bg isn't an RGB value to blend, so keep the DIM attribute (native dim).
        let palette = app.palette();
        let unavailable_style = match palette.bg {
            Color::Rgb(..) => {
                Style::default().fg(crate::theme::blend_toward(palette.faint, palette.bg, 0.72))
            }
            _ => Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
        };
        let entries: [(RepoPageColumn, &str, &str, bool); 9] = [
            (RepoPageColumn::AheadBehind, "b", "↑↓", columns.ahead_behind),
            (RepoPageColumn::Dirty, "y", "dirty", columns.dirty),
            (RepoPageColumn::Added, "a", "added", columns.added),
            (RepoPageColumn::Modified, "m", "modified", columns.modified),
            (RepoPageColumn::Deleted, "d", "deleted", columns.deleted),
            (RepoPageColumn::Total, "c", "total", columns.total),
            (RepoPageColumn::Upstream, "u", "upstream", columns.upstream),
            (RepoPageColumn::Age, "g", "age", columns.age),
            (RepoPageColumn::Subject, "s", "subject", columns.subject),
        ];
        let mut spans: Vec<Span> = vec![Span::styled(" cols: ", label)];
        let mut col = area.x + 7;
        for (column, letter, name, on) in entries {
            let available = app.repo_page_column_available(column);
            // Three distinct states: on `●` (green), off `○` (gray), unavailable `–` (faint,
            // non-circular so it doesn't read as just another off column, and inert).
            let mark = if !available {
                "–"
            } else if on {
                "●"
            } else {
                "○"
            };
            let chip = format!("{mark} {letter} {name}");
            let chip_width = UnicodeWidthStr::width(chip.as_str()) as u16;
            let style = if !available {
                unavailable_style
            } else if on {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Gray)
            };
            if available {
                app.repo_page_toggle_click.push((area.y, col, col + chip_width, column));
            }
            spans.push(Span::styled(chip, style));
            spans.push(Span::raw("  "));
            col += chip_width + 2;
        }
        spans.push(Span::styled("esc", Style::default().fg(Color::Cyan)));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    // The action banner / fetch error sits in its reserved bottom row.
    if let (Some((text, color)), Some(area)) = (banner, banner_area) {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))),
            area,
        );
    }
}

/// Render the yes/no confirmation dialog (keyboard-driven: y / n / Esc).
/// Render the build-info modal (opened by clicking the "built … ago" status tag): the running
/// version, the watched executable path, when it was built, and how new-build watching works.
fn render_build_info(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let built = app
        .binary_built
        .and_then(|built| built.elapsed().ok())
        .map(|age| crate::app::format_ago(age.as_secs()))
        .unwrap_or_else(|| "unknown".to_string());
    let label = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
    let value = Style::default().fg(Color::Gray);
    let dim = Style::default().fg(Color::DarkGray);
    let field = |name: &str, text: String| {
        Line::from(vec![
            Span::styled(format!("{name:<9}"), label),
            Span::styled(text, value),
        ])
    };

    let mut lines: Vec<Line> = vec![
        field("Version", concat!("v", env!("CARGO_PKG_VERSION")).to_string()),
        field("Built", built),
        field("Path", app.exe_path.clone()),
        Line::from(String::new()),
        Line::from(Span::styled("Watching this file for new builds", label)),
        Line::from(Span::styled(
            "polygit polls this executable's size + mtime every few seconds. When a newer",
            dim,
        )),
        Line::from(Span::styled(
            "build lands at the same path (e.g. make install's atomic rename), a ↺ [reload]",
            dim,
        )),
        Line::from(Span::styled("notice appears top-right on every screen.", dim)),
        Line::from(String::new()),
    ];
    let status = if app.update_available && !app.update_dismissed {
        Span::styled(
            "● A new build is available — click [reload] to restart.",
            Style::default().fg(Color::Yellow),
        )
    } else if app.update_dismissed {
        Span::styled("○ A new build was dismissed; it re-arms if the file changes.", dim)
    } else {
        Span::styled("✓ Running the latest build on disk.", Style::default().fg(Color::Green))
    };
    lines.push(Line::from(status));

    let pad = if app.panel_padding { 2 } else { 0 };
    let content_width = lines.iter().map(|line| line.width()).max().unwrap_or(40) as u16 + 4 + pad;
    let width = content_width.clamp(40, area.width.saturating_sub(4).max(40));
    // Allow two extra rows in case a long path wraps.
    let height = (lines.len() as u16 + 4 + pad).min(area.height.saturating_sub(2).max(8));
    let modal = centered_rect(width, height, area);
    let (close_line, close_click) = modal_close_button(modal);
    // Clickable bottom-border footer: `r` exec-restarts the binary (same as the reload notice),
    // `esc` closes.
    let mut footer: Vec<(String, Style, Option<HintKey>)> = Vec::new();
    footer.extend(footer_chip("r", " restart", HintKey::Char('r')));
    footer.push(footer_sep());
    footer.extend(footer_chip("esc", " close", HintKey::Esc));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Build info ")
        .title_top(close_line)
        .title_bottom(modal_border_footer(footer, modal, &mut app.hint_click));
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    app.build_info_close_click = close_click;
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_confirm(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let Some(confirm) = &app.confirm else {
        return;
    };
    // Cap how many files we enumerate so a huge dirty tree can't overflow the screen.
    let max_per_list = 10usize;
    let has_files = !confirm.restore_files.is_empty() || !confirm.delete_files.is_empty();

    // Widen to fit the longest file line (with its two-space indent) when listing files.
    let file_width = confirm
        .restore_files
        .iter()
        .chain(confirm.delete_files.iter())
        .map(|file| file.chars().count() + 4)
        .max()
        .unwrap_or(0) as u16;
    // Padding eats 2 rows/cols inside the border; grow the box so content still fits.
    let pad = if app.panel_padding { 2 } else { 0 };
    let content_width = (confirm.message.chars().count() as u16 + 8).max(file_width) + pad;
    let width = content_width.clamp(30, area.width.saturating_sub(4).max(30));

    // Build the file-detail body first so we can size the dialog to it.
    let mut detail_lines: Vec<Line> = Vec::new();
    let mut push_file_section = |files: &[String], label: &str, color: Color| {
        if files.is_empty() {
            return;
        }
        detail_lines.push(Line::from(Span::styled(
            format!("  {label} ({}):", files.len()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        for file in files.iter().take(max_per_list) {
            detail_lines.push(Line::from(Span::styled(
                format!("    {file}"),
                Style::default().fg(color),
            )));
        }
        if files.len() > max_per_list {
            detail_lines.push(Line::from(Span::styled(
                format!("    … and {} more", files.len() - max_per_list),
                Style::default().fg(Color::DarkGray),
            )));
        }
    };
    push_file_section(&confirm.restore_files, "Restore", Color::Yellow);
    push_file_section(&confirm.delete_files, "Delete", Color::Red);

    // Base height: borders + blank + message (+ blank + danger warning) + blank + prompt. Add
    // the file body plus a separating blank line when there are files to list.
    let mut height = if confirm.danger { 8 } else { 6 };
    if has_files {
        height += detail_lines.len() as u16 + 1;
    }
    height += pad;
    let height = height.min(area.height.saturating_sub(2).max(6));

    let icons = app.icons();
    let modal = centered_rect(width, height, area);
    let (border_color, title) = if confirm.danger {
        (Color::Red, format!(" {} Confirm — destructive ", icons.warning))
    } else {
        (Color::Yellow, " Confirm ".to_string())
    };
    let (close_line, close_click) = modal_close_button(modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(border_color))
        .title(title)
        .title_top(close_line);
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    let danger = confirm.danger;
    let message = confirm.message.clone();
    app.confirm_area = modal;
    app.confirm_close_click = close_click;
    let mut lines = vec![
        Line::from(String::new()),
        Line::from(Span::styled(format!("  {message}"), Style::default().fg(Color::Gray))),
    ];
    if has_files {
        lines.push(Line::from(String::new()));
        lines.append(&mut detail_lines);
    }
    if danger {
        lines.push(Line::from(String::new()));
        lines.push(Line::from(Span::styled(
            format!("  {} This cannot be undone.", icons.warning),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(String::new()));
    // The yes/no prompt — both halves are clickable.
    let yes_text = "[y/enter] yes";
    let gap = "     ";
    let no_text = "[n] no";
    let prompt_y = inner.y + lines.len() as u16;
    if prompt_y < inner.y + inner.height {
        let yes_start = inner.x + 2;
        let yes_end = yes_start + yes_text.len() as u16;
        let no_start = yes_end + gap.len() as u16;
        app.confirm_yes_click = Some((prompt_y, yes_start, yes_end));
        app.confirm_no_click = Some((prompt_y, no_start, no_start + no_text.len() as u16));
    } else {
        app.confirm_yes_click = None;
        app.confirm_no_click = None;
    }
    lines.push(Line::from(Span::styled(
        format!("  {yes_text}{gap}{no_text}"),
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Settings label column width — fits the longest label ("Changed-row highlight" = 21).
const SETTINGS_LABEL_W: u16 = 22;

/// Render one settings row — `> Label   ● value  ○ value` — and capture its label/chip click
/// regions (keyed by the global `row_idx`). `left_x` is the row's left edge.
/// The option index to underline for a radio row (Theme only): when `auto` is selected, underline
/// the autodetected option it resolves to (`dark`=1 / `light`=2). `None` for every other row/state.
fn radio_underline_idx(app: &AppState, row_idx: usize) -> Option<usize> {
    if row_idx == 5 && app.theme == crate::app::Theme::Auto {
        Some(if app.auto_dark { 1 } else { 2 })
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn settings_row_line(
    row_idx: usize,
    selected: bool,
    label: &str,
    options: &[(&str, bool)],
    pos: (u16, u16),
    in_view: bool,
    underline_idx: Option<usize>,
    disabled: bool,
    query: Option<&str>,
    clicks: &mut Vec<(u16, u16, u16, usize, Option<usize>)>,
) -> Line<'static> {
    let (left_x, row_y) = pos;
    let cursor = if selected { "> " } else { "  " };
    // A disabled row reads dim and inert (no click regions) — e.g. Hide zeros under emoji icons,
    // which always hides zeros, so the radio is force-selected and not togglable.
    let label_style = if disabled {
        Style::default().fg(Color::DarkGray)
    } else if selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let padded = format!("{label:<width$}", width = SETTINGS_LABEL_W as usize);
    let mut spans = vec![Span::styled(format!("  {cursor}"), label_style)];
    // Highlight the search-matched chars of the label (the padding stays plain).
    match query.and_then(|query| tui_pick::finder::fuzzy_match(label, query)) {
        Some((_, matched)) if !matched.is_empty() => {
            let hl = label_style.fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            let set: std::collections::HashSet<usize> = matched.into_iter().collect();
            for (idx, ch) in padded.chars().enumerate() {
                let style = if set.contains(&idx) { hl } else { label_style };
                spans.push(Span::styled(ch.to_string(), style));
            }
        }
        _ => spans.push(Span::styled(padded, label_style)),
    }
    let mut col = left_x + 4;
    if in_view && !disabled {
        clicks.push((row_y, col, col + SETTINGS_LABEL_W, row_idx, None));
    }
    col += SETTINGS_LABEL_W;
    for (option_idx, (text, active)) in options.iter().enumerate() {
        if option_idx > 0 {
            spans.push(Span::raw("  "));
            col += 2;
        }
        let mut style = if disabled {
            Style::default().fg(Color::DarkGray)
        } else if *active {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        // Underline the autodetected option (the theme `auto` resolves to) for a subtle hint.
        if underline_idx == Some(option_idx) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        let chip = format!("{} {text}", if *active { "●" } else { "○" });
        let chip_width = UnicodeWidthStr::width(chip.as_str()) as u16;
        if in_view && !disabled {
            clicks.push((row_y, col, col + chip_width, row_idx, Some(option_idx)));
        }
        col += chip_width;
        spans.push(Span::styled(chip, style));
    }
    Line::from(spans)
}

/// Render the settings modal (`,`): IDE-style vertical tabs (or a flat list — toggle with `v`).
/// `↑↓` move, `←→`/`tab` switch tab, `space`/`enter` toggle, `esc` closes.
fn render_settings(frame: &mut Frame, app: &mut AppState, area: Rect) {
    use crate::app::{
        Background, ButtonHoverStyle, Contrast, SelectionStyle, SettingsLayout, Theme, SETTINGS_TABS,
    };
    let emoji = app.icon_style == crate::app::IconStyle::Emoji;
    let hide_zero = app.hide_zero_counts;
    let hide_lines = app.hide_folder_lines;
    // Sections of (label, option chips). Global row indices run across sections and must
    // match `set_setting_option` / `toggle_selected_setting`:
    // 0 grouping · 1 tree · 2 hide-folder-lines (Lists), 3 icons · 4 hide-zeros · 5 theme ·
    // 6 background · 7 contrast · 8 selection · 9 button-hover (Theming), 10 auto-pull · 11 limit ·
    // 12 auto-pull-in-tree (Sync), 13 hover · 14 changed-row flash · 15 changed-row highlight
    // (Interaction), 16 padding · 17 borders · 18 splitter · 19 repo-page tabs · 20 dock ·
    // 21 branch-check (Layout).
    type SettingsRow<'a> = (&'a str, Vec<(&'a str, bool)>);
    let sections: Vec<(&str, Vec<SettingsRow>)> = vec![
        (
            "Lists",
            vec![
                ("Grouping", vec![("on", app.grouping_enabled), ("off", !app.grouping_enabled)]),
                ("Tree view", vec![("on", app.tree_enabled), ("off", !app.tree_enabled)]),
                ("Hide folder lines", vec![("on", hide_lines), ("off", !hide_lines)]),
            ],
        ),
        (
            "Theming",
            vec![
                ("Icons", vec![("unicode", !emoji), ("emoji", emoji)]),
                // Emoji always hides zeros, so force "on" and let push_row render the row disabled.
                (
                    "Hide zeros",
                    vec![("on", hide_zero || emoji), ("off", !hide_zero && !emoji)],
                ),
                (
                    "Theme",
                    vec![
                        ("auto", app.theme == Theme::Auto),
                        ("dark", app.theme == Theme::Dark),
                        ("light", app.theme == Theme::Light),
                    ],
                ),
                (
                    "Background",
                    vec![
                        ("normal", app.background == Background::Normal),
                        ("soft", app.background == Background::Soft),
                        ("terminal", app.background == Background::Terminal),
                    ],
                ),
                (
                    "Contrast",
                    vec![
                        ("normal", app.contrast == Contrast::Normal),
                        ("soft", app.contrast == Contrast::Soft),
                    ],
                ),
                (
                    "List selection",
                    vec![
                        ("blue", app.selection_style == SelectionStyle::Blue),
                        ("subtle", app.selection_style == SelectionStyle::Subtle),
                    ],
                ),
                (
                    "Button hover",
                    vec![
                        ("inverted", app.button_hover_style == ButtonHoverStyle::Inverted),
                        ("subtle", app.button_hover_style == ButtonHoverStyle::Subtle),
                    ],
                ),
            ],
        ),
        (
            "Sync",
            vec![
                (
                    "Auto-pull on launch",
                    vec![("on", app.auto_pull_on_launch), ("off", !app.auto_pull_on_launch)],
                ),
                (
                    "Auto-pull limit",
                    vec![
                        ("50", app.auto_pull_max_repos == 50),
                        ("100", app.auto_pull_max_repos == 100),
                        ("250", app.auto_pull_max_repos == 250),
                        ("\u{221e}", app.auto_pull_max_repos == 0),
                    ],
                ),
                (
                    "Auto-pull in tree",
                    vec![("on", app.auto_pull_in_tree), ("off", !app.auto_pull_in_tree)],
                ),
            ],
        ),
        (
            "Interaction",
            vec![
                ("Hover effects", vec![("on", app.hover_effects), ("off", !app.hover_effects)]),
                (
                    "Changed-row flash",
                    vec![("on", app.changed_row_flash), ("off", !app.changed_row_flash)],
                ),
                (
                    "Changed-row highlight",
                    vec![("on", app.changed_row_highlight), ("off", !app.changed_row_highlight)],
                ),
            ],
        ),
        (
            "Layout",
            vec![
                ("Panel padding", vec![("on", app.panel_padding), ("off", !app.panel_padding)]),
                ("Borders", vec![("on", app.show_borders), ("off", !app.show_borders)]),
                ("Splitter", vec![("on", app.show_splitter), ("off", !app.show_splitter)]),
                (
                    "Repo page tabs",
                    vec![
                        ("off", app.repo_page_tabs == crate::app::RepoTabsMode::Off),
                        ("auto", app.repo_page_tabs == crate::app::RepoTabsMode::Auto),
                    ],
                ),
                (
                    "Dock repo page",
                    vec![("on", app.dock_repo_panel), ("off", !app.dock_repo_panel)],
                ),
                (
                    "Auto branch-check",
                    vec![
                        ("off", app.branch_check == crate::app::BranchCheck::Off),
                        ("auto", app.branch_check == crate::app::BranchCheck::Auto),
                    ],
                ),
            ],
        ),
    ];

    // Flatten the sections into the global row order (`SETTINGS_TABS` defines the grouping).
    let all_rows: Vec<SettingsRow> = sections.into_iter().flat_map(|(_, rows)| rows).collect();
    let row_width = |options: &[(&str, bool)]| -> u16 {
        let chips: u16 = options
            .iter()
            .enumerate()
            .map(|(idx, (text, _))| {
                UnicodeWidthStr::width(format!("● {text}").as_str()) as u16 + u16::from(idx > 0) * 2
            })
            .sum();
        4 + SETTINGS_LABEL_W + chips
    };
    let content_w = all_rows.iter().map(|(_, opts)| row_width(opts)).max().unwrap_or(40);
    let tabbed = app.settings_layout == SettingsLayout::Tabbed;
    let tab_col_w = SETTINGS_TABS
        .iter()
        .map(|(name, _)| UnicodeWidthStr::width(*name) as u16 + 4)
        .max()
        .unwrap_or(12);
    let max_tab_rows =
        SETTINGS_TABS.iter().map(|(_, count)| *count).max().unwrap_or(1) as u16 + 1; // +1 groups hint
    let groups_hint = usize::from(app.groups.is_empty());

    let accordion = app.settings_layout == SettingsLayout::Accordion;
    let pad = if app.panel_padding { 2 } else { 0 };
    // The hint footer now lives on the bottom border, not an in-content row, so the content
    // height no longer reserves a trailing row for it.
    // The search box reserves 2 rows at the top of every layout. When a query is active the body
    // becomes a flat filtered list (one row per match + a count line), regardless of layout.
    let search_active = !app.settings_search.is_empty();
    let filtered_rows = app.settings_filtered_rows();
    let search_rows = 2u16;
    let (base_width, base_rows) = if tabbed {
        (tab_col_w + 1 + content_w, max_tab_rows.max(SETTINGS_TABS.len() as u16) + 1)
    } else if accordion {
        // collapse-all button + blank, then per section: a header + (expanded) its rows.
        let mut rows = 2u16;
        for (tab_idx, (name, count)) in SETTINGS_TABS.iter().enumerate() {
            rows += 1;
            if !app.settings_section_collapsed(tab_idx) {
                rows += *count as u16;
                if *name == "General" && app.groups.is_empty() {
                    rows += 1;
                }
            }
        }
        (content_w.max(40), rows)
    } else {
        let row_count = all_rows.len() as u16;
        (content_w.max(40), row_count + SETTINGS_TABS.len() as u16 * 2 + groups_hint as u16)
    };
    let (width, content_rows) = if search_active {
        (content_w.max(40), filtered_rows.len() as u16 + 1 + search_rows)
    } else {
        (base_width, base_rows + search_rows)
    };
    let width = (width + 2 + pad).min(area.width.saturating_sub(2)).max(20);
    let height = (content_rows + 2 + pad).min(area.height.saturating_sub(2).max(6));
    let modal = centered_rect(width, height, area);
    let (close_line, close_click) = modal_close_button(modal);
    // One clickable footer on the bottom border — the single source of every settings hint (the
    // old layout doubled them: an in-content key row AND a plain border line). `move` is
    // informational; tab / space / enter / v / esc inject their keys.
    let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let hint = Style::default().fg(Color::DarkGray);
    let mut footer: Vec<(String, Style, Option<HintKey>)> =
        vec![("↑↓".to_string(), key, None), (" move".to_string(), hint, None), footer_sep()];
    if tabbed {
        footer.push(("←→/tab".to_string(), key, Some(HintKey::Tab)));
        footer.push((" tab".to_string(), hint, Some(HintKey::Tab)));
        footer.push(footer_sep());
    }
    footer.push(("space".to_string(), key, Some(HintKey::Char(' '))));
    footer.push(("/".to_string(), hint, None));
    footer.push(("enter".to_string(), key, Some(HintKey::Enter)));
    footer.push((" toggle".to_string(), hint, Some(HintKey::Enter)));
    footer.push(footer_sep());
    footer.extend(footer_chip("v", app.settings_layout.next_label(), HintKey::Char('v')));
    footer.push(footer_sep());
    footer.extend(footer_chip("esc", " close", HintKey::Esc));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Settings ")
        .title_top(close_line)
        .title_bottom(modal_border_footer(footer, modal, &mut app.hint_click));
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    app.settings_area = modal;
    app.settings_close_click = close_click;
    app.settings_click.clear();
    app.settings_tab_click.clear();
    app.settings_section_click.clear();
    app.settings_collapse_all_click = None;

    // Search box at the top of every layout (filters rows across all tabs); `/` focuses it.
    let full_inner = inner;
    let cursor = if app.settings_search_focused { "\u{2588}" } else { "" };
    let mut search_spans = vec![
        Span::styled("Search: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{}{cursor}", app.settings_search),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
    ];
    if app.settings_search.is_empty() && !app.settings_search_focused {
        search_spans.push(Span::styled("(/ to search)", Style::default().fg(Color::DarkGray)));
    }
    frame.render_widget(Paragraph::new(Line::from(search_spans)), Rect { height: 1, ..full_inner });
    app.settings_search_click = Some((full_inner.y, full_inner.x, full_inner.x + full_inner.width));
    // The body sits below the search box (+ a blank spacer row).
    let inner = Rect {
        y: full_inner.y + search_rows,
        height: full_inner.height.saturating_sub(search_rows),
        ..full_inner
    };

    let section_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    // Precomputed (not via `app` inside the closure, which would conflict with the closure's
    // disjoint field borrows): the Theme row's autodetect underline.
    let theme_underline = radio_underline_idx(app, 5);
    let emoji_icons = app.icon_style == crate::app::IconStyle::Emoji;
    // Precomputed before push_row (its closure borrows app.settings_click, so a `&self` method call
    // mid-loop would conflict): per-section collapse state for the accordion layout.
    let section_collapsed: Vec<bool> =
        (0..SETTINGS_TABS.len()).map(|tab_idx| app.settings_section_collapsed(tab_idx)).collect();
    let all_collapsed = app.settings_all_collapsed();
    // A `>Label  ● value` row plus the optional "no groups" hint, given the row's left edge.
    let mut push_row = |row_idx: usize, left_x: u16, row_y: u16, out: &mut Vec<Line>| {
        let (label, options) = &all_rows[row_idx];
        let in_view = row_y < inner.y + inner.height;
        let underline_idx = if row_idx == 5 { theme_underline } else { None };
        // Hide zeros (row 4) is inert under emoji icons (which always hide zeros).
        let disabled = row_idx == 4 && emoji_icons;
        out.push(settings_row_line(
            row_idx,
            app.settings_selected == row_idx,
            label,
            options,
            (left_x, row_y),
            in_view,
            underline_idx,
            disabled,
            None,
            &mut app.settings_click,
        ));
        if *label == "Grouping" && app.groups.is_empty() {
            out.push(Line::from(Span::styled(
                "      no groups defined — ~/.config/polygit/groups.json",
                Style::default().fg(Color::DarkGray),
            )));
        }
    };

    if search_active {
        // A flat list of the matching rows with the matched chars highlighted (ignores tabs).
        let query = app.settings_search.clone();
        let mut lines: Vec<Line> = vec![Line::from(Span::styled(
            format!(
                "  {} match{}",
                filtered_rows.len(),
                if filtered_rows.len() == 1 { "" } else { "es" }
            ),
            Style::default().fg(Color::DarkGray),
        ))];
        for &row_idx in &filtered_rows {
            let (label, options) = &all_rows[row_idx];
            let row_y = inner.y + lines.len() as u16;
            let in_view = row_y < inner.y + inner.height;
            let underline_idx = if row_idx == 5 { theme_underline } else { None };
            let disabled = row_idx == 4 && emoji_icons;
            lines.push(settings_row_line(
                row_idx,
                app.settings_selected == row_idx,
                label,
                options,
                (inner.x, row_y),
                in_view,
                underline_idx,
                disabled,
                Some(query.as_str()),
                &mut app.settings_click,
            ));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    } else if tabbed {
        // Left: clickable vertical tab list. Right: the active tab's rows.
        let mut tab_lines: Vec<Line> = Vec::new();
        for (tab_idx, (name, _)) in SETTINGS_TABS.iter().enumerate() {
            let row_y = inner.y + tab_idx as u16;
            let active = tab_idx == app.settings_tab;
            let style = if active {
                Style::default().fg(Color::Black).bg(Color::LightCyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            app.settings_tab_click.push((row_y, inner.x, inner.x + tab_col_w, tab_idx));
            tab_lines.push(Line::from(Span::styled(
                format!(" {name:<width$}", width = (tab_col_w - 1) as usize),
                style,
            )));
        }
        let tabs_area = Rect { width: tab_col_w, ..inner };
        frame.render_widget(Paragraph::new(tab_lines), tabs_area);

        let content_x = inner.x + tab_col_w + 1;
        let (start, len) = AppState::settings_tab_range(app.settings_tab);
        let mut content_lines: Vec<Line> = Vec::new();
        for offset in 0..len {
            // Visual-only group separators (tabbed view): a blank row before certain settings.
            // Nav skips them (it moves by row index) and they register no click region.
            if AppState::settings_tabbed_blank_before(start + offset) {
                content_lines.push(Line::from(""));
            }
            let row_y = inner.y + content_lines.len() as u16;
            push_row(start + offset, content_x, row_y, &mut content_lines);
        }
        let content_area = Rect {
            x: content_x,
            width: inner.width.saturating_sub(tab_col_w + 1),
            ..inner
        };
        frame.render_widget(Paragraph::new(content_lines), content_area);
    } else if accordion {
        let mut lines: Vec<Line> = Vec::new();
        // Expand/collapse-all button (label flips once every section is collapsed).
        let btn_label = if all_collapsed { "[+ expand all]" } else { "[- collapse all]" };
        let btn_y = inner.y + lines.len() as u16;
        let btn_w = UnicodeWidthStr::width(btn_label) as u16;
        app.settings_collapse_all_click = Some((btn_y, inner.x + 2, inner.x + 2 + btn_w));
        lines.push(Line::from(Span::styled(
            format!("  {btn_label}"),
            Style::default().fg(Color::Cyan),
        )));
        lines.push(Line::from(String::new()));
        let selected_section = AppState::settings_tab_of_row(app.settings_selected);
        let mut row_idx = 0usize;
        for (tab_idx, (name, count)) in SETTINGS_TABS.iter().enumerate() {
            let collapsed = section_collapsed[tab_idx];
            let chevron = if collapsed { "\u{25b8}" } else { "\u{25be}" }; // ▸ / ▾
            // The section owning the selection gets a cursor + brighter style so a selection hidden
            // inside a collapsed section stays discoverable (←/→ collapse/expand it).
            let owns_selection = tab_idx == selected_section;
            let cursor = if owns_selection { ">" } else { " " };
            let header = format!(" {cursor}{chevron} {name}");
            let header_y = inner.y + lines.len() as u16;
            let header_w = UnicodeWidthStr::width(header.as_str()) as u16;
            app.settings_section_click.push((header_y, inner.x, inner.x + header_w, tab_idx));
            let style = if owns_selection {
                section_style.add_modifier(Modifier::REVERSED)
            } else {
                section_style
            };
            lines.push(Line::from(Span::styled(header, style)));
            for _ in 0..*count {
                if !collapsed {
                    let row_y = inner.y + lines.len() as u16;
                    push_row(row_idx, inner.x, row_y, &mut lines);
                }
                row_idx += 1;
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);
    } else {
        let mut lines: Vec<Line> = Vec::new();
        if !app.panel_padding {
            lines.push(Line::from(String::new()));
        }
        let mut row_idx = 0usize;
        for (tab_idx, (name, count)) in SETTINGS_TABS.iter().enumerate() {
            if tab_idx > 0 {
                lines.push(Line::from(String::new()));
            }
            lines.push(Line::from(Span::styled(format!("  {name}"), section_style)));
            for _ in 0..*count {
                let row_y = inner.y + lines.len() as u16;
                push_row(row_idx, inner.x, row_y, &mut lines);
                row_idx += 1;
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
    // The settings hint footer lives on the bottom border (built above); no in-content row.
}

/// Render the persistent new-build notice (top-right): shown when a newer binary replaced the
/// running one on disk, with clickable `[reload]` (exec the new build) and `[x]` (dismiss).
/// Sits 1 cell in from the top/right (one more with panel padding on), with a glint sweeping
/// around its border to catch the eye.
fn render_update_notice(frame: &mut Frame, app: &mut AppState, area: Rect, tick: u64) {
    if !app.update_available || app.update_dismissed {
        app.update_reload_click = None;
        app.update_close_click = None;
        return;
    }
    let message = " ↺ new build installed · ";
    let reload = "[reload]";
    let close = " [x] ";
    let content_width = (UnicodeWidthStr::width(message)
        + UnicodeWidthStr::width(reload)
        + UnicodeWidthStr::width(close)) as u16;
    let width = (content_width + 2).min(area.width);
    let inset = u16::from(app.panel_padding);
    let notice_area = Rect {
        x: area.x + area.width.saturating_sub(width + 2 + inset),
        y: area.y + 1 + inset,
        width,
        height: 3.min(area.height),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(notice_area);
    cast_shadow(frame, notice_area);
    frame.render_widget(Clear, notice_area);
    frame.render_widget(block, notice_area);

    let reload_start = inner.x + UnicodeWidthStr::width(message) as u16;
    let reload_end = reload_start + reload.len() as u16;
    let close_start = reload_end + 1;
    let close_end = close_start + 3;
    app.update_reload_click = Some((inner.y, reload_start, reload_end));
    app.update_close_click = Some((inner.y, close_start, close_end));

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(message, Style::default().fg(Color::Yellow)),
            Span::styled(
                reload,
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(close, Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        ])),
        inner,
    );

    // Border shine: a short accent glint sweeping clockwise around the border, one cell per
    // tick (free under render-every-tick). Skip degenerate boxes.
    if notice_area.width >= 4 && notice_area.height >= 3 {
        let left = notice_area.x;
        let right = notice_area.x + notice_area.width - 1;
        let top = notice_area.y;
        let bottom = notice_area.y + notice_area.height - 1;
        let mut perimeter: Vec<(u16, u16)> = Vec::new();
        perimeter.extend((left..=right).map(|col| (col, top)));
        perimeter.extend((top + 1..bottom).map(|row| (right, row)));
        perimeter.extend((left..=right).rev().map(|col| (col, bottom)));
        perimeter.extend((top + 1..bottom).rev().map(|row| (left, row)));
        let offset = tick as usize % perimeter.len();
        let buffer = frame.buffer_mut();
        for step in 0..6 {
            let (col, row) = perimeter[(offset + step) % perimeter.len()];
            if let Some(cell) = buffer.cell_mut((col, row)) {
                cell.set_fg(Color::Cyan);
            }
        }
    }
}

/// Render the throttle warning banner (top-center) while the remote is rate-limiting us: shows
/// the reduced concurrency cap and how many repos are backing off. Overlays the panes; no-op
/// when nothing's throttled and none was seen in the last minute.
fn render_throttle_banner(frame: &mut Frame, app: &AppState, area: Rect) {
    let throttled = app.counts().7;
    if !app.throttle.recently_throttled() && throttled == 0 {
        return;
    }
    let glyph = app.icons().throttled;
    let eff = app.throttle.effective();
    let configured = app.throttle.configured();
    let message = if app.throttle.reduced() {
        format!(" {glyph} remote throttling — concurrency {eff}↓{configured} · retrying {throttled} ")
    } else {
        format!(" {glyph} remote throttling detected · {throttled} repo(s) backing off ")
    };
    let content_width = UnicodeWidthStr::width(message.as_str()) as u16;
    let width = (content_width + 2).min(area.width);
    if width < 4 || area.height < 3 {
        return;
    }
    let banner_area = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y,
        width,
        height: 3,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(banner_area);
    cast_shadow(frame, banner_area);
    frame.render_widget(Clear, banner_area);
    frame.render_widget(block, banner_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            message,
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ))),
        inner,
    );
}

/// Render the transient toast (reusable, app-wide): a small rounded notice near the bottom-center
/// that auto-dismisses. Call last so it overlays everything; no-op when no toast is active.
fn render_toast(frame: &mut Frame, app: &AppState, area: Rect) {
    let Some(toast) = app.active_toast() else {
        return;
    };
    // Nothing legible fits in a sliver of a terminal — skip (and avoid a min>max clamp panic).
    if area.width < 8 || area.height < 3 {
        return;
    }
    let text = format!("  {}  ", toast.message);
    // Wide enough for the headline and every preview line (clamped to the terminal).
    let content_width = toast
        .preview
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()) + 4)
        .chain(std::iter::once(UnicodeWidthStr::width(text.as_str())))
        .max()
        .unwrap_or(0);
    let width = (content_width as u16 + 2).clamp(8, area.width);
    let height = (3 + toast.preview.len() as u16).min(area.height);
    let toast_area = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height + 3),
        width,
        height,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(toast_area);
    cast_shadow(frame, toast_area);
    frame.render_widget(Clear, toast_area);
    frame.render_widget(block, toast_area);
    let mut lines = vec![
        Line::from(Span::styled(text, Style::default().add_modifier(Modifier::BOLD))).centered(),
    ];
    let preview_width = inner.width.saturating_sub(4) as usize;
    for preview in &toast.preview {
        lines.push(Line::from(Span::styled(
            format!("  {}", truncate_str(preview, preview_width)),
            Style::default().fg(Color::DarkGray),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Render the repo-page `y` copy menu: pick what to copy — path, branch, or both.
fn render_copy_menu(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let selected = app.copy_menu.unwrap_or(0);
    let options = ["absolute path", "branch name", "both (path + branch)"];

    let pad = if app.panel_padding { 2 } else { 0 };
    let content_rows = usize::from(!app.panel_padding) + options.len() + 2;
    let width = 38u16.min(area.width.saturating_sub(2)).max(24) + pad;
    let height = (content_rows as u16 + 2 + pad).min(area.height.saturating_sub(2).max(6));
    let modal = centered_rect(width, height, area);
    let (close_line, close_click) = modal_close_button(modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Copy ")
        .title_top(close_line);
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    app.copy_menu_area = modal;
    app.copy_menu_close_click = close_click;
    app.copy_menu_click.clear();

    let mut lines: Vec<Line> = Vec::new();
    if !app.panel_padding {
        lines.push(Line::from(String::new()));
    }
    for (index, label) in options.iter().enumerate() {
        let row_y = inner.y + lines.len() as u16;
        if row_y < inner.y + inner.height {
            app.copy_menu_click.push((row_y, index));
        }
        let cursor = if index == selected { "> " } else { "  " };
        let style = if index == selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(format!("  {cursor}{label}"), style)));
    }
    lines.push(Line::from(String::new()));
    let footer_key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let footer_hint = Style::default().fg(Color::DarkGray);
    let footer_row = inner.y + lines.len() as u16;
    lines.push(build_hint_footer(
        vec![
            ("  ".to_string(), footer_hint, None),
            ("↑↓".to_string(), footer_key, None),
            (" move · ".to_string(), footer_hint, None),
            ("enter".to_string(), footer_key, Some(HintKey::Enter)),
            (" copy".to_string(), footer_hint, Some(HintKey::Enter)),
            (" · ".to_string(), footer_hint, None),
            ("esc".to_string(), footer_key, Some(HintKey::Esc)),
            (" close".to_string(), footer_hint, Some(HintKey::Esc)),
        ],
        inner.x,
        footer_row,
        &mut app.hint_click,
    ));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The base-branch picker modal: row 0 is "auto-detect" (clears any override), then every
/// candidate branch. The current override is checked; the detected fork parent is tagged. Scrolls
/// to keep the highlighted row in view when there are more candidates than fit.
fn render_base_picker(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let Some(picker) = app.base_picker.clone() else {
        return;
    };
    let pad = if app.panel_padding { 2 } else { 0 };
    let width = 56u16.min(area.width.saturating_sub(2)).max(32) + pad;
    let height = (16u16 + pad).min(area.height.saturating_sub(2).max(8));
    let modal = centered_rect(width, height, area);
    let (close_line, close_click) = modal_close_button(modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Magenta))
        .title(format!(" base for {} ", truncate_str(&picker.branch, 30)))
        .title_top(close_line);
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    app.base_picker_area = modal;
    app.base_picker_close_click = close_click;
    app.base_picker_click.clear();

    // Reserve the last two inner rows for a blank + hint line; the rest scrolls the option list.
    let list_height = inner.height.saturating_sub(2) as usize;
    let total = picker.row_count();
    let view_start = if picker.selected >= list_height {
        picker.selected - list_height + 1
    } else {
        0
    };
    let view_end = (view_start + list_height).min(total);

    let mut lines: Vec<Line> = Vec::new();
    for index in view_start..view_end {
        let row_y = inner.y + lines.len() as u16;
        if row_y < inner.y + inner.height {
            app.base_picker_click.push((row_y, index));
        }
        let cursor = if index == picker.selected { "> " } else { "  " };
        let (text, is_current) = if index == 0 {
            let label = match &picker.detected {
                Some(detected) => format!("auto-detect ({detected})"),
                None => "auto-detect".to_string(),
            };
            (label, picker.current.is_none())
        } else {
            let candidate = &picker.candidates[index - 1];
            let mut label = candidate.clone();
            if picker.detected.as_deref() == Some(candidate.as_str()) {
                label.push_str("  (detected)");
            }
            (label, picker.current.as_deref() == Some(candidate.as_str()))
        };
        let check = if is_current { "✓ " } else { "  " };
        let style = if index == picker.selected {
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(format!("  {cursor}{check}{}", truncate_str(&text, 44)), style)));
    }
    lines.push(Line::from(String::new()));
    let footer_key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let footer_hint = Style::default().fg(Color::DarkGray);
    let footer_row = inner.y + lines.len() as u16;
    lines.push(build_hint_footer(
        vec![
            ("  ".to_string(), footer_hint, None),
            ("↑↓".to_string(), footer_key, None),
            (" move · ".to_string(), footer_hint, None),
            ("enter".to_string(), footer_key, Some(HintKey::Enter)),
            (" set".to_string(), footer_hint, Some(HintKey::Enter)),
            (" · ".to_string(), footer_hint, None),
            ("esc".to_string(), footer_key, Some(HintKey::Esc)),
            (" close".to_string(), footer_hint, Some(HintKey::Esc)),
        ],
        inner.x,
        footer_row,
        &mut app.hint_click,
    ));
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_cell_text_is_tri_state() {
        assert_eq!(count_cell_text("⎇", None), ("…".to_string(), true));
        assert_eq!(count_cell_text("⎇", Some(0)), ("⎇0".to_string(), true));
        assert_eq!(count_cell_text("⎇", Some(3)), ("⎇3".to_string(), false));
    }

    #[test]
    fn help_search_matches_keys_and_descriptions() {
        // The key column is the leading 18 cells; the rest is the description.
        let items: Vec<(Line<'static>, Option<String>)> = vec![
            (Line::from("Basics"), None), // a section header (no 'c'/'r' to avoid cross-hits)
            (Line::from("    r / R          retry selected / all"), None),
            (Line::from("    z              start claude in the editor"), None),
            (Line::from(""), None), // a blank
        ];
        // Plain search matches description text.
        assert_eq!(filter_help_items(&items, "claude", false).len(), 1);
        // Plain hotkeys-mode search matches the full row (key + description).
        assert_eq!(filter_help_items(&items, "retry", true).len(), 1);
        // `@` (hotkeys mode) restricts the match to the key column: "claude"'s key is `z`, so
        // `@claude` finds nothing (claude is only in the description).
        assert!(filter_help_items(&items, "@claude", true).is_empty());
        // `@r` matches the key column of the r/R row only.
        assert_eq!(filter_help_items(&items, "@r", true).len(), 1);
        // Blanks never survive a filter.
        assert!(filter_help_items(&items, "", false).iter().all(|(line, _)| {
            !line.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim().is_empty()
        }));
    }

    #[test]
    fn count_cell_hidden_for_emoji_or_hide_zero_setting() {
        // Emoji mode OR the hide-zero setting hides a zero count; everything else stays visible.
        assert!(count_cell_hidden(true, false, Some(0))); // emoji + zero
        assert!(count_cell_hidden(false, true, Some(0))); // unicode + hide-zero setting
        assert!(!count_cell_hidden(false, false, Some(0))); // unicode default keeps the dim 0
        assert!(!count_cell_hidden(true, true, Some(2))); // non-zero always shows
        assert!(!count_cell_hidden(true, true, None)); // loading "…" still shows
    }

    #[test]
    fn truncate_left_keeps_the_tail() {
        assert_eq!(truncate_left("short.rs", 20), "short.rs");
        // Keeps the filename end with a leading ellipsis when it overflows.
        let long = "src/features/CalendarStats/context/unassignedStatsProvider.test.tsx";
        let out = truncate_left(long, 20);
        assert!(out.starts_with('…'));
        assert!(out.ends_with("test.tsx"));
        assert!(UnicodeWidthStr::width(out.as_str()) <= 20);
    }

    #[test]
    fn diff_modal_footer_depends_on_focus_and_source() {
        // Flatten the footer's segment texts so the content assertions read naturally.
        let joined = |source: &DiffSource, focus: DiffFocus, chips: bool| -> String {
            diff_modal_footer(source, focus, chips).iter().map(|(text, _, _)| text.as_str()).collect()
        };
        let stash = DiffSource::Stash { path: "/tmp".into(), index: 0, label: "x".into() };
        let files = joined(&stash, DiffFocus::Files, false);
        assert!(files.contains("tab → diff"));
        assert!(files.contains("⇧PgUp/PgDn page"));
        assert!(files.contains("d drop"));
        let diff = joined(&stash, DiffFocus::Diff, false);
        assert!(diff.contains("tab → files"));
        assert!(diff.contains("g/G top/end"));
        // A read-only branch diff has no verb; chips add `f filter` when active.
        let branch = DiffSource::Branch { path: "/tmp".into(), name: "b".into() };
        let plain = joined(&branch, DiffFocus::Files, false);
        assert!(!plain.contains(" drop") && !plain.contains(" discard"));
        assert!(!plain.contains("f filter"));
        assert!(joined(&branch, DiffFocus::Files, true).contains("f filter"));
    }
}
