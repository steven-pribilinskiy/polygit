
use std::time::Instant;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
};
use ratatui::Frame;
use tuilith::scroll;
use unicode_width::UnicodeWidthStr;

use crate::app::{
    AppState, BranchExistenceMode, ClickRegion, Column, ColumnFlags, Command, DiffFocus, DiffMode,
    DiffSource, DiffView, DropdownKind, FilterKind, HelpTab, HintClick, HintKey, IconSet,
    InfoAction, Leader, ListRow, PageRow, PageRowKind, Pane, RepoPageSort, RepoState, RepoStatus,
    ResultDiffView, RightView, ScrollHit, ScrollKind, SortColumn, SortDir, SplitterMode,
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

/// A window-control button = a cyan `keycap` (the key that triggers it, e.g. `m` / `esc`) + a dim
/// window-control `glyph` (`▢`/`▣`/`✕` in Unicode mode, emoji in emoji mode), right-aligned ending
/// at `right_end` (exclusive) on `row`. Returns the two spans, the button's `(row, start, end)`
/// click region (measured by display width, so a 2-cell emoji glyph hit-tests correctly), and the
/// column just left of the button (1-col gap) for chips a caller right-aligns to its left.
fn window_button(
    keycap: &str,
    glyph: &str,
    row: u16,
    right_end: u16,
) -> ([Span<'static>; 2], (u16, u16, u16), u16) {
    let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    // `icon_cols`, not `UnicodeWidthStr`: `✕` and `❌` are one column and two,
    // and the first of them inks past its cell, so it carries a pad the region
    // has to span or the button's right edge stops short of its own glyph.
    let width = UnicodeWidthStr::width(keycap) as u16 + icon_cols(glyph);
    let start = right_end.saturating_sub(width);
    (
        [Span::styled(keycap.to_string(), key), icon_span(glyph, dim)],
        (row, start, right_end),
        start.saturating_sub(1),
    )
}

/// The maximize/restore button (`m`+`▢`/`▣`, or the emoji equivalents) for `pane`, registered into
/// `max_click` so the universal hit-test + hover wiring handle it. Returns the spans + the column
/// just left of it. Every pane gets one, so maximize has a consistent click affordance + `m` key.
///
/// The hotspot is padded a column wider than the glyphs themselves: a trailing blank column is
/// appended after the icon, and the click region also swallows the blank/separator column the
/// caller already renders just before `m` — so clicking either side of the icon, not just the
/// glyphs, toggles maximize.
fn max_button_spans(
    app: &mut AppState,
    pane: Pane,
    row: u16,
    right_end: u16,
) -> ([Span<'static>; 3], u16) {
    let icons = app.icons();
    let glyph = if app.maximized == Some(pane) { icons.restore } else { icons.maximize };
    let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    // The emoji `🗖`/`🗗` are the case this has to ask about rather than assume:
    // they are single codepoints, which is what the icon-set rule tests, but
    // their East_Asian_Width is N — `unicode-width` reports 1 while they ink
    // 1.71 cells. `icon_cols` gives them the cell they need; the `+ 1` after it
    // is still the button's own trailing pad, which is a different thing.
    let width = UnicodeWidthStr::width("m") as u16 + icon_cols(glyph) + 1;
    let start = right_end.saturating_sub(width);
    let left = start.saturating_sub(1);
    app.max_click.push((row, left, right_end, pane));
    ([Span::styled("m", key), icon_span(glyph, dim), Span::raw(" ")], left)
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
    // `allow` restricts which scrollbars can match: a modal passes only its own kind(s) so a pane
    // scrollbar still sitting in `scroll_hits` (registered earlier this frame, now behind the modal)
    // can't light up through it. `None` = any (the no-modal case).
    let scrollbar_col_hit = |allow: Option<&[crate::app::ScrollKind]>| -> Option<Rect> {
        app.scroll_hits.iter().find_map(|hit| {
            if allow.is_some_and(|kinds| !kinds.contains(&hit.kind)) {
                return None;
            }
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
    // The perf overlay is NOT handled here. It draws after this pass and `Clear`s its own cells,
    // which would reset anything applied to them — so it paints its own hover in
    // `render_perf_overlay`. A branch here would be a second source of truth that does nothing.
    // An open header dropdown floats above every pane, so its rows win the hover first — the item
    // under the cursor (and the `[x]` close button) get the standard soft button tint.
    if app.dropdown.is_some() {
        if let Some(&(row, start, end, _)) =
            app.dropdown_item_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.dropdown_action_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.dropdown_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        }
    }
    // Footer status-bar commands stay clickable over any modal (only settings/help/quit keep a
    // region there). Check them first, everywhere — so the live footer reacts to hover even with a
    // modal on top, where the per-modal branches below only inspect that modal's own regions.
    else if let Some(region) = app.clickable.iter().find(|c| contains(c.row, c.col_start, c.col_end)) {
        for sibling in app.clickable.iter().filter(|c| c.command == region.command) {
            button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
        }
    } else if app.confirm.is_some() {
        if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            // The yes/no chips: light up the key and its label together (siblings by key).
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) =
            app.confirm_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        }
    } else if app.show_settings {
        if let Some((row, start, end)) =
            app.settings_release_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, ..)) =
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
    } else if app.show_keybindings {
        // Buttons first (they sit on top of the row), then the row body, the close button, the
        // footer chips, and finally the scrollbar column.
        if let Some(&(row, start, end, _)) =
            app.keybindings_set_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.keybindings_clear_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.keybindings_row_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.keybindings_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some(rect) = scrollbar_col_hit(Some(&[crate::app::ScrollKind::Keybindings])) {
            hits.push(rect);
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
            app.help_remap_click.filter(|&(r, s, e)| contains(r, s, e))
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
        } else if let Some(&(row, start, end, section)) =
            app.help_design_tab_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // The active section tab keeps its solid highlight (no extra hover tint).
            if section != app.design_section {
                button_hits.push(row_rect(row, start, end));
            }
        } else if let Some((row, start, end, _)) =
            app.cli_helpmode_click.iter().find(|&&(r, s, e, _)| contains(r, s, e)).copied()
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(flag) =
            app.cli_command_click.iter().find(|&&(row, _)| row == hrow).map(|&(_, idx)| idx)
        {
            // Hovering a built-command token tints it AND the matching flag row above (so you can
            // see which flag a click would remove).
            hits.push(inner_row(app.help_area));
            if let Some(&(flag_row, _)) = app.cli_flag_click.iter().find(|&&(_, idx)| idx == flag) {
                hits.push(Rect {
                    x: app.help_area.x + 1,
                    y: flag_row,
                    width: app.help_area.width.saturating_sub(2),
                    height: 1,
                });
            }
        } else if let Some((row, start, end)) =
            app.help_preview_click.filter(|&(r, s, e)| contains(r, s, e))
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
            scrollbar_col_hit(Some(&[crate::app::ScrollKind::DiffFiles, crate::app::ScrollKind::DiffBody]))
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
    } else if app.kebab.is_some() {
        // Kebab menu: its close button + hint chips, and a row tint on the hovered (enabled) item.
        if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) =
            app.kebab_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if app.kebab_click.iter().any(|&(row, _)| row == hrow) {
            hits.push(inner_row(app.kebab_area));
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
    } else if app.branch_picker.is_some() {
        if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) =
            app.branch_picker_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if app.branch_picker_click.iter().any(|&(row, _)| row == hrow) {
            hits.push(inner_row(app.branch_picker_area));
        }
    } else if app.branch_filter_modal.is_some() {
        if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) =
            app.branch_filter_modal_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.branch_filter_mode_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if app.branch_filter_rows_click.iter().any(|&(row, _)| row == hrow) {
            hits.push(inner_row(app.branch_filter_modal_area));
        } else if let Some(scroll) = scrollbar_col_hit(Some(&[crate::app::ScrollKind::BranchFilter])) {
            hits.push(scroll);
        }
    } else if app.coverage_modal.is_some() {
        // Hint → close → tabs → rows, the same order every modal uses, and only this panel's own
        // scrollbar kind so a pane behind it cannot light through.
        if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) =
            app.coverage_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.coverage_tab_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.coverage_check_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if app.coverage_rows_click.iter().any(|&(row, _)| row == hrow) {
            hits.push(inner_row(app.coverage_area));
        } else if let Some(scroll) = scrollbar_col_hit(Some(&[crate::app::ScrollKind::Coverage])) {
            hits.push(scroll);
        }
    } else if app.show_build_info {
        if let Some((row, start, end)) =
            app.build_info_check_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.build_info_install_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.build_info_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.build_info_fold_all_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.build_info_unfold_all_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if app.build_info_tree_click.iter().any(|&(r, s, e, _)| contains(r, s, e)) {
            // A container row — tint the whole row width (it toggles on click).
            hits.push(inner_row(app.build_info_area));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        }
    } else if app.show_changelog {
        // Pin picker: the `[pin]` buttons and release-header rows (accordion) get the button tint.
        if let Some(&(row, start, end, _)) =
            app.pin_row_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, vis)) =
            app.pin_header_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // The selected (expanded) release keeps its solid highlight.
            if vis != app.pin_selected {
                button_hits.push(row_rect(row, start, end));
            }
        } else if let Some(&(row, start, end, idx)) =
            app.changelog_header_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // The selected header keeps its solid highlight (no extra hover tint).
            if idx != app.changelog_selected {
                button_hits.push(row_rect(row, start, end));
            }
        } else if let Some((row, start, end)) =
            app.changelog_maximize_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) =
            app.changelog_close_click.filter(|&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        }
    } else if app.pr_modal.is_some() {
        // PR viewer: only its own controls hover (close, hint chips, scrollbar, the collapsible
        // section headers, the collapse-all + search rows) — never the panes behind it.
        if let Some((row, start, end)) = app.pr_modal_close_click.filter(|&(r, s, e)| contains(r, s, e)) {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some(scroll) = scrollbar_col_hit(Some(&[crate::app::ScrollKind::PrModal])) {
            hits.push(scroll);
        } else if let Some(&(row, start, end)) =
            app.pr_collapse_all_click.as_ref().filter(|&&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end)) =
            app.pr_search_click.as_ref().filter(|&&(r, s, e)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.pr_modal_tab_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(&(row, start, end, _)) =
            app.pr_files_view_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end, _)) =
            app.pr_checks_click.iter().find(|&(r, s, e, _)| contains(*r, *s, *e))
        {
            button_hits.push(row_rect(*row, *start, *end));
        } else if let Some(&(row, start, end, _)) =
            app.pr_section_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            button_hits.push(row_rect(row, start, end));
        }
    } else if let Some(explorer) = app.explorer.as_ref() {
        // File explorer: only its own controls hover (close, hint chips, the two scrollbars, and the
        // file rows) — never the list/info/result panes behind it.
        if let Some((row, start, end)) = explorer.close_click.filter(|&(r, s, e)| contains(r, s, e)) {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) = explorer.pin_click.filter(|&(r, s, e)| contains(r, s, e)) {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, col)) = explorer.resize_click.filter(|&(r, c)| contains(r, c, c + 1)) {
            button_hits.push(row_rect(row, col, col + 1));
        } else if explorer.mode == crate::explorer::SurfaceMode::Floating
            && contains(
                explorer.titlebar_drag_area.y,
                explorer.titlebar_drag_area.x,
                explorer.titlebar_drag_area.x + explorer.titlebar_drag_area.width,
            )
        {
            hits.push(explorer.titlebar_drag_area);
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some(scroll) = scrollbar_col_hit(Some(&[
            crate::app::ScrollKind::ExplorerList,
            crate::app::ScrollKind::ExplorerPreview,
        ])) {
            hits.push(scroll);
        } else if let Some(&(row, start, end, index)) =
            explorer.rows_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // The selected row gets the deeper selection-hover tint; others the soft hover.
            if index == explorer.selected {
                strong_hits.push(row_rect(row, start, end));
            } else {
                hits.push(row_rect(row, start, end));
            }
        }
    } else {
        // No modal: hover follows the cursor across whatever panes rendered this frame (it's
        // independent of focus — so the docked repo page no longer kills the list/info/result
        // hovers). Each pane's regions are gated by whether that pane is actually visible: a
        // maximized pane hides the others, whose click vecs would otherwise hold stale geometry.
        // `max_click`, `hint_click`, and the scrollbar are cleared every frame, so they're always
        // safe to check. Regions are position-disjoint, so the first containing the cursor wins.
        let max = app.maximized;
        let repo_visible = app.repo_page.is_some() && max.is_none_or(|pane| pane == Pane::RepoPage);
        let list_visible = max.is_none_or(|pane| pane == Pane::List);
        let right_visible = max.is_none_or(|pane| matches!(pane, Pane::Info | Pane::Result));

        // Gated button regions (precomputed so the else-if chain stays flat — no let-chains).
        let repo_button = repo_visible
            .then(|| {
                app.page_cols_click
                    .filter(|&(r, s, e)| contains(r, s, e))
                    .or_else(|| app.page_sort_click.filter(|&(r, s, e)| contains(r, s, e)))
                    .or_else(|| app.repo_page_window_click.filter(|&(r, s, e)| contains(r, s, e)))
                    .or_else(|| app.repo_page_back_click.filter(|&(r, s, e)| contains(r, s, e)))
                    .or_else(|| {
                        app.repo_page_tab_click
                            .iter()
                            .find(|&&(r, s, e, _)| contains(r, s, e))
                            .map(|&(r, s, e, _)| (r, s, e))
                    })
                    .or_else(|| {
                        app.repo_page_section_click
                            .iter()
                            .find(|&&(r, s, e, _)| contains(r, s, e))
                            .map(|&(r, s, e, _)| (r, s, e))
                    })
                    .or_else(|| {
                        app.repo_page_sort_click
                            .iter()
                            .find(|&&(r, s, e, _)| contains(r, s, e))
                            .map(|&(r, s, e, _)| (r, s, e))
                    })
                    .or_else(|| {
                        app.base_cell_click
                            .iter()
                            .find(|&&(r, s, e, _)| contains(r, s, e))
                            .map(|&(r, s, e, _)| (r, s, e))
                    })
            })
            .flatten();
        let list_button = list_visible
            .then(|| {
                app.list_cols_click
                    .filter(|&(r, s, e)| contains(r, s, e))
                    .or_else(|| app.list_sort_click.filter(|&(r, s, e)| contains(r, s, e)))
                    .or_else(|| app.filter_search_click.filter(|&(r, s, e)| contains(r, s, e)))
                    .or_else(|| app.filter_search_clear_click.filter(|&(r, s, e)| contains(r, s, e)))
                    .or_else(|| {
                        app.filter_chip_click
                            .iter()
                            .find(|&&(r, s, e, _)| contains(r, s, e))
                            .map(|&(r, s, e, _)| (r, s, e))
                    })
                    .or_else(|| {
                        app.filter_chip_remove_click
                            .iter()
                            .find(|&&(r, s, e, _)| contains(r, s, e))
                            .map(|&(r, s, e, _)| (r, s, e))
                    })
                    .or_else(|| app.filter_add_click.filter(|&(r, s, e)| contains(r, s, e)))
                    .or_else(|| app.filter_reset_click.filter(|&(r, s, e)| contains(r, s, e)))
            })
            .flatten();
        let header_col = if list_visible { app.header_sort_at(hcol, hrow) } else { None };
        let info_button = if right_visible {
            app.info_click.iter().find(|&&(r, s, e, _)| contains(r, s, e)).map(|&(r, s, e, _)| (r, s, e))
        } else {
            None
        };
        let pr_hit = if list_visible {
            app.pr_cell_click.iter().find(|&&(r, s, e, _)| contains(r, s, e)).map(|&(r, s, e, _)| (r, s, e))
        } else {
            None
        };
        let kebab_hit = if list_visible {
            app.kebab_open_click.iter().find(|&&(r, s, e, _)| contains(r, s, e)).map(|&(r, s, e, _)| (r, s, e))
        } else {
            None
        };
        let repo_row = if repo_visible {
            app.repo_page_click.iter().find(|&&(row, _)| row == hrow).map(|&(_, idx)| idx)
        } else {
            None
        };
        let list_row = if list_visible { app.list_selection_at(hcol, hrow) } else { None };

        if let Some(&(row, start, end, _)) =
            app.max_click.iter().find(|&&(r, s, e, _)| contains(r, s, e))
        {
            // A pane's maximize/restore button (List/Info/Result top border).
            button_hits.push(row_rect(row, start, end));
        } else if let Some(hint) = app.hint_click.iter().find(|h| contains(h.row, h.col_start, h.col_end)) {
            for sibling in app.hint_click.iter().filter(|h| h.key == hint.key) {
                button_hits.push(row_rect(sibling.row, sibling.col_start, sibling.col_end));
            }
        } else if let Some((row, start, end)) = repo_button {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) = list_button {
            button_hits.push(row_rect(row, start, end));
        } else if let Some(column) = header_col {
            // A sortable list column header cell — highlight it across the header's rows.
            if let Some(&(start, end, _)) =
                app.header_click.iter().find(|&&(s, e, c)| c == column && hcol >= s && hcol < e)
            {
                let header = app.header_area;
                for row in header.y..header.y + header.height {
                    hits.push(row_rect(row, start, end));
                }
            }
        } else if let Some((row, start, end)) = info_button {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) = pr_hit {
            button_hits.push(row_rect(row, start, end));
        } else if let Some((row, start, end)) = kebab_hit {
            // The rightmost `⋮` kebab affordance on the hovered row.
            button_hits.push(row_rect(row, start, end));
        } else if let Some(scroll) = scrollbar_col_hit(None) {
            hits.push(scroll);
        } else if let Some(sel_index) = repo_row {
            // A selectable repo-page body row (branch / worktree / stash).
            let rect =
                Rect { x: app.repo_page_inner.x, y: hrow, width: app.repo_page_inner.width, height: 1 };
            if sel_index == app.repo_page_selected {
                strong_hits.push(rect);
            } else {
                hits.push(rect);
            }
        } else if max.is_none()
            && (i32::from(hcol) - i32::from(app.divider_col)).abs() <= 1
            && hrow >= app.main_area.y
            && hrow < app.main_area.y + app.main_area.height
        {
            hits.push(Rect { x: app.divider_col, y: app.main_area.y, width: 1, height: app.main_area.height });
        } else if let Some(idx) = list_row {
            // Any selectable list row — repo/group/folder rows plus the Result/Errors summary rows.
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

/// Glyphs the terminal draws wider than the cell it advances over.
///
/// `unicode-width` is right about the width — these step one column, and a
/// cursor-position report agrees. What it cannot see is whether the glyph FITS:
/// one the terminal's monospace font lacks is supplied by a fallback whose
/// advance is its own business, and the ink is drawn past the cell into the
/// next one. Whatever style that cell carries is the background the overflow
/// lands on, so a styled span ENDING on one of these has half its glyph painted
/// on somebody else's background — which is what a magenta `⧉` beside dim text
/// looked like, half in its own hover highlight and half out.
///
/// Measured against Cascadia Mono, which covers none of them, and the Segoe UI
/// Symbol it falls back to. The ratio is the fallback's advance as a fraction of
/// the cell:
///
/// | glyph | advance | where |
/// |---|---|---|
/// | `↗` U+2197 | 1.25 | `external` |
/// | `↯` U+21AF | 1.16 | `throttled` |
/// | `↻` U+21BB | 1.45 | `retry_log` |
/// | `⎇` U+2387 | 1.71 | the switch chip |
/// | `★` U+2605 | 1.42 | `fav_on` |
/// | `☆` U+2606 | 1.42 | `fav_off` |
/// | `⚠` U+26A0 | 1.47 | `warning` |
/// | `✕` U+2715 | 1.39 | `close` |
/// | `✗` U+2717 | 1.39 | `failed` |
/// | `⧉` U+29C9 | 1.71 | `copy` |
/// | `🏷` U+1F3F7 | 1.71 | emoji `tags` |
/// | `🗖` U+1F5D6 | 1.71 | emoji `maximize` |
/// | `🗗` U+1F5D7 | 1.71 | emoji `restore` |
///
/// The last three are the trap the icon-set rule does not catch: they are single
/// codepoints, which is what that rule tests, but their East_Asian_Width is **N**
/// — so `unicode-width` reports 1 for them, not the 2 an emoji is assumed to be.
/// `📋`, `🔗` and `❌` are EAW=W and genuinely two cells; they are fine.
///
/// Sorted by codepoint; `FIRST_OVERFLOWING` is the scan guard.
const DRAWN_PAST_THEIR_CELL: &[char] = &[
    '\u{2197}', '\u{21af}', '\u{21bb}', '\u{2387}', '\u{2605}', '\u{2606}', '\u{26a0}',
    '\u{2715}', '\u{2717}', '\u{29c9}', '\u{1f3f7}', '\u{1f5d6}', '\u{1f5d7}',
];

/// Nothing below this is in the table, so a scan can stop asking.
const FIRST_OVERFLOWING: char = '\u{2197}';

/// Whether `glyph`'s ink is drawn past the cell it advances over.
pub(crate) fn draws_past_its_cell(glyph: &str) -> bool {
    glyph.chars().next_back().is_some_and(|last| {
        last >= FIRST_OVERFLOWING && DRAWN_PAST_THEIR_CELL.binary_search(&last).is_ok()
    })
}

/// Columns a glyph needs so its ink stays on its own background.
///
/// Its display width, plus a cell for the overflow when it has one. Every click
/// region, hover rect and column budget around an icon asks this rather than
/// `UnicodeWidthStr::width`, or the highlight stops short of the ink.
pub(crate) fn icon_cols(glyph: &str) -> u16 {
    let width = UnicodeWidthStr::width(glyph) as u16;
    width + u16::from(draws_past_its_cell(glyph))
}

/// A styled span whose ink stays inside it.
///
/// The pad is a space in the SAME span, never a wider measurement: the glyph
/// really is one column, and claiming two would put every later region on the
/// row a column right of where it is drawn.
pub(crate) fn icon_span(glyph: &str, style: Style) -> Span<'static> {
    let text = if draws_past_its_cell(glyph) {
        format!("{glyph} ")
    } else {
        glyph.to_string()
    };
    Span::styled(text, style)
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
        // `↯` and `✗` both ink past their cell, and the span after this one is
        // the row's own padding at the default style — so without the pad the
        // overflow lands on it rather than on the status colour.
        RepoStatus::Throttled => {
            icon_span(icons.throttled, Style::default().fg(Color::Magenta))
        }
        RepoStatus::Failed => icon_span(icons.failed, Style::default().fg(Color::Red)),
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

/// Parse inline markdown — `**bold**`, `*italic*`, and `` `code` `` — over `base`, returning styled
/// runs with the markers stripped. Code spans get a distinct color; bold/italic add their modifier
/// to `base`. A lone/unmatched marker renders literally. Shared by the changelog/PR prose, the help
/// modal, and tooltips. Authored content only — keymap.json actions stay marker-free (the web docs
/// render them verbatim).
pub(crate) fn inline_md_runs(text: &str, base: Style) -> Vec<(String, Style)> {
    let code_style = Style::default().fg(Color::Yellow);
    let style_now = |bold: bool, italic: bool, code: bool| {
        if code {
            return code_style;
        }
        let mut style = base;
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        style
    };
    let chars: Vec<char> = text.chars().collect();
    let mut runs: Vec<(String, Style)> = Vec::new();
    let mut buf = String::new();
    let (mut bold, mut italic, mut code) = (false, false, false);
    let mut idx = 0;
    while idx < chars.len() {
        if !code && chars[idx] == '*' && chars.get(idx + 1) == Some(&'*') {
            if !buf.is_empty() {
                runs.push((std::mem::take(&mut buf), style_now(bold, italic, code)));
            }
            bold = !bold;
            idx += 2;
            continue;
        }
        if !code && chars[idx] == '*' {
            if !buf.is_empty() {
                runs.push((std::mem::take(&mut buf), style_now(bold, italic, code)));
            }
            italic = !italic;
            idx += 1;
            continue;
        }
        if chars[idx] == '`' {
            if !buf.is_empty() {
                runs.push((std::mem::take(&mut buf), style_now(bold, italic, code)));
            }
            code = !code;
            idx += 1;
            continue;
        }
        buf.push(chars[idx]);
        idx += 1;
    }
    if !buf.is_empty() {
        runs.push((buf, style_now(bold, italic, code)));
    }
    runs
}

/// [`inline_md_runs`] as owned `Span`s — the common case for building a `Line`.
pub(crate) fn inline_md_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    inline_md_runs(text, base).into_iter().map(|(run, style)| Span::styled(run, style)).collect()
}

/// The visible width of `text` with markdown markers stripped (for layout/centering).
pub(crate) fn md_display_width(text: &str) -> usize {
    inline_md_runs(text, Style::default()).iter().map(|(run, _)| UnicodeWidthStr::width(run.as_str())).sum()
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

/// The live perf overlay (`Ctrl+T`). Drawn last — after the palette pass — so it uses the
/// palette's RGB values directly rather than semantic ANSI, which would no longer be remapped.
///
/// Deliberately compact and top-right anchored: it has to be readable WHILE the mouse is moving
/// over the list, so it must not sit under the cursor or cover the rows being hovered.
fn render_perf_overlay(frame: &mut Frame, app: &mut AppState) {
    app.perf_close_click = None;
    app.perf_menu_click = None;
    app.perf_drag_area = Rect::default();
    app.perf_panel_rect = Rect::default();
    if !app.perf.overlay {
        return;
    }
    let area = frame.area();
    let palette = app.palette();
    let now = Instant::now();

    // `ms` renders a microsecond channel reading; a blank cell reads better than "0.00m" for a
    // channel that has never been sampled, so an unsampled row is honestly empty.
    let ms = |value: f64, sampled: bool| -> String {
        if !sampled {
            "   –".to_string()
        } else if value >= 1000.0 {
            format!("{:>6.1}", value / 1000.0)
        } else {
            format!("{:>6.2}", value / 1000.0)
        }
    };

    let perf = &mut app.perf;
    let motion_per_sec = perf.motion_rate.per_sec(now);
    let frames_per_sec = perf.frame_rate.per_sec(now);
    let events_per_sec = perf.event_rate.per_sec(now);
    let verdict = perf.verdict();

    // (label, value, colour, tier) — tier 0 is core, 1 is detail, 2 is a rate row.
    let mut rows: Vec<(String, String, Color, u8)> = Vec::new();
    // Hover lag leads: it is the symptom the user actually reports, and its color is the alarm.
    let lag_p95 = perf.lag.p95();
    let lag_color = if perf.lag.is_empty() {
        palette.faint
    } else if lag_p95 > 100_000.0 {
        palette.error
    } else if lag_p95 > 33_000.0 {
        palette.warn
    } else {
        palette.ok
    };
    rows.push((
        "hover lag".into(),
        format!("{} {}", ms(lag_p95, !perf.lag.is_empty()), ms(perf.lag.p50(), !perf.lag.is_empty())),
        lag_color,
        0,
    ));
    rows.push((
        "  build".into(),
        format!("{} {}", ms(perf.build.p95(), !perf.build.is_empty()), ms(perf.build.p50(), !perf.build.is_empty())),
        palette.fg,
        0,
    ));
    rows.push((
        "  flush".into(),
        format!("{} {}", ms(perf.flush.p95(), !perf.flush.is_empty()), ms(perf.flush.p50(), !perf.flush.is_empty())),
        palette.fg,
        0,
    ));
    rows.push((
        "  upkeep".into(),
        format!("{} {}", ms(perf.upkeep.p95(), !perf.upkeep.is_empty()), ms(perf.upkeep.p50(), !perf.upkeep.is_empty())),
        palette.muted,
        1,
    ));
    rows.push((
        "  lock".into(),
        format!("{} {}", ms(perf.lock_wait.p95(), !perf.lock_wait.is_empty()), ms(perf.lock_wait.p50(), !perf.lock_wait.is_empty())),
        palette.muted,
        1,
    ));
    // What this panel costs to draw. Shown because it is subtracted from `flush` — a reader who
    // cannot see the correction has to take it on trust.
    rows.push((
        "overlay".into(),
        format!("{} {}", ms(perf.overlay_cost.p95(), !perf.overlay_cost.is_empty()), ms(perf.overlay_cost.p50(), !perf.overlay_cost.is_empty())),
        palette.faint,
        1,
    ));

    // A backlog is the tell that the loop is structurally behind, so it gets the same alarm colors
    // as the lag rather than being buried with the rates.
    let backlog_p95 = perf.backlog.p95();
    let backlog_color = if perf.backlog.is_empty() {
        palette.faint
    } else if backlog_p95 >= 8.0 {
        palette.error
    } else if backlog_p95 >= 2.0 {
        palette.warn
    } else {
        palette.ok
    };
    rows.push((
        "backlog".into(),
        format!("{:>6.0} {:>6.0}", backlog_p95, perf.backlog.p50()),
        backlog_color,
        0,
    ));
    rows.push((
        "dropped".into(),
        format!("{:>6.0} {:>6.0}", perf.coalesced.p95(), perf.coalesced.p50()),
        palette.muted,
        1,
    ));
    rows.push((
        "motion/s".into(),
        format!("{motion_per_sec:>6} /s{:>4}", ""),
        palette.fg,
        2,
    ));
    rows.push((
        "event/s".into(),
        format!("{events_per_sec:>6} /s{:>4}", ""),
        palette.muted,
        2,
    ));
    rows.push((
        "frame/s".into(),
        format!("{frames_per_sec:>6} /s{:>4}", ""),
        palette.fg,
        2,
    ));
    if let Some(rtt) = perf.terminal_rtt {
        rows.push((
            "term rtt".into(),
            format!("{:>6.2} ms{:>4}", rtt.as_secs_f64() * 1e3, ""),
            palette.muted,
            2,
        ));
    }

    // Width is driven by the widest row plus the verdict, so a long verdict never truncates into
    // uselessness — the verdict IS the deliverable of this overlay.
    let label_width = 9;
    let value_width = 13;
    let body_width = label_width + 1 + value_width;
    let verdict_lines = wrap_plain(verdict, body_width);
    let inner_width = body_width;
    let width = (inner_width + 2) as u16;

    // The verdict's height is RESERVED at the longest one, not taken from the current text. The
    // string changes as the numbers move ("hover is keeping up" is one line; the backlog verdict is
    // four), so sizing to the live text makes the panel grow and shrink under the reader's eyes
    // while they are trying to read it.
    let verdict_rows = crate::perf::Perf::max_verdict_rows(body_width) as u16;

    let graph_rows = app.perf.graph.rows;
    // Count the tiers that actually exist this frame — `term rtt` is only present when the
    // terminal answered the probe, and planning for a row that is not there leaves a blank one.
    let detail_rows = rows.iter().filter(|(_, _, _, tier)| *tier == 1).count() as u16;
    let rate_rows = rows.iter().filter(|(_, _, _, tier)| *tier == 2).count() as u16;
    let Some(plan) =
        crate::perf::plan_panel(area.height, verdict_rows, graph_rows, detail_rows, rate_rows)
    else {
        return;
    };
    if area.width < width + 2 {
        return;
    }
    let height = plan.height;
    // Resolved from the placement every frame, never stored as a rect. The panel's height changes
    // as the verdict rewraps, and `resolve` clamps for this frame without writing anything back —
    // so a terminal that shrinks and grows again puts the panel back where the user left it.
    let rect = app.perf.placement.placement().resolve(area, (width, height));
    frame.render_widget(Clear, rect);

    let mut lines: Vec<Line> = Vec::new();
    let mut perf_tips: Vec<(u16, String)> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("{:<label_width$}", "channel"), Style::default().fg(palette.faint)),
        Span::styled(format!(" {:>6} {:>6}", "p95", "p50"), Style::default().fg(palette.faint)),
    ]));
    // Row index within the table, for turning a row into the screen line it will occupy. The header
    // takes the first line, so the first metric row is at offset 1.
    let mut drawn = 1_u16;
    let popover_side = perf_popover_side(rect, area);
    for (label, value, color, tier) in rows {
        let keep = match tier {
            1 => plan.detail,
            2 => plan.rates,
            _ => true,
        };
        if !keep {
            continue;
        }
        // A popover per metric row, explaining what the abbreviation measures and what a bad
        // reading indicts. Registered against the row's real screen position — the panel moves, so
        // a computed offset would be right only at the corner it was written for.
        if let Some((title, body)) = crate::perf::channel_help(&label) {
            let mut text = format!("**{title}**\n{body}");
            // Percentiles for the rows that are channels. A rate row has none, and inventing a
            // line of zeros for it would read as a measurement.
            if let Some(channel) = app.perf.channel_by_label(&label)
                && !channel.is_empty()
            {
                text.push_str(&format!(
                    "\np50 {:.2}ms · p95 {:.2}ms · p99 {:.2}ms\nworst {:.2}ms · peak {:.2}ms · n {}",
                    channel.p50() / 1000.0,
                    channel.p95() / 1000.0,
                    channel.p99() / 1000.0,
                    channel.window_max() / 1000.0,
                    channel.peak / 1000.0,
                    channel.count,
                ));
            }
            perf_tips.push((rect.y + 1 + drawn, text));
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{label:<label_width$}"), Style::default().fg(palette.muted)),
            Span::styled(value, Style::default().fg(color)),
        ]));
        drawn += 1;
    }
    lines.push(Line::from(Span::styled(
        "─".repeat(inner_width),
        Style::default().fg(palette.faint),
    )));
    // The verdict is drawn as its own block at the bottom, below the graph — see the layout split.
    let verdict_block: Vec<Line> = verdict_lines
        .into_iter()
        .map(|line| {
            Line::from(Span::styled(
                line,
                Style::default().fg(palette.accent).add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.accent))
        .style(Style::default().bg(palette.base_bg))
        .title(Span::styled(" perf ^T ", Style::default().fg(palette.accent)));
    let inner = block.inner(rect);
    frame.render_widget(&block, rect);

    // Top to bottom: the table, then the graph, then the verdict. The verdict sits LAST because its
    // height is reserved at the tallest of the six possible strings — put anywhere else, the unused
    // rows read as a hole in the middle of the panel; at the bottom they are simply the edge.
    // The verdict's height is reserved at the tallest string, so a short verdict leaves rows over.
    // Give them to the graph rather than leaving them blank: a sparkline that gains a row of
    // resolution when the verdict shortens is a far quieter change than a panel that resizes.
    let verdict_used = verdict_block.len();
    let slack = verdict_rows.saturating_sub(verdict_block.len() as u16);
    // One spare row is kept back from the graph for the controls hint; the rest of the verdict's
    // unused height still goes to the graph rather than sitting blank.
    let hint_row = u16::from(slack > 0);
    let graph_height =
        if plan.graph { plan.graph_rows + 1 + slack.saturating_sub(hint_row) } else { 0 };
    let verdict_height =
        if plan.graph { verdict_rows - slack.saturating_sub(hint_row) } else { verdict_rows };
    let table_height =
        inner.height.saturating_sub(graph_height).saturating_sub(verdict_height);
    let table_area = Rect { height: table_height, ..inner };
    frame.render_widget(Paragraph::new(lines), table_area);

    if plan.graph && graph_height > 0 {
        let graph_area = Rect {
            x: inner.x,
            y: inner.y + table_height,
            width: inner.width,
            height: graph_height,
        };
        render_perf_graph(frame, app, graph_area, &palette);
    }

    let verdict_area = Rect {
        x: inner.x,
        y: inner.y + table_height + graph_height,
        width: inner.width,
        height: verdict_height,
    };
    frame.render_widget(Paragraph::new(verdict_block), verdict_area);

    // The panel advertises its own controls, footer-hint style — the keys exist whether or not
    // anyone opens the help modal, and a draggable surface with no visible affordance is one nobody
    // discovers. Only drawn when there is a spare row, since the verdict outranks it.
    if verdict_height > u16::try_from(verdict_used).unwrap_or(u16::MAX) {
        let hint = clip_to_width("drag title · alt+↔ move", usize::from(inner.width));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(palette.faint)))),
            Rect { y: verdict_area.y + verdict_area.height.saturating_sub(1), height: 1, ..verdict_area },
        );
    }

    // The `[x]` closes the overlay by mouse, mirroring `Ctrl+T`. Captured, not recomputed, so it
    // stays correct if the box moves or resizes.
    let (close_line, close_region) = perf_title_buttons(rect);
    frame.render_widget(
        Paragraph::new(close_line),
        Rect { x: rect.x, y: rect.y, width: rect.width, height: 1 },
    );
    let (close_region, menu_region) = close_region;
    app.perf_close_click = close_region;
    app.perf_menu_click = menu_region;
    // The title row left of the buttons drags the panel. Captured, not recomputed — it moves with
    // the panel, and a hardcoded offset would be right only at the corner it was written for.
    let buttons_start = menu_region.map_or(rect.x + rect.width, |(_, start, _)| start);
    app.perf_drag_area = Rect {
        x: rect.x + 1,
        y: rect.y,
        width: buttons_start.saturating_sub(rect.x + 1),
        height: 1,
    };
    // The whole panel is registered, not just the button: every mouse event inside it belongs to
    // the panel, and without this a click in the middle of it selects the repo row behind it.
    app.perf_panel_rect = rect;

    // Per-row popovers, opening AWAY from the panel. `hover_tooltips` is cleared at the top of the
    // widget pass and this runs later in the same frame, so appending here is safe; the dwell loop
    // reads the previous frame's registry, exactly as every column header already does.
    for (row, text) in perf_tips {
        app.hover_tooltips.push(crate::app::TooltipRegion {
            row,
            col_start: rect.x + 1,
            col_end: rect.x + rect.width.saturating_sub(1),
            text,
            anchor: Rect { x: rect.x, y: row, width: rect.width, height: 1 },
            placement: tui_pick::Placement::new(popover_side, tui_pick::Align::Start),
            hide_column: None,
            area: crate::app::TooltipArea::Perf,
        });
    }

    // The panel paints its OWN hover. `apply_hover` runs earlier and this function then draws
    // `Clear` over the same cells, which resets them — so a highlight applied there is computed
    // and then wiped, leaving a button that is clickable and dead on hover. Doing it here also
    // means the highlight cannot lag a frame behind a panel that has just moved.
    if let Some((hover_col, hover_row)) = app.hover {
        let over = |region: Option<(u16, u16, u16)>| {
            region.filter(|&(row, start, end)| {
                hover_row == row && hover_col >= start && hover_col < end
            })
        };
        let tint = |frame: &mut Frame, row: u16, start: u16, end: u16| {
            frame.buffer_mut().set_style(
                Rect { x: start, y: row, width: end.saturating_sub(start), height: 1 },
                Style::default().bg(palette.hover_bg()),
            );
        };
        if let Some((row, start, end)) = over(close_region) {
            tint(frame, row, start, end);
        } else if let Some((row, start, end)) = over(menu_region) {
            tint(frame, row, start, end);
        } else if crate::app::point_in(app.perf_drag_area, hover_col, hover_row) {
            // A softer tint on the title strip: it is a draggable surface, not a button.
            tint(frame, app.perf_drag_area.y, app.perf_drag_area.x, app.perf_drag_area.right());
        }
    }
}

/// Which side of the panel its popovers open toward: whichever half of the screen it is NOT in.
///
/// The panel draws LAST — deliberately, so its own cost stays out of the channels it reports — and
/// the tooltip draws in the widget pass before it. So a popover that overlaps the panel is painted
/// over by it. Flipping is disabled at the call site for the same reason: flipping is the one thing
/// that could throw it back across the panel.
fn perf_popover_side(panel: Rect, viewport: Rect) -> tui_pick::Side {
    let panel_centre = panel.x.saturating_add(panel.width / 2);
    if panel_centre >= viewport.x + viewport.width / 2 {
        tui_pick::Side::Left
    } else {
        tui_pick::Side::Right
    }
}

/// The panel's title-bar button regions: `(close, menu)`.
type PerfTitleRegions = (BtnRegion, BtnRegion);

/// The panel's title-bar controls: `[menu]` opens its menu, `[x]` closes it. Returns the line and
/// its two click regions.
fn perf_title_buttons(rect: Rect) -> (Line<'static>, PerfTitleRegions) {
    let close = "[x]";
    let menu = "[menu]";
    let dim = Style::default().fg(Color::DarkGray);
    let line = Line::from(vec![
        Span::styled(menu, dim),
        Span::raw(" "),
        Span::styled(close, dim.add_modifier(Modifier::BOLD)),
    ])
    .right_aligned();
    let col_end = rect.x + rect.width.saturating_sub(1);
    let close_start = col_end.saturating_sub(close.len() as u16);
    let menu_end = close_start.saturating_sub(1);
    let menu_start = menu_end.saturating_sub(menu.len() as u16);
    (line, (Some((rect.y, close_start, col_end)), Some((rect.y, menu_start, menu_end))))
}

/// The history graph: a caption naming the metric and its scale, then a sparkline of the window.
///
/// Drawn after the palette pass like the rest of the panel, so every style carries explicit RGB
/// from the palette — a default-styled sparkline would inherit the post-`Clear` reset and render
/// unthemed.
fn render_perf_graph(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    palette: &crate::theme::Palette,
) {
    use ratatui::widgets::{RenderDirection, Sparkline};

    let graph = app.perf.graph;
    let seconds = graph.seconds();
    let columns = usize::from(area.width);
    let data = app.perf.series.window(graph.metric, seconds, columns);
    let range = app.perf.series.range(graph.metric, seconds);

    // The caption is not decoration. A sparkline auto-normalises, so a flat line at 20 fps and a
    // flat line at 120 fps are the same picture — without the scale the graph says nothing.
    let scale = match range {
        Some((low, high)) if graph.metric.is_duration() => {
            format!("{:.1}–{:.1}{}", low / 1000.0, high / 1000.0, graph.metric.unit())
        }
        Some((low, high)) => format!("{low:.0}–{high:.0}{}", graph.metric.unit()),
        None => "no data yet".to_string(),
    };
    let perturbed = app.perf.series.window_perturbed(seconds);
    let caption = format!("{} {} · {}", graph.metric.label(), scale, graph.window_label());
    let caption = clip_to_width(&caption, usize::from(area.width));
    let caption_style = Style::default().fg(if perturbed { palette.warn } else { palette.faint });
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(caption, caption_style))),
        Rect { height: 1, ..area },
    );

    let bars = Rect { y: area.y + 1, height: area.height.saturating_sub(1), ..area };
    if bars.height == 0 || data.is_empty() {
        return;
    }
    frame.render_widget(
        Sparkline::default()
            .data(data)
            .direction(RenderDirection::RightToLeft)
            // A second in which nothing was observed is a GAP, not a zero — a zero would draw a
            // floor-height bar and make an idle app look like a catastrophic stall.
            .absent_value_symbol(" ")
            .style(Style::default().fg(palette.accent))
            .absent_value_style(Style::default().fg(palette.faint)),
        bars,
    );
}

/// Greedy word wrap to `width`, used by the perf overlay's verdict line. Falls back to a hard
/// split for a single word longer than the box.
fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
        while current.chars().count() > width {
            let head: String = current.chars().take(width).collect();
            let tail: String = current.chars().skip(width).collect();
            lines.push(head);
            current = tail;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Render a single frame into `frame`: draw every widget with semantic ANSI colors, then
/// remap the whole buffer to the active theme + contrast palette.
pub fn render(frame: &mut Frame, app: &mut AppState, tick: u64) {
    // Captured before anything can return early: the perf panel floats over every view including
    // the repo page, and its bounds are the terminal, not the docked main area.
    app.frame_area = frame.area();

    // Instrumented only while the perf overlay/report is on: an `Instant::now()` per pass is cheap,
    // but a disabled session should pay nothing beyond the branch.
    if !app.perf.enabled {
        render_widgets(frame, app, tick);
        render_tooltip(frame, app);
        let palette = app.palette();
        apply_palette(frame, &palette);
        apply_hover(frame, app, &palette);
        return;
    }

    let build_started = Instant::now();
    render_widgets(frame, app, tick);
    render_tooltip(frame, app);
    let palette = app.palette();

    let palette_started = Instant::now();
    apply_palette(frame, &palette);
    let palette_took = palette_started.elapsed();

    let hover_started = Instant::now();
    apply_hover(frame, app, &palette);
    let hover_took = hover_started.elapsed();

    let area = frame.area();
    app.perf.cells = u32::from(area.width) * u32::from(area.height);
    app.perf.palette.record(palette_took);
    app.perf.hover.record(hover_took);
    let build_took = build_started.elapsed();
    app.perf.last_build = build_took;
    app.perf.build.record(build_took);

    // The overlay is painted last, over the finished frame, so its own cost is excluded from every
    // channel above — an overlay that inflated the numbers it reports would be worse than useless.
    // Excluded from `build` by being drawn after it, and excluded from `flush` by `last_overlay`
    // (the event loop subtracts it); without that second half the panel's cost is billed to the
    // emulator, which is the one channel the verdict uses to blame the emulator.
    let overlay_started = Instant::now();
    render_perf_overlay(frame, app);
    let overlay_took = overlay_started.elapsed();
    app.perf.last_overlay = overlay_took;
    app.perf.overlay_cost.record(overlay_took);
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
    // The text may be multi-line (`\n`): e.g. an action description over its resolved target URL.
    let rows: Vec<&str> = tip.text.split('\n').collect();
    let text_width =
        rows.iter().map(|row| md_display_width(row)).max().unwrap_or(0) as u16;
    let extra = if tip.hide_column.is_some() { x_label.len() as u16 } else { 0 };
    // border (2) + 1-cell horizontal padding (2) around the text (+ the optional `[x]`).
    let width = (text_width + extra + 4).min(area.width);
    let height = rows.len() as u16 + 2;
    let rect = tui_pick::position(
        tip.anchor,
        (width, height),
        area,
        tip.placement,
        tui_pick::PositionOptions { offset: 0, flip: tip.flip, shift: true },
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
    let base = Style::default();
    if let Some(column) = tip.hide_column {
        let mut spans = inline_md_spans(&tip.text, base);
        spans.push(Span::styled(x_label, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
        // The `[x]` sits after the text + a leading space (3 cells wide).
        let x_start = inner.x + text_width + 1;
        app.tooltip_hide_click = Some((inner.y, x_start, x_start + 3, column));
    } else {
        let lines: Vec<Line> =
            rows.iter().map(|row| Line::from(inline_md_spans(row, base))).collect();
        frame.render_widget(Paragraph::new(lines), inner);
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
    // Cleared per frame, not inside the panel's own render: a closed panel never runs that code,
    // and a stale rect keeps swallowing clicks meant for the list underneath.
    app.coverage_area = Rect::default();
    app.coverage_close_click = None;
    app.coverage_tab_click.clear();
    app.coverage_rows_click.clear();
    app.coverage_check_click.clear();
    app.max_click.clear();
    // Dwell-tooltip regions are re-registered by whatever panes render (list headers/counts, the
    // info panel). Clear once per frame here — NOT in render_list, which is skipped when a non-list
    // pane is maximized (the info panel would then accumulate its tooltips unboundedly).
    app.hover_tooltips.clear();
    // The list pane's click geometry is captured ONLY by `render_list`. When a pane is maximized
    // (Info/Result/RepoPage) `render_list` doesn't run, so these would otherwise keep last frame's
    // rects and a click on the maximized pane would fall THROUGH to a stale list row / header / kebab
    // behind it. Reset them here every frame; `render_list` re-populates them when the list shows.
    app.list_area = Rect::default();
    app.preview_area = Rect::default();
    // Preserve last frame's rows geometry for the hover-only affordances (kebab / hover ★) BEFORE
    // clearing it — they need the frame the list last rendered, not this frame's reset-to-empty rect.
    app.list_rows_area_prev = app.list_rows_area;
    app.list_rows_area = Rect::default();
    app.list_footer_area = Rect::default();
    app.header_area = Rect::default();
    app.list_cols_click = None;
    app.list_sort_click = None;
    app.filter_search_click = None;
    app.filter_search_clear_click = None;
    app.filter_chip_click.clear();
    app.filter_chip_remove_click.clear();
    app.filter_add_click = None;
    app.filter_reset_click = None;
    app.header_click.clear();
    app.pr_cell_click.clear();
    app.fav_cell_click.clear();
    app.kebab_open_click.clear();

    // A maximized repo page is full-screen and replaces the normal layout (it carries its own
    // border footer, so — unlike the other panes — it returns early with no status bar). A restored
    // one falls through to render as a docked bottom panel below the two panes (panel [4]).
    if app.maximized == Some(Pane::RepoPage) && app.repo_page.is_some() {
        app.dock_rect = Rect::default();
        render_repo_page(frame, app, area, tick);
        render_throttle_banner(frame, app, area);
        if app.diff_modal.is_some() {
            render_diff_modal(frame, app, area);
        }
        if app.pr_modal.is_some() {
            render_pr_modal(frame, app, area, tick);
        }
        if app.show_settings {
            render_settings(frame, app, area);
        }
        if app.show_build_info {
            render_build_info(frame, app, area);
        }
        if app.show_changelog {
            render_changelog(frame, app, area);
        }
        // Confirm renders after the modal it may overlay (settings reset, pin-version picker), so
        // it always sits on top.
        if app.confirm.is_some() {
            render_confirm(frame, app, area);
        }
        if app.copy_menu.is_some() {
            render_copy_menu(frame, app, area);
        }
        if app.kebab.is_some() {
            render_kebab(frame, app, area);
        }
        if app.base_picker.is_some() {
            render_base_picker(frame, app, area);
        }
        if app.branch_picker.is_some() {
            render_branch_picker(frame, app, area);
        }
        if app.branch_filter_modal.is_some() {
            render_branch_filter_modal(frame, app, area);
        }
        if app.coverage_modal.is_some() {
            render_coverage(frame, app, area, tick);
        }
        if app.explorer.is_some() {
            render_explorer(frame, app, area);
        }
        // Help overlays the page / diff modal / explorer, showing that view's contextual hotkeys.
        if app.show_help {
            render_help(frame, app, area);
        }
        // The keyboard viewer sits on top of help (it's launched from the Hotkeys tab).
        if app.show_keyboard {
            render_keyboard_modal(frame, app, area);
        }
        if app.show_keybindings {
            render_keybindings_modal(frame, app, area);
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

    app.dock_full_area = full_main_area;
    app.dock_divider_row = None;
    app.dock_rect = Rect::default();

    // A maximized main pane (List/Info/Result) fills the whole main area; the 3-row status bar still
    // shows beneath it (its commands describe these panes), so unlike the repo page this isn't an
    // early return. `divider_col` is parked off-screen-edge so wheel/click routing treats the whole
    // area as that pane's side.
    let max_main = match app.maximized {
        Some(pane) if pane != Pane::RepoPage && app.is_pane_available(pane) => Some(pane),
        _ => None,
    };
    if let Some(pane) = max_main {
        app.main_area = full_main_area;
        if pane == Pane::List {
            app.list_area = full_main_area;
            app.preview_area = Rect::default();
            app.divider_col = full_main_area.x.saturating_add(full_main_area.width);
            let list_offset = render_list(frame, app, full_main_area, tick);
            app.list_offset = list_offset;
        } else {
            // Info or Result — render_preview shows only the maximized sub-pane.
            app.list_area = Rect::default();
            app.preview_area = full_main_area;
            app.divider_col = full_main_area.x;
            render_preview(frame, app, full_main_area, tick);
        }
    } else {
        // Every boundary (dock, list/preview, info/result) always reserves a real 1-cell lane (a
        // row for the dock/info-result splits, a column for the list/preview split) — that's what
        // keeps the splitter draggable regardless of style. `splitter_mode` only controls how
        // render_divider paints that lane: a persistent grip fill (`Dedicated`) or a grip that only
        // shows under the cursor (`Hover`), the lane itself stays empty either way. The lane steals
        // one cell, so the panes are laid out against the reduced extent.

        // Docked repo page: carve a bottom panel off the main area; the boundary is a draggable
        // horizontal splitter (height = dock_ratio of the main area).
        let dock_area = if app.repo_page.is_some() {
            let dock_height = (f64::from(full_main_area.height) * app.dock_ratio).round() as u16;
            let dock_height = dock_height.clamp(6, full_main_area.height.saturating_sub(6).max(6));
            let constraints =
                vec![Constraint::Min(0), Constraint::Length(1), Constraint::Length(dock_height)];
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(full_main_area);
            let dock = *split.last().unwrap();
            // The hotspot/lane row: always the reserved lane (split[1]).
            app.dock_divider_row = Some(split[1].y);
            Some((split[0], dock))
        } else {
            None
        };
        let main_area = dock_area.map_or(full_main_area, |(top, _)| top);

        // Split main area horizontally using the adjustable ratio (against the width left after the
        // reserved divider lane).
        let avail = main_area.width.saturating_sub(1);
        let left_width = ((f64::from(avail)) * app.split_ratio).round() as u16;
        let left_width = left_width.clamp(1, avail.saturating_sub(1).max(1));
        let constraints =
            vec![Constraint::Length(left_width), Constraint::Length(1), Constraint::Min(0)];
        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(main_area);

        let list_area = horizontal_chunks[0];
        let preview_area = *horizontal_chunks.last().unwrap();

        // Capture geometry for mouse hit-testing in the event loop. `divider_col` is always the
        // reserved lane column; the hotspot test is ±1 around it.
        app.main_area = main_area;
        app.list_area = list_area;
        app.preview_area = preview_area;
        app.divider_col = horizontal_chunks[1].x;

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
    }

    // Render status bar
    render_status_bar(frame, app, status_bar_area);

    // The splitter grips: a persistent lane fill (dedicated mode) or a thin on-hover grip (hover
    // mode). No divider when a single pane is maximized (no boundary then). render_divider decides
    // what to draw per mode + cursor. Suppressed while any overlay is up — a modal blocks the panes,
    // so a hover grip cyan-bleeding beside (or, via the tinted scrollbar column, on top of) it is
    // pure noise.
    let overlay_up =
        app.any_modal_open() || app.dropdown.is_some() || app.picker.is_some() || app.finder.is_some();
    if max_main.is_none() && !overlay_up {
        render_divider(frame, app);
    }

    // Throttle warning (top-center) while a remote is rate-limiting us.
    render_throttle_banner(frame, app, area);

    // The explorer is a base modal; Help (and the dropdown pickers, above) overlay it.
    if app.explorer.is_some() {
        render_explorer(frame, app, area);
    }
    // Help modal overlays everything else.
    if app.show_help {
        render_help(frame, app, area);
    }
    // Settings modal overlays everything.
    if app.show_settings {
        render_settings(frame, app, area);
    }
    if app.show_build_info {
        render_build_info(frame, app, area);
    }
    if app.show_changelog {
        render_changelog(frame, app, area);
    }
    // Modals opened from the docked repo page (panel [4]) — without these they open in state but
    // never draw, so a double-click/enter on a stash/dirty row looked like a no-op. The maximized
    // page draws the same set on its own path above.
    if app.diff_modal.is_some() {
        render_diff_modal(frame, app, area);
    }
    if app.pr_modal.is_some() {
        render_pr_modal(frame, app, area, tick);
    }
    if app.copy_menu.is_some() {
        render_copy_menu(frame, app, area);
    }
    if app.kebab.is_some() {
        render_kebab(frame, app, area);
    }
    if app.base_picker.is_some() {
        render_base_picker(frame, app, area);
    }
    if app.branch_picker.is_some() {
        render_branch_picker(frame, app, area);
    }
    if app.branch_filter_modal.is_some() {
        render_branch_filter_modal(frame, app, area);
    }
    if app.coverage_modal.is_some() {
        render_coverage(frame, app, area, tick);
    }
    // Confirmation dialog overlays all — rendered after the modal it may sit over (settings reset,
    // the pin-version picker) so it's always on top.
    if app.confirm.is_some() {
        render_confirm(frame, app, area);
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
    if app.show_keybindings {
        render_keybindings_modal(frame, app, area);
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
/// Fill a vertical run of cells at `col`, rows `[top, bottom)`, with `symbol` in `color`.
fn fill_col(frame: &mut Frame, col: u16, top: u16, bottom: u16, symbol: &str, color: Color) {
    let buffer = frame.buffer_mut();
    for row in top..bottom {
        if let Some(cell) = buffer.cell_mut((col, row)) {
            cell.set_symbol(symbol).set_fg(color);
        }
    }
}

/// Fill a horizontal run of cells at `row`, cols `[left, right)`, with `symbol` in `color`.
fn fill_row(frame: &mut Frame, row: u16, left: u16, right: u16, symbol: &str, color: Color) {
    let buffer = frame.buffer_mut();
    for col in left..right {
        if let Some(cell) = buffer.cell_mut((col, row)) {
            cell.set_symbol(symbol).set_fg(color);
        }
    }
}

/// Draw the pane splitters per `splitter_mode`. Dedicated mode fills each boundary's reserved lane
/// with a persistent `▒` grip (full-height column for list|preview, full-width row for the dock and
/// info/result splits); hover mode keeps the panes flush and shows only a short heavy grip (`┃`
/// vertical, `━` horizontal) under the cursor. Either mode brightens to cyan on hover and `█`/cyan while the
/// vertical splitter is dragged. The vertical grip stays on `divider_col` only (never `col-1`, the
/// list's scrollbar column).
fn render_divider(frame: &mut Frame, app: &AppState) {
    let dedicated = app.splitter_mode == SplitterMode::Dedicated;
    let hover = if app.hover_effects { app.hover } else { None };

    // Vertical splitter (list | preview).
    let area = app.main_area;
    let col = app.divider_col;
    if area.height >= 3 && col > area.x && col < area.x + area.width {
        let top = area.y + 1;
        let bottom = area.y + area.height - 1;
        let dragging = app.divider_dragging;
        let hovered = !dragging
            && hover.is_some_and(|(hc, hr)| {
                (i32::from(hc) - i32::from(col)).abs() <= 1 && hr >= top && hr < bottom
            });
        if dedicated {
            let (sym, color) = if dragging {
                ("█", Color::Cyan)
            } else if hovered {
                ("▒", Color::Cyan)
            } else {
                ("▒", Color::Gray)
            };
            fill_col(frame, col, top, bottom, sym, color);
        } else if dragging || hovered {
            let center = area.y + area.height / 2;
            let half = (area.height / 5).clamp(3, 9) / 2;
            let start = center.saturating_sub(half).max(top);
            let end = (center + half + 1).min(bottom);
            let (sym, color) = if dragging { ("█", Color::Cyan) } else { ("┃", Color::Cyan) };
            fill_col(frame, col, start, end, sym, color);
        }
    }

    // Horizontal splitters: the dock boundary and the info/result split. Dedicated mode fills the
    // reserved lane row; hover mode shows a thin centered grip only under the cursor (its row is the
    // adjacent pane's top border, so it must stay transient — a persistent fill would erase the border).
    let mut h_split = |row: u16, x: u16, width: u16| {
        if width == 0 {
            return;
        }
        let (left, right) = (x, x + width);
        // The boundary is two rows thick — the lower pane's top/title border (`row`) and the upper
        // pane's bottom border (`row - 1`, a clean box-drawing line) — and the grab zone in the event
        // loop accepts both. So treat either row as a hover, and draw the handle on the upper pane's
        // bottom border, where there is no title text to clobber.
        let hovered =
            hover.is_some_and(|(hc, hr)| (hr == row || hr + 1 == row) && hc >= left && hc < right);
        if dedicated {
            let (sym, color) = if hovered { ("▒", Color::Cyan) } else { ("▒", Color::Gray) };
            fill_row(frame, row, left, right, sym, color);
        } else if hovered {
            // A short heavy-horizontal handle, centered and mid-cell so it sits on the `─` border
            // line it overlays and reads as a thicker, grabbable segment.
            let grip_row = row.saturating_sub(1);
            let center = x + width / 2;
            let half = 3u16;
            let start = center.saturating_sub(half).max(left);
            let end = (center + half + 1).min(right);
            fill_row(frame, grip_row, start, end, "━", Color::Cyan);
        }
    };
    if let Some(row) = app.dock_divider_row {
        h_split(row, app.dock_full_area.x, app.dock_full_area.width);
    }
    if let Some(row) = app.preview_divider_row {
        h_split(row, app.preview_area.x, app.preview_area.width);
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

/// A pane's scrollable region: the bar rides the pane's right border, and the content keeps every
/// column the border and padding leave it (minus a gap column when the pane has no padding of its own).
///
/// Vertically clamped to the inner content area, so the bar stays within the scrollable region and off
/// the rounded corners — like a web scrollbar inside its box.
fn scroll_on_border(outer: Rect, inner: Rect) -> scroll::Area {
    scroll::Area::on_border(outer, inner)
}

/// A scrollable region with no border to ride: the last column of `area` becomes the track, the one
/// before it a gap. For a surface drawn inside someone else's frame (a modal's body, a split panel).
fn scroll_inside(area: Rect) -> scroll::Area {
    scroll::Area::inside(area)
}

/// Draw a scrollbar in `region`'s track when the content overflows, and register its draggable
/// `ScrollHit`. `position` is the scroll offset (0..=total-viewport); the thumb brightens while it's
/// being dragged, like the divider.
///
/// **The only way to draw a scrollbar.** It takes a [`scroll::Area`] rather than a rect, so the track
/// is a column carved out by the same call that handed the caller its content rect — a bar can no
/// longer be pointed at the text's own last column, which is how it came to paint over it.
fn render_scrollbar(
    frame: &mut Frame,
    app: &mut AppState,
    region: &scroll::Area,
    position: usize,
    total: usize,
    viewport: usize,
    kind: ScrollKind,
) {
    // INVARIANT: drawing a scrollbar AND registering its draggable `ScrollHit` are one operation —
    // they can't drift apart (a scrollbar that's drawn but not registered is decorative: not
    // draggable, wheel can't target it). Register first so the geometry is always captured;
    // `scrollbar_at` guards `total > viewport`, so a non-overflowing hit simply never matches.
    app.scroll_hits.push(ScrollHit { kind, track: region.track(), total, viewport });
    let thumb = if app.scrollbar_dragging == Some(kind) {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    region.bar(total, position).viewport(viewport).thumb(thumb).draw(frame);
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
