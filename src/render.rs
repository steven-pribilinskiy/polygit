
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
    IconSet, Leader, PageRowKind, RepoStatus, RightView, ScrollHit, ScrollKind, SortColumn, SortDir,
    StatusFilter,
};

/// The published documentation site (opened by the `D` hotkey and linked in the help modal).
pub const DOCS_URL: &str = "https://steven-pribilinskiy.github.io/pull-all/";

/// The spinner frame for the current render tick (advances every 2 ticks). Shared by the
/// list status glyph and the repo-page loading indicator so they animate identically.
fn spinner_frame(tick: u64, icons: &IconSet) -> &'static str {
    icons.spinner[(tick as usize / 2) % icons.spinner.len()]
}

/// Border color for a main pane: a bright accent when it's the focused pane, dim otherwise.
fn pane_border_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// The base background + foreground for the active theme, or None for Auto (inherit the terminal).
fn theme_base(theme: crate::app::Theme) -> Option<Style> {
    use crate::app::Theme;
    match theme {
        Theme::Auto => None,
        Theme::Dark => {
            Some(Style::default().bg(Color::Rgb(26, 27, 38)).fg(Color::Rgb(192, 197, 206)))
        }
        Theme::Light => {
            Some(Style::default().bg(Color::Rgb(245, 246, 248)).fg(Color::Rgb(40, 42, 48)))
        }
    }
}

/// Clear `area` and, under an explicit theme, repaint the theme's base background so modal
/// interiors match the themed app instead of the terminal's own background.
fn clear_themed(frame: &mut Frame, app: &AppState, area: Rect) {
    frame.render_widget(Clear, area);
    if let Some(base) = theme_base(app.theme) {
        frame.render_widget(Block::default().style(base), area);
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

/// Render a single frame into `frame`.
pub fn render(frame: &mut Frame, app: &mut AppState, tick: u64) {
    let area = frame.area();
    // Paint the theme's base background/foreground first (Auto = inherit the terminal).
    if let Some(base) = theme_base(app.theme) {
        frame.render_widget(Block::default().style(base), area);
    }
    // Draggable scrollbars are re-registered every frame by whatever panels are visible.
    app.scroll_hits.clear();

    // The dedicated repo page is full-screen and replaces the normal layout.
    if app.repo_page.is_some() {
        render_repo_page(frame, app, area, tick);
        if app.confirm.is_some() {
            render_confirm(frame, app, area);
        }
        if app.diff_modal.is_some() {
            render_diff_modal(frame, app, area);
        }
        if app.show_settings {
            render_settings(frame, app, area);
        }
        if app.copy_menu.is_some() {
            render_copy_menu(frame, app, area);
        }
        // Help overlays the page / diff modal, showing that view's contextual hotkeys.
        if app.show_help {
            render_help(frame, app, area);
        }
        render_toast(frame, app, area);
        return;
    }

    // Layout: main area + three-line status bar at bottom
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    let main_area = vertical_chunks[0];
    let status_bar_area = vertical_chunks[1];

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

    // Render status bar
    render_status_bar(frame, app, status_bar_area);

    // Draw the draggable divider grip (and a live highlight while it's being dragged).
    render_divider(frame, app);

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
    // Transient toast on top of everything.
    render_toast(frame, app, area);
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
    // The pane boundary is two adjacent border columns (list's right border + preview's left
    // border); straddle both so the grip is ~2 cells wide and sits right in the middle.
    let cols = [col.saturating_sub(1), col];
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

    // A dotted run at center hints "grab here"; its length scales with the pane height, and it
    // brightens to cyan while dragging.
    let grip_color = if dragging { Color::Cyan } else { Color::Gray };
    let half = (area.height / 5).clamp(3, 9) / 2;
    let start = center.saturating_sub(half).max(top);
    let end = (center + half + 1).min(bottom);
    for &grip_col in &cols {
        for row in start..end {
            if let Some(cell) = buffer.cell_mut((grip_col, row)) {
                cell.set_symbol("▒").set_fg(grip_color);
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

/// First case-insensitive (ASCII) occurrence of `needle` in `name_chars`, as a (start, len)
/// pair in char units. Char-based so multibyte names stay aligned.
fn find_ci(name_chars: &[char], needle: &str) -> Option<(usize, usize)> {
    let needle_chars: Vec<char> = needle.chars().collect();
    if needle_chars.is_empty() || needle_chars.len() > name_chars.len() {
        return None;
    }
    (0..=name_chars.len() - needle_chars.len()).find_map(|start| {
        let matches = name_chars[start..start + needle_chars.len()]
            .iter()
            .zip(&needle_chars)
            .all(|(actual, wanted)| actual.eq_ignore_ascii_case(wanted));
        matches.then_some((start, needle_chars.len()))
    })
}

/// Repo-name spans for the list, underlining the substring that matches the active filter.
/// Padded with trailing spaces to `width` chars in `base` style (no truncation, as before).
fn highlight_name(name: &str, filter: Option<&str>, base: Style, width: usize) -> Vec<Span<'static>> {
    let name_chars: Vec<char> = name.chars().collect();
    let total = name_chars.len();
    let mut spans: Vec<Span<'static>> = Vec::new();

    match filter.filter(|f| !f.is_empty()).and_then(|f| find_ci(&name_chars, f)) {
        Some((start, len)) => {
            let before: String = name_chars[..start].iter().collect();
            let matched: String = name_chars[start..start + len].iter().collect();
            let after: String = name_chars[start + len..].iter().collect();
            if !before.is_empty() {
                spans.push(Span::styled(before, base));
            }
            spans.push(Span::styled(matched, base.add_modifier(Modifier::UNDERLINED)));
            if !after.is_empty() {
                spans.push(Span::styled(after, base));
            }
        }
        None => spans.push(Span::styled(name.to_string(), base)),
    }
    if width > total {
        spans.push(Span::styled(" ".repeat(width - total), base));
    }
    spans
}

fn render_list(frame: &mut Frame, app: &mut AppState, area: Rect, tick: u64) -> usize {
    let visible = app.visible_indices();
    let total_repos = app.repos.len();
    let elapsed = app.finished_elapsed.unwrap_or_else(|| app.start.elapsed()).as_secs_f64();

    let done = app.done_count();
    let title = format!(
        " [1] pull-all · {done}/{total_repos} · {elapsed:.1}s "
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(!app.preview_focused));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Compute column widths
    let max_name_len = app
        .repos
        .iter()
        .map(|repo| repo.lock().unwrap().name.len())
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
    // render 1 cell wider than the Unicode set, so the count columns get +1 each.
    let columns = app.columns;
    let emoji = app.icon_style == crate::app::IconStyle::Emoji;
    let col_extra = usize::from(emoji);
    let dirty_w = 3 + col_extra; // glyph + up to 2 digits
    let count_w = 4 + col_extra; // glyph + count (worktrees / branches / stashes)
    let columns_width = usize::from(columns.ahead_behind) * 10
        + (dirty_w + 1)
        + usize::from(columns.last_commit) * 15
        + usize::from(columns.worktrees) * (count_w + 1)
        + usize::from(columns.branches) * (count_w + 1)
        + usize::from(columns.stashes) * (count_w + 1);

    let inner_width = inner.width as usize;
    let branch_col_width = inner_width
        .saturating_sub(icon_width + name_col_width + separator_width + 2 + columns_width);

    let mut items: Vec<ListItem> = visible
        .iter()
        .map(|&repo_idx| {
            let state = app.repos[repo_idx].lock().unwrap();
            let icons = app.icons();
            // Post-refetch attention flash: pulse REVERSED on the cells whose value changed.
            let flash_on = state.flash_on();
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
            // Pad the glyph to `icon_width` display cells so the name column lines up
            // regardless of whether the glyph is a 1-cell Unicode char or a 2-cell emoji.
            let glyph_pad = icon_width.saturating_sub(glyph.width()).max(1);

            let branch_str = state
                .branch
                .as_deref()
                .unwrap_or("—")
                .to_string();
            let branch_truncated = truncate_str(&branch_str, branch_col_width.max(1));

            let name_style = match &state.status {
                RepoStatus::Failed => Style::default().fg(Color::Red),
                RepoStatus::Updated => Style::default().fg(Color::Green),
                RepoStatus::Skipped | RepoStatus::NoUpstream => Style::default().fg(Color::DarkGray),
                RepoStatus::Running { .. } => Style::default().fg(Color::Yellow),
                _ => Style::default(),
            };

            let mut spans = vec![glyph, Span::raw(" ".repeat(glyph_pad))];
            spans.extend(highlight_name(
                &state.name,
                app.filter.as_deref(),
                name_style,
                name_col_width,
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{branch_truncated:<branch_col_width$}"),
                Style::default().fg(Color::Cyan),
            ));

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
                    flash_style(Style::default().fg(Color::Red), flash.dirty),
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
            if columns.worktrees {
                let count = app.worktrees.iter().filter(|entry| entry.repo == state.name).count();
                let text = if count > 0 { format!("{}{count}", icons.worktrees) } else { String::new() };
                spans.push(Span::styled(
                    format!(" {}", pad_display(&text, count_w)),
                    flash_style(Style::default().fg(Color::Cyan), flash.worktrees),
                ));
            }
            if columns.branches {
                let text = match &state.details {
                    Some(details) if details.branch_count > 0 => {
                        format!("{}{}", icons.branches, details.branch_count)
                    }
                    Some(_) => String::new(),
                    None => "…".to_string(),
                };
                spans.push(Span::styled(
                    format!(" {}", pad_display(&text, count_w)),
                    flash_style(Style::default().fg(Color::Green), flash.branches),
                ));
            }
            if columns.stashes {
                let text = match &state.details {
                    Some(details) if details.stash_count > 0 => {
                        format!("{}{}", icons.stashes, details.stash_count)
                    }
                    Some(_) => String::new(),
                    None => "…".to_string(),
                };
                spans.push(Span::styled(
                    format!(" {}", pad_display(&text, count_w)),
                    flash_style(Style::default().fg(Color::Magenta), flash.stashes),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    // Add separator and Result item
    items.push(ListItem::new(Line::from(vec![Span::styled(
        "─".repeat(inner_width.saturating_sub(2)),
        Style::default().fg(Color::DarkGray),
    )])));

    let result_icons = app.icons();
    let result_glyph = if app.all_done {
        let (_, _, _, _, _, failed, _) = app.counts();
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

    let mut list_state = ListState::default();
    // Map the logical selection to a list index, skipping separator lines:
    //   repo rows → same index; Result → visible.len()+1; Errors → visible.len()+3.
    if app.selected < visible.len() {
        list_state.select(Some(app.selected));
    } else if app.selected == visible.len() {
        list_state.select(Some(visible.len() + 1));
    } else {
        list_state.select(Some(visible.len() + 3));
    }

    // Split the inner area into a 2-row column header (titles + sort indicator) and the repo
    // rows beneath. Too short for a header → use the whole inner area for rows.
    let header_height: u16 = if inner.height >= 4 { 2 } else { 0 };
    let rows_area = Rect {
        x: inner.x,
        y: inner.y + header_height,
        width: inner.width,
        height: inner.height.saturating_sub(header_height),
    };
    let (header_lines, header_click) = if header_height > 0 {
        build_list_header(
            inner,
            icon_width,
            name_col_width,
            branch_col_width,
            columns,
            count_w,
            dirty_w,
            app.sort_column,
            app.sort_dir,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    if header_height > 0 {
        let header_area = Rect { height: header_height, ..inner };
        frame.render_widget(Paragraph::new(header_lines), header_area);
        app.header_area = header_area;
    } else {
        app.header_area = Rect::default();
    }
    app.header_click = header_click;

    let total_items = items.len();
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    frame.render_stateful_widget(list, rows_area, &mut list_state);
    // Scrollbar on the pane's right border, aligned to the rows region (below the header).
    let scrollbar_area = Rect {
        x: area.x,
        y: rows_area.y,
        width: area.width,
        height: rows_area.height,
    };
    render_scrollbar(
        frame,
        scrollbar_area,
        list_state.offset(),
        total_items,
        rows_area.height as usize,
        false,
    );

    app.list_rows_area = rows_area;
    list_state.offset()
}

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
    sort_column: SortColumn,
    sort_dir: SortDir,
) -> (Vec<Line<'static>>, Vec<(u16, u16, SortColumn)>) {
    // (label, width, leading_space, sort) — mirrors the exact widths the rows use.
    struct Cell {
        label: &'static str,
        width: usize,
        lead: bool,
        sort: Option<SortColumn>,
    }
    let mut cells = vec![
        Cell { label: "", width: icon_width, lead: false, sort: None },
        Cell { label: "name", width: name_col_width, lead: false, sort: Some(SortColumn::Name) },
        Cell { label: "", width: 1, lead: false, sort: None },
        Cell { label: "branch", width: branch_col_width, lead: false, sort: None },
    ];
    if columns.ahead_behind {
        cells.push(Cell { label: "↑↓", width: 9, lead: true, sort: Some(SortColumn::AheadBehind) });
    }
    // The dirty column is always present (the `t d` toggle controls the count, not visibility).
    cells.push(Cell { label: "Δ", width: dirty_w, lead: true, sort: Some(SortColumn::Dirty) });
    if columns.last_commit {
        cells.push(Cell { label: "age", width: 14, lead: true, sort: Some(SortColumn::LastCommit) });
    }
    if columns.worktrees {
        cells.push(Cell { label: "wt", width: count_w, lead: true, sort: Some(SortColumn::Worktrees) });
    }
    if columns.branches {
        cells.push(Cell { label: "br", width: count_w, lead: true, sort: Some(SortColumn::Branches) });
    }
    if columns.stashes {
        cells.push(Cell { label: "st", width: count_w, lead: true, sort: Some(SortColumn::Stashes) });
    }

    let active_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let title_style = Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD);
    let mut spans: Vec<Span> = Vec::new();
    let mut clicks: Vec<(u16, u16, SortColumn)> = Vec::new();
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
        } else if cell.sort.is_some() {
            title_style
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(pad_display(&text, cell.width), style));
        if let Some(sort) = cell.sort {
            clicks.push((col, col + cell.width as u16, sort));
        }
        col += cell.width as u16;
    }

    let underline = Line::from(Span::styled(
        "─".repeat(inner.width as usize),
        Style::default().fg(Color::DarkGray),
    ));
    (vec![Line::from(spans), underline], clicks)
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
        RepoStatus::Failed => "failed",
    }
}

fn render_preview(frame: &mut Frame, app: &mut AppState, area: Rect, _tick: u64) {
    let visible = app.visible_indices();

    // Which pane is showing: a repo's log/diff, the Result summary, or the Errors list.
    // The Result overlay (Space) forces Result regardless of selection.
    let show_errors = !app.result_overlay && app.has_errors() && app.selected == visible.len() + 1;
    let show_result = app.result_overlay || (app.selected >= visible.len() && !show_errors);
    let on_repo = !show_result && !show_errors;

    // Info block (`i`): a compact info section above the log/diff, tracking the selection.
    let area = if app.info_pinned && on_repo {
        let repo_idx = visible[app.selected];
        let name = app.repos[repo_idx].lock().unwrap().name.clone();
        let info_width = area.width.saturating_sub(if app.panel_padding { 4 } else { 2 }) as usize;
        let lines = build_info_lines(app, repo_idx, info_width);
        // +2 for the border, +2 more for inner padding when the setting is on.
        let chrome = if app.panel_padding { 4 } else { 2 };
        // Count wrapped rows, not logical lines: a long field (Changes, Remote, Path) wraps to
        // several rows, so sizing by `lines.len()` would clip the tail (Worktrees / Path).
        let wrap_width = info_width.max(1);
        let wrapped_rows: usize = lines
            .iter()
            .map(|line| line.width().max(1).div_ceil(wrap_width))
            .sum();
        // Grow the block to fit its content; only cap it so the log/diff beneath keeps a few rows.
        let max_info = area.height.saturating_sub(3).max(3);
        let desired = (wrapped_rows as u16 + chrome).clamp(3, max_info);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(desired), Constraint::Min(0)])
            .split(area);
        render_info_block(frame, app, chunks[0], format!(" {name} · info "), lines);
        chunks[1]
    } else {
        area
    };

    let (header_text, content_lines, scroll_offset) = if show_errors {
        (" Errors ".to_string(), build_error_summary(app), 0usize)
    } else if show_result {
        (" Result ".to_string(), build_result_summary(app), 0usize)
    } else {
        let repo_idx = visible[app.selected];
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
                " {} · {} · {}{} ",
                state.name,
                status_label(&state.status),
                pid_str,
                elapsed_str
            );
            let lines: Vec<String> = state.log.lines().iter().cloned().collect();
            (header, lines, state.preview_scroll)
        }
    };

    let block = Block::default()
        .title(format!(" [2]{header_text}"))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.preview_focused));

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
    render_scrollbar(
        frame,
        area,
        effective_scroll,
        total_lines,
        inner_height,
        app.scrollbar_dragging == Some(ScrollKind::Preview),
    );

    // Capture scroll geometry for the event loop's wheel/scrollbar hit-testing.
    app.preview_total = total_lines;
    app.preview_viewport = inner_height;
    app.preview_scroll_area = area;
    app.scroll_hits.push(ScrollHit {
        kind: ScrollKind::Preview,
        track: area,
        total: total_lines,
        viewport: inner_height,
    });
}

/// Render the per-repo info view (status, branch, ahead/behind, remote, last commit,
/// worktrees, changes, path) plus a command-hint footer, for the selected repo.
/// Build the per-repo info content lines (status, branch, ahead/behind, commit, changes,
/// remote, worktrees, path) — shared by the full info view and the pinned info section.
fn build_info_lines(app: &AppState, repo_idx: usize, content_width: usize) -> Vec<Line<'static>> {
    let state = app.repos[repo_idx].lock().unwrap();

    let label = Style::default().fg(Color::DarkGray);
    let value = Style::default().fg(Color::Gray);
    let link = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::UNDERLINED);

    let field = |name: &str, text: String| {
        Line::from(vec![
            Span::styled(format!("{name:<13}"), label),
            Span::styled(text, value),
        ])
    };

    let elapsed_str = match state.elapsed {
        Some(elapsed) => format!("{:.2}s", elapsed.as_secs_f64()),
        None => "—".to_string(),
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(field(
        "Status",
        format!("{} · {elapsed_str}", status_label(&state.status)),
    ));
    lines.push(field(
        "Branch",
        state.branch.clone().unwrap_or_else(|| "—".to_string()),
    ));

    if let Some(details) = &state.details {
        let ahead_behind = match (details.ahead, details.behind) {
            (Some(ahead), Some(behind)) => format!("↑{ahead}  ↓{behind}"),
            _ => "(no upstream)".to_string(),
        };
        lines.push(field("Ahead/behind", ahead_behind));
        if details.commit_hash.is_empty() {
            lines.push(field("Last commit", "—".to_string()));
        } else {
            // Three tidy rows — hash, subject, then (date, author) — each starting at the value
            // column and truncated so a long subject never wraps back to the label column.
            let value_width = content_width.saturating_sub(13).max(1);
            lines.push(field("Last commit", details.commit_hash.clone()));
            lines.push(Line::from(vec![
                Span::styled(format!("{:<13}", ""), label),
                Span::styled(truncate_str(&details.commit_subject, value_width), value),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("{:<13}", ""), label),
                Span::styled(
                    truncate_str(
                        &format!("({}, {})", details.commit_rel_date, details.commit_author),
                        value_width,
                    ),
                    label,
                ),
            ]));
        }
        lines.push(field(
            "Changes",
            format!(
                "{} uncommitted · {} stashed · {} feature branches",
                details.dirty_count, details.stash_count, details.branch_count
            ),
        ));
    } else {
        lines.push(field("Ahead/behind", "(loading…)".to_string()));
        lines.push(field("Last commit", "(loading…)".to_string()));
        lines.push(field("Changes", "(loading…)".to_string()));
    }

    match &state.remote_url {
        Some(url) => lines.push(Line::from(vec![
            Span::styled(format!("{:<13}", "Remote"), label),
            Span::styled(url.clone(), link),
        ])),
        None => lines.push(field("Remote", "(none)".to_string())),
    }

    let worktrees: Vec<String> = app
        .worktrees
        .iter()
        .filter(|entry| entry.repo == state.name)
        .map(|entry| entry.branch.clone())
        .collect();
    lines.push(field(
        "Worktrees",
        if worktrees.is_empty() {
            "—".to_string()
        } else {
            worktrees.join(", ")
        },
    ));
    lines.push(field("Path", state.path.display().to_string()));
    lines
}

/// Render an info block (border + lines + scrollbar) into `area`.
fn render_info_block(frame: &mut Frame, app: &AppState, area: Rect, title: String, lines: Vec<Line<'static>>) {
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.preview_focused));
    let inner = block.inner(area);
    let total = lines.len();
    frame.render_widget(block, area);
    let para = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
    render_scrollbar(frame, area, 0, total, inner.height as usize, false);
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

    let (_, _, updated_count, up_to_date_count, skipped_count, failed_count, no_upstream_count) =
        app.counts();

    let total =
        updated_count + up_to_date_count + skipped_count + failed_count + no_upstream_count;

    lines.push("Pull completed!".to_string());
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
    if failed_count > 0 {
        parts.push(format!("{failed_count} failed"));
    }

    lines.push(format!("   {total} total: {}", parts.join(", ")));

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

/// Build a footer row from clickable left segments plus a right-aligned `right` stat fragment
/// (justify-between). When the two sides have room, the gap is plain spaces; when they'd touch
/// or overlap, the left is truncated with `…` and a `·` separator is shown before the stat.
fn compose_status_row(
    segments: Vec<(String, Style, Option<Command>)>,
    right: String,
    area: Rect,
    row_y: u16,
    clickable: &mut Vec<ClickRegion>,
    hint: Style,
) -> Line<'static> {
    let left_width: usize = segments
        .iter()
        .map(|(text, _, _)| UnicodeWidthStr::width(text.as_str()))
        .sum();
    let mut line = build_status_row(segments, area.x, row_y, clickable);
    let avail = area.width as usize;
    let right_width = UnicodeWidthStr::width(right.as_str());
    if right.is_empty() || avail == 0 {
        return line;
    }
    if left_width + right_width + 3 <= avail {
        let gap = avail - left_width - right_width;
        line.spans.push(Span::raw(" ".repeat(gap)));
        line.spans.push(Span::styled(right, hint));
    } else {
        let keep = avail.saturating_sub(right_width + 4);
        line.spans = clip_spans(std::mem::take(&mut line.spans), keep);
        line.spans.push(Span::styled("… · ".to_string(), hint));
        line.spans.push(Span::styled(right, hint));
    }
    line
}

fn render_status_bar(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let (_, running, _, _, _, _, _) = app.counts();
    let done = app.done_count();
    let total = app.repos.len();
    let elapsed = app.finished_elapsed.unwrap_or_else(|| app.start.elapsed()).as_secs_f64();

    let hint = Style::default().fg(Color::DarkGray);
    let active = Style::default().fg(Color::Gray);
    let dimmed = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);

    let style_retry_one = if app.selected_repo_retryable() { active } else { dimmed };
    let style_retry_all = if app.any_retryable() { active } else { dimmed };
    let style_refetch_one = if app.selected_repo_refetchable() { active } else { dimmed };
    let style_refetch_all = if app.any_refetchable() { active } else { dimmed };

    let filtering = app.filter_input_mode;
    let filter_text = app.filter.clone().unwrap_or_default();
    let leader = app.pending_leader;
    let columns = app.columns;
    let status_filter = app.status_filter;
    let sort_column = app.sort_column;
    let sort_dir = app.sort_dir;

    // Per-row right-aligned stat fragments (justify-between: hints left, stat right).
    let stat_done = format!("{done}/{total} done");
    let stat_running = format!("{running} running");
    let stat_elapsed = format!("{elapsed:.1}s");

    let mut clickable: Vec<ClickRegion> = Vec::new();
    let mark = |on: bool| if on { "[x]" } else { "[ ]" };

    // Row 1: the filter prompt, an active leader menu (`t` cols / `f` status / `s` sort), or the
    // normal navigation/filter/sort/layout hints.
    let row1 = if filtering {
        Line::from(format!("Filter: {filter_text}"))
    } else if leader == Some(Leader::Toggle) {
        compose_status_row(
            vec![
                ("cols: ".to_string(), hint, None),
                (format!("{} a ahead/behind", mark(columns.ahead_behind)), active, Some(Command::ToggleColumn(Column::AheadBehind))),
                (" · ".to_string(), hint, None),
                (format!("{} d dirty count", mark(columns.dirty)), active, Some(Command::ToggleColumn(Column::Dirty))),
                (" · ".to_string(), hint, None),
                (format!("{} l last-commit", mark(columns.last_commit)), active, Some(Command::ToggleColumn(Column::LastCommit))),
                (" · ".to_string(), hint, None),
                (format!("{} w worktrees", mark(columns.worktrees)), active, Some(Command::ToggleColumn(Column::Worktrees))),
                (" · ".to_string(), hint, None),
                (format!("{} b branches", mark(columns.branches)), active, Some(Command::ToggleColumn(Column::Branches))),
                (" · ".to_string(), hint, None),
                (format!("{} s stashes", mark(columns.stashes)), active, Some(Command::ToggleColumn(Column::Stashes))),
                (" · esc".to_string(), hint, None),
            ],
            stat_done.clone(),
            area,
            area.y,
            &mut clickable,
            hint,
        )
    } else if leader == Some(Leader::Filter) {
        let pick = |on: bool| if on { "●" } else { "○" };
        let chosen = |filter: StatusFilter| status_filter == filter;
        compose_status_row(
            vec![
                ("filter: ".to_string(), hint, None),
                (format!("{} a all", pick(chosen(StatusFilter::All))), active, Some(Command::SetFilter(StatusFilter::All))),
                (" · ".to_string(), hint, None),
                (format!("{} u updated", pick(chosen(StatusFilter::Updated))), active, Some(Command::SetFilter(StatusFilter::Updated))),
                (" · ".to_string(), hint, None),
                (format!("{} c up-to-date", pick(chosen(StatusFilter::UpToDate))), active, Some(Command::SetFilter(StatusFilter::UpToDate))),
                (" · ".to_string(), hint, None),
                (format!("{} s skipped", pick(chosen(StatusFilter::Skipped))), active, Some(Command::SetFilter(StatusFilter::Skipped))),
                (" · ".to_string(), hint, None),
                (format!("{} f failed", pick(chosen(StatusFilter::Failed))), active, Some(Command::SetFilter(StatusFilter::Failed))),
                (" · ".to_string(), hint, None),
                (format!("{} i issues", pick(chosen(StatusFilter::Issues))), active, Some(Command::SetFilter(StatusFilter::Issues))),
                (" · esc".to_string(), hint, None),
            ],
            stat_done.clone(),
            area,
            area.y,
            &mut clickable,
            hint,
        )
    } else if leader == Some(Leader::Sort) {
        let sort_item = |key: &str, name: &str, column: SortColumn| {
            let chosen = sort_column == column;
            let dot = if chosen { "●" } else { "○" };
            let arrow = if chosen { sort_dir.arrow() } else { "" };
            (format!("{dot} {key} {name}{arrow}"), active, Some(Command::SetSort(column)))
        };
        compose_status_row(
            vec![
                ("sort: ".to_string(), hint, None),
                sort_item("n", "name", SortColumn::Name),
                (" · ".to_string(), hint, None),
                sort_item("s", "status", SortColumn::Status),
                (" · ".to_string(), hint, None),
                sort_item("a", "ahead/behind", SortColumn::AheadBehind),
                (" · ".to_string(), hint, None),
                sort_item("d", "dirty", SortColumn::Dirty),
                (" · ".to_string(), hint, None),
                sort_item("l", "last-commit", SortColumn::LastCommit),
                (" · ".to_string(), hint, None),
                sort_item("w", "worktrees", SortColumn::Worktrees),
                (" · ".to_string(), hint, None),
                sort_item("b", "branches", SortColumn::Branches),
                (" · ".to_string(), hint, None),
                sort_item("k", "stashes", SortColumn::Stashes),
                (" · ".to_string(), hint, None),
                sort_item("o", "none", SortColumn::Discovery),
                (" · esc".to_string(), hint, None),
            ],
            stat_done.clone(),
            area,
            area.y,
            &mut clickable,
            hint,
        )
    } else {
        // Row 1 — move & view. The label words are clickable too, not just the keys.
        compose_status_row(
            vec![
                ("j/k move · space result · ".to_string(), hint, None),
                ("i".to_string(), active, Some(Command::Info)),
                (" info".to_string(), hint, Some(Command::Info)),
                (" · tab focus".to_string(), hint, None),
            ],
            stat_done.clone(),
            area,
            area.y,
            &mut clickable,
            hint,
        )
    };

    // Row 2 — find & layout, prefixed with the active filter/sort tags.
    let name_tag = if filter_text.is_empty() {
        String::new()
    } else {
        format!("[{filter_text}] ")
    };
    let status_tag = status_filter.tag().map(|tag| format!("{{{tag}}} ")).unwrap_or_default();
    let sort_tag = if sort_column == SortColumn::Discovery {
        String::new()
    } else {
        format!("⟪{} {}⟫ ", sort_column.label(), sort_dir.arrow())
    };
    let row2 = compose_status_row(
        vec![
            (format!("{name_tag}{status_tag}{sort_tag}/ filter · "), hint, None),
            ("f".to_string(), active, Some(Command::FilterLeader)),
            (" by-status · ".to_string(), hint, None),
            ("s".to_string(), active, Some(Command::SortLeader)),
            (" sort · ".to_string(), hint, None),
            ("t".to_string(), active, Some(Command::ToggleLeader)),
            (" cols".to_string(), hint, Some(Command::ToggleLeader)),
            (" · [ ] resize".to_string(), hint, None),
        ],
        stat_running.clone(),
        area,
        area.y + 1,
        &mut clickable,
        hint,
    );

    // Row 3 — actions. r/R/e/E dim when they'd be a no-op. The label words are clickable too;
    // clicking the "refetch"/"retry" label runs the all-repos (capital) variant.
    let row3 = compose_status_row(
        vec![
            ("e".to_string(), style_refetch_one, Some(Command::Refetch)),
            ("/".to_string(), hint, None),
            ("E".to_string(), style_refetch_all, Some(Command::RefetchAll)),
            (" refetch".to_string(), hint, Some(Command::RefetchAll)),
            (" · ".to_string(), hint, None),
            ("r".to_string(), style_retry_one, Some(Command::Retry)),
            ("/".to_string(), hint, None),
            ("R".to_string(), style_retry_all, Some(Command::RetryAll)),
            (" retry".to_string(), hint, Some(Command::RetryAll)),
            (" · ".to_string(), hint, None),
            ("enter".to_string(), active, Some(Command::OpenPage)),
            (" page".to_string(), hint, Some(Command::OpenPage)),
            (" · ".to_string(), hint, None),
            (",".to_string(), active, Some(Command::Settings)),
            (" settings".to_string(), hint, Some(Command::Settings)),
            (" · ".to_string(), hint, None),
            ("?".to_string(), active, Some(Command::Help)),
            (" help".to_string(), hint, Some(Command::Help)),
            (" · ".to_string(), hint, None),
            ("q".to_string(), active, Some(Command::Quit)),
            (" quit".to_string(), hint, Some(Command::Quit)),
        ],
        stat_elapsed.clone(),
        area,
        area.y + 2,
        &mut clickable,
        hint,
    );

    app.clickable = clickable;

    let text = Text::from(vec![row1, row2, row3]);
    let para = Paragraph::new(text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(para, area);
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
/// The content of the help modal's "About" tab — what pull-all is, plus clickable links.
fn help_items_about() -> Vec<(Line<'static>, Option<String>)> {
    const GITHUB_URL: &str = "https://github.com/steven-pribilinskiy/pull-all";
    const LAZYGIT_URL: &str = "https://github.com/jesseduffield/lazygit";
    const NOTES_BAKEOFF: &str =
        "https://notes.lvh.me/library/default/devtools/pull-all-tui-bake-off-2026.md";
    const NOTES_FEATURES: &str =
        "https://notes.lvh.me/library/default/devtools/pull-all-tui-interaction-features-2026.md";

    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::Gray);
    let link_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);

    let mut items: Vec<(Line<'static>, Option<String>)> = Vec::new();
    let plain = |text: &str| (Line::from(text.to_string()), None);
    let link = |label: &str, url: &str| {
        let line = Line::from(vec![
            Span::styled(format!("{label:<9}"), label_style),
            Span::styled(url.to_string(), link_style),
        ]);
        (line, Some(url.to_string()))
    };

    items.push((
        Line::from(Span::styled(
            "pull-all — interactive multi-repo git pull dashboard".to_string(),
            header_style,
        )),
        None,
    ));
    items.push(plain(""));
    items.push(plain("Pull every git repo in a directory in parallel, with live per-repo logs,"));
    items.push(plain("branch / worktree / stash management, inline diffs, and a jump into lazygit."));
    items.push(plain("Built with Rust · ratatui · tokio."));
    items.push(plain(""));
    items.push(link("Docs", DOCS_URL));
    items.push(link("GitHub", GITHUB_URL));
    items.push(link("lazygit", LAZYGIT_URL));
    items.push(link("Notes", NOTES_BAKEOFF));
    items.push(link("", NOTES_FEATURES));
    items
}

/// The content of the help modal's "CLI & Flags" tab (subcommands, flags/env, exit codes).
fn help_items_cli() -> Vec<(Line<'static>, Option<String>)> {
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let mut items: Vec<(Line<'static>, Option<String>)> = Vec::new();
    let header = |text: &str| (Line::from(Span::styled(text.to_string(), header_style)), None);
    let plain = |text: &str| (Line::from(text.to_string()), None);

    items.push(header("SUBCOMMANDS  (forward to sibling builds; args passed through)"));
    items.push(plain("  pull-all go  [args]   Go / bubbletea build"));
    items.push(plain("  pull-all bun [args]   Bun / ink build (JIT)"));
    items.push(plain("  pull-all cli [args]   bash streaming version"));
    items.push(plain(""));

    items.push(header("FLAGS & ENVIRONMENT"));
    items.push(plain("  [DIR]                          directory to scan (default: cwd)"));
    items.push(plain("  -j N  / PULL_JOBS=N            concurrency (default: nproc)"));
    items.push(plain("  --timeout S / PULL_TIMEOUT=S   per-pull timeout seconds (default: 30)"));
    items.push(plain("  --no-tui                       plain streaming output (no TUI)"));
    items.push(plain("  --no-worktrees                 skip worktree discovery"));
    items.push(plain("  --profile / PULL_PROFILE=1     per-repo timing report (slowest first)"));
    items.push(plain("  --profile-out FILE             write the profile report to FILE"));
    items.push(plain(""));

    items.push(header("EXIT CODES"));
    items.push(plain("  0 all ok  ·  1 any failed  ·  2 quit mid-run  ·  130 Ctrl-C"));
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
fn help_items_hotkeys(view: HelpView) -> Vec<(Line<'static>, Option<String>)> {
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let subhead_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(Color::Cyan);
    let mut items: Vec<(Line<'static>, Option<String>)> = Vec::new();
    let header = |text: &str| (Line::from(Span::styled(text.to_string(), header_style)), None);
    let subhead = |text: &str| (Line::from(Span::styled(text.to_string(), subhead_style)), None);
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
            items.push(subhead("  Navigate"));
            items.push(kb("j/k  ↑/↓", "move"));
            items.push(kb("g / G", "jump to top / end"));
            items.push(kb("Home / End", "jump to top / bottom"));
            items.push(kb("PgUp / PgDn", "page up / down"));
            items.push(kb("wheel · click", "select a row"));
            items.push(subhead("  Views & panes"));
            items.push(kb("space", "Result / Errors overlay"));
            items.push(kb("tab · 1/2", "focus list ⇄ preview"));
            items.push(kb("i", "info panel"));
            items.push(kb("d", "diff view"));
            items.push(kb("End", "resume autoscroll"));
            items.push(subhead("  Find & sort"));
            items.push(kb("/", "filter by name"));
            items.push(kb("f", "filter by status: a/u/c/s/f/i"));
            items.push(kb("s", "sort: n/s/a/d/l/w/b/k/o (re-pick flips ▲▼); or click a header"));
            items.push(kb("t", "toggle columns: a/d/l/w/b/s"));
            items.push(subhead("  Pull / retry"));
            items.push(kb("r / R", "retry selected / all (failed or skipped)"));
            items.push(kb("e / E", "refetch selected / all (re-pull anything)"));
            items.push(subhead("  Clipboard & open"));
            items.push(kb("y", "copy absolute path"));
            items.push(kb("Y", "copy remote (origin) url"));
            items.push(kb("o", "open remote in browser"));
            items.push(kb("x", "clear this repo's log buffer"));
            items.push(subhead("  Run"));
            items.push(kb("c", "claude in repo dir"));
            items.push(kb("l", "lazygit in repo dir"));
            items.push(subhead("  Other"));
            items.push(kb(", · D", "settings · open docs site"));
            items.push(kb("? · q · ^C", "help · quit · exit"));
            items.push(plain(""));
            items.push(subhead("  Layout"));
            items.push(kb("[ ]", "resize panes"));
            items.push(kb("drag divider", "resize with the mouse"));
        }
        HelpView::RepoPage => {
            items.push(header("HOTKEYS — repo page"));
            items.push(kb("↑↓ · j/k", "move"));
            items.push(kb("g/G · Home/End", "jump to top / bottom"));
            items.push(kb("enter", "open diff (stash or dirty row)"));
            items.push(kb("shift+enter", "checkout (clean, non-current branch)"));
            items.push(kb("p / P", "pull branch / all branches"));
            items.push(kb("d", "delete branch · drop stash · remove worktree · discard (confirm)"));
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
            items.push(kb("↑↓ · j/k", "pick a file"));
            items.push(kb("g / G", "first / last file"));
            items.push(kb("PgUp/PgDn", "scroll the diff"));
            items.push(kb("Home / End", "diff top / bottom"));
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
    let cli = help_items_cli();
    let about = help_items_about();
    let items = match app.help_tab {
        HelpTab::Hotkeys => &hotkeys,
        HelpTab::CliFlags => &cli,
        HelpTab::About => &about,
    };

    // Size the box to the widest/tallest tab (capped to the screen) so switching doesn't resize it.
    let pad = if app.panel_padding { 2 } else { 0 };
    let widest = hotkeys
        .iter()
        .chain(cli.iter())
        .chain(about.iter())
        .map(|(line, _)| line.width())
        .max()
        .unwrap_or(0) as u16;
    let tallest = hotkeys.len().max(cli.len()).max(about.len()) as u16 + 1; // +1 for the tab bar
    let max_width = area.width.saturating_sub(2);
    let max_height = area.height.saturating_sub(2);
    let modal_width = (widest + 4 + pad).min(max_width).max(40.min(max_width));
    let modal_height = (tallest + 2 + pad).min(max_height).max(8.min(max_height));
    let modal_area = centered_rect(modal_width, modal_height, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" pull-all — help · {} ", view.label()))
        .title_bottom(
            Line::from(" tab switch · ↑/↓ scroll · click a link · ?/Esc close ").right_aligned(),
        );
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
        ("CLI & Flags", HelpTab::CliFlags),
        ("About", HelpTab::About),
    ];
    let mut tab_spans: Vec<Span> = Vec::new();
    let mut tab_col = tab_bar_area.x;
    for (label, tab) in tabs {
        let chip = format!(" {label} ");
        let chip_w = UnicodeWidthStr::width(chip.as_str()) as u16;
        let style = if app.help_tab == tab {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        app.help_tab_click.push((tab_bar_area.y, tab_col, tab_col + chip_w, tab));
        tab_spans.push(Span::styled(chip, style));
        tab_spans.push(Span::raw(" "));
        tab_col += chip_w + 1;
    }
    let esc = "[esc]";
    let esc_w = esc.len() as u16;
    let esc_col = tab_bar_area.x + tab_bar_area.width.saturating_sub(esc_w);
    if esc_col > tab_col {
        tab_spans.push(Span::raw(" ".repeat((esc_col - tab_col) as usize)));
    }
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
    let mut lines: Vec<Line> = Vec::with_capacity(end.saturating_sub(start));
    for (offset, (line, url)) in items[start..end].iter().enumerate() {
        if let Some(url) = url {
            app.help_links.push((content_area.y + offset as u16, url.clone()));
        }
        lines.push(line.clone());
    }

    cast_shadow(frame, modal_area);
    clear_themed(frame, app, modal_area);
    frame.render_widget(block, modal_area);
    frame.render_widget(Paragraph::new(tab_bar), tab_bar_area);
    frame.render_widget(Paragraph::new(lines), content_area);
    render_scrollbar(
        frame,
        modal_area,
        app.help_scroll,
        items.len(),
        content_height,
        app.scrollbar_dragging == Some(ScrollKind::Help),
    );
    app.scroll_hits.push(ScrollHit {
        kind: ScrollKind::Help,
        track: modal_area,
        total: items.len(),
        viewport: content_height,
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
fn render_diff_modal(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let modal_width = (area.width * 9 / 10).max(20);
    let modal_height = (area.height * 9 / 10).max(8);
    let modal_area = centered_rect(modal_width, modal_height, area);

    // Owned snapshot so the immutable borrow ends before we write scroll/areas back.
    let (title, footer, files, selected, diff_lines, diff_scroll_req, file_scroll_in, focus) = {
        let Some(modal) = app.diff_modal.as_ref() else {
            return;
        };
        let (title, footer) = match &modal.source {
            DiffSource::Stash { index, label, .. } => (
                format!(" stash@{{{index}}} · {} ", truncate_str(label, 50)),
                " tab panel · j/k navigate · d drop · esc ".to_string(),
            ),
            DiffSource::Dirty { name, .. } => {
                let mode = match modal.mode {
                    DiffMode::Uncommitted => "uncommitted",
                    DiffMode::BaseBranch => "vs base branch",
                };
                (
                    format!(" {name} · {mode} "),
                    " tab panel · j/k navigate · t toggle · d discard/remove · esc ".to_string(),
                )
            }
            DiffSource::Branch { name, .. } => (
                format!(" {name} · vs base branch "),
                " tab panel · j/k navigate · esc ".to_string(),
            ),
        };
        (
            title,
            footer,
            modal.files.clone(),
            modal.selected,
            modal.lines.clone(),
            modal.scroll,
            modal.file_scroll,
            modal.focus,
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(title)
        .title_bottom(Line::from(footer).right_aligned());
    let inner = block.inner(modal_area);
    cast_shadow(frame, modal_area);
    clear_themed(frame, app, modal_area);
    frame.render_widget(block, modal_area);

    // Two bordered sub-panels floating inside the modal: a file-list panel (≤40% height) over the
    // diff panel. Inset from the modal border with a 1-row gap between them so their borders and
    // scrollbars don't collide with the modal border. The focused panel (Tab) gets a bright border.
    let panels = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    let panel_chrome = if app.panel_padding { 4 } else { 2 };
    let max_file_box = (panels.height * 4 / 10).max(3);
    let wanted_file_box = files.len() as u16 + panel_chrome;
    let file_box_height = wanted_file_box.clamp(3, max_file_box);
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
    let file_panel = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(focus_color(focus == DiffFocus::Files)))
        .title(format!(" files ({}) ", files.len()));
    let file_inner = file_panel.inner(file_box);
    frame.render_widget(file_panel, file_box);
    // Reserve the inner's right column for the scrollbar so the rounded border corners stay intact.
    let file_content = Rect { width: file_inner.width.saturating_sub(1), ..file_inner };

    let view_rows = file_content.height as usize;
    // File-list scroll is independent of the selection — just clamp it to the valid range.
    let file_scroll = file_scroll_in.min(files.len().saturating_sub(view_rows));

    if files.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "(no changed files)",
                Style::default().fg(Color::DarkGray),
            ))),
            file_content,
        );
    } else {
        let path_width = file_content.width.saturating_sub(5) as usize;
        let rows: Vec<Line> = files
            .iter()
            .enumerate()
            .skip(file_scroll)
            .take(view_rows)
            .map(|(index, file)| {
                let status = Span::styled(
                    format!(" {} ", file.status),
                    Style::default().fg(diff_status_color(&file.status)),
                );
                let path = Span::raw(truncate_str(&file.path, path_width.max(1)));
                let line = Line::from(vec![status, path]);
                if index == selected {
                    line.style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
                } else {
                    line
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(rows), file_content);
        // Scrollbar inside the panel (on the inner's right column), not on the border.
        render_scrollbar(
            frame,
            file_inner,
            file_scroll,
            files.len(),
            view_rows,
            app.scrollbar_dragging == Some(ScrollKind::DiffFiles),
        );
        app.scroll_hits.push(ScrollHit {
            kind: ScrollKind::DiffFiles,
            track: file_inner,
            total: files.len(),
            viewport: view_rows,
        });
    }

    // ---- Diff panel ----
    let diff_title = if files.is_empty() {
        " diff ".to_string()
    } else {
        format!(" file {}/{} — {} ", selected + 1, files.len(), truncate_str(&files[selected].path, 40))
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

/// Render the full-screen dedicated repo page: branches + worktrees + fresh ahead/behind.
fn render_repo_page(frame: &mut Frame, app: &mut AppState, area: Rect, tick: u64) {
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
    // A terse footer; the `d` verb is dynamic to the selected row, and `?` opens the full keys.
    let d_hint = rows
        .get(selected)
        .and_then(|row| row.delete_action())
        .map(|action| format!(" · d {action}"))
        .unwrap_or_default();
    let footer =
        format!(" ↑↓ move · enter diff · ⇧enter checkout · p pull{d_hint} · y copy · ? help · esc ");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(title)
        .title_bottom(Line::from(footer).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let label = Style::default().fg(Color::DarkGray);
    let head_style = Style::default().fg(Color::Green);
    let value = Style::default().fg(Color::Gray);
    let cyan = Style::default().fg(Color::Cyan);
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);

    let branch_count = rows.iter().filter(|row| row.kind == PageRowKind::Branch).count();
    let worktree_count = rows.iter().filter(|row| row.kind == PageRowKind::Worktree).count();
    let stash_count = rows.iter().filter(|row| row.kind == PageRowKind::Stash).count();
    // Fixed-width dirty column (`•N`, the uncommitted-change count) — same as the main list, so
    // rows stay aligned whether or not they're dirty.
    let dirty_marker = |count: u32| {
        if count > 0 {
            Span::styled(
                pad_display(&format!("{}{count}", icons.dirty), 5),
                Style::default().fg(Color::Red),
            )
        } else {
            Span::raw("     ")
        }
    };
    // Cap the branch-name column so a very long branch name can't push the ahead/behind,
    // upstream, date and subject columns off the screen; longer names truncate with `…`.
    const NAME_MAX: usize = 40;
    let name_pad = rows
        .iter()
        .map(|row| row.branch.chars().count())
        .max()
        .unwrap_or(8)
        .min(NAME_MAX);

    // Section header: a colored type icon for quick recognition, then the yellow label.
    let section_header = |icon: &'static str, icon_color: Color, text: String| {
        (
            Line::from(vec![
                Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
                Span::styled(text, header_style),
            ]),
            None,
        )
    };

    // (Line, Option<selectable index>) — None for headers/blanks. The action banner / fetch
    // error render in a fixed row at the bottom (below), not at the top of the list.
    let mut items: Vec<(Line<'static>, Option<usize>)> = Vec::new();

    items.push(section_header(icons.branches, Color::Green, format!("BRANCHES ({branch_count})")));
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
        let upstream = Span::styled(format!("  {}", row.upstream.clone().unwrap_or_default()), label);
        let date = Span::styled(format!("  {}", row.last_commit_rel), label);
        let subject = Span::styled(format!("  {}", truncate_str(&row.subject, 50)), label);
        let mut line_spans = vec![marker, name_span, Span::raw("  ")];
        line_spans.extend(ahead_behind_spans(row.ahead, row.behind, 10, icons));
        line_spans.push(dirty_marker(row.dirty_count));
        line_spans.push(upstream);
        line_spans.push(date);
        line_spans.push(subject);
        items.push((Line::from(line_spans), Some(sel_index)));
    }

    // Worktrees / stashes sections only appear when there's something to show.
    if worktree_count > 0 {
        items.push((Line::from(String::new()), None));
        items.push(section_header(icons.worktrees, Color::Cyan, format!("WORKTREES ({worktree_count})")));
        for (sel_index, row) in rows.iter().enumerate() {
            if row.kind != PageRowKind::Worktree {
                continue;
            }
            let mut line_spans = vec![
                Span::styled(format!("  {:<name_pad$}", truncate_str(&row.branch, name_pad)), cyan),
                Span::raw("  "),
            ];
            line_spans.extend(ahead_behind_spans(row.ahead, row.behind, 10, icons));
            line_spans.push(dirty_marker(row.dirty_count));
            line_spans.push(Span::styled(format!("  {}", row.path.display()), label));
            items.push((Line::from(line_spans), Some(sel_index)));
        }
    }

    if stash_count > 0 {
        items.push((Line::from(String::new()), None));
        items.push(section_header(icons.stashes, Color::Magenta, format!("STASHES ({stash_count})")));
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
            ));
        }
    }

    // Reserve a fixed bottom row for the action banner / fetch error when present.
    let banner = app
        .repo_page_message
        .clone()
        .map(|message| (format!(" {message}"), Color::Yellow))
        .or_else(|| fetch_error.as_ref().map(|error| (format!(" fetch: {error}"), Color::Red)));
    let content = if banner.is_some() {
        Rect { height: inner.height.saturating_sub(1), ..inner }
    } else {
        inner
    };
    let inner = content;
    let inner_height = inner.height as usize;
    let max_scroll = items.len().saturating_sub(inner_height);
    if app.repo_page_scroll > max_scroll {
        app.repo_page_scroll = max_scroll;
    }
    let start = app.repo_page_scroll;
    let end = (start + inner_height).min(items.len());

    app.repo_page_click.clear();
    let mut lines: Vec<Line> = Vec::with_capacity(end.saturating_sub(start));
    for (offset, (line, sel)) in items[start..end].iter().enumerate() {
        let mut line = line.clone();
        if let Some(sel_index) = sel {
            app.repo_page_click.push((inner.y + offset as u16, *sel_index));
            if *sel_index == selected {
                line.style = Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD);
            }
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
    render_scrollbar(
        frame,
        area,
        app.repo_page_scroll,
        items.len(),
        inner_height,
        app.scrollbar_dragging == Some(ScrollKind::RepoPage),
    );
    app.scroll_hits.push(ScrollHit {
        kind: ScrollKind::RepoPage,
        track: area,
        total: items.len(),
        viewport: inner_height,
    });

    // The action banner / fetch error sits in the reserved bottom row.
    if let Some((text, color)) = banner {
        let banner_area = Rect {
            x: inner.x,
            y: inner.y + inner.height,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))),
            banner_area,
        );
    }
}

/// Render the yes/no confirmation dialog (keyboard-driven: y / n / Esc).
fn render_confirm(frame: &mut Frame, app: &AppState, area: Rect) {
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
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(border_color))
        .title(title);
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    clear_themed(frame, app, modal);
    frame.render_widget(block, modal);
    let mut lines = vec![
        Line::from(String::new()),
        Line::from(Span::styled(
            format!("  {}", confirm.message),
            Style::default().fg(Color::Gray),
        )),
    ];
    if has_files {
        lines.push(Line::from(String::new()));
        lines.append(&mut detail_lines);
    }
    if confirm.danger {
        lines.push(Line::from(String::new()));
        lines.push(Line::from(Span::styled(
            format!("  {} This cannot be undone.", icons.warning),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(String::new()));
    lines.push(Line::from(Span::styled(
        "  [y/enter] yes     [n] no",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Render the settings modal (`,`): a small centered box with toggle rows for panel padding
/// and the icon style. `↑↓` move, `space`/`enter` toggle, `esc` closes.
fn render_settings(frame: &mut Frame, app: &AppState, area: Rect) {
    // One row: a `>` cursor for the selected row, a label, then two option chips where the
    // active value is colored and the other dim.
    let row = |selected: bool, label: &str, options: &[(&str, bool)]| -> Line<'static> {
        let cursor = if selected { "> " } else { "  " };
        let label_style = if selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let mut spans = vec![
            Span::styled(format!("  {cursor}"), label_style),
            Span::styled(format!("{label:<14}"), label_style),
        ];
        for (index, (text, active)) in options.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw("  "));
            }
            let style = if *active {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let dot = if *active { "●" } else { "○" };
            spans.push(Span::styled(format!("{dot} {text}"), style));
        }
        Line::from(spans)
    };

    let padding_on = app.panel_padding;
    let emoji = app.icon_style == crate::app::IconStyle::Emoji;
    let theme = app.theme;
    use crate::app::Theme;
    let mut lines = vec![
        Line::from(String::new()),
        row(
            app.settings_selected == 0,
            "Panel padding",
            &[("on", padding_on), ("off", !padding_on)],
        ),
        row(
            app.settings_selected == 1,
            "Icons",
            &[("unicode", !emoji), ("emoji", emoji)],
        ),
        row(
            app.settings_selected == 2,
            "Theme",
            &[
                ("auto", theme == Theme::Auto),
                ("dark", theme == Theme::Dark),
                ("light", theme == Theme::Light),
            ],
        ),
        Line::from(String::new()),
        Line::from(Span::styled(
            "  ↑↓ move · space/enter toggle · esc close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let pad = if app.panel_padding { 2 } else { 0 };
    let width = 48u16.min(area.width.saturating_sub(2)).max(20) + pad;
    let height = (lines.len() as u16 + 2 + pad).min(area.height.saturating_sub(2).max(6));
    let modal = centered_rect(width, height, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Settings ");
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    clear_themed(frame, app, modal);
    frame.render_widget(block, modal);
    // Drop the leading blank line if padding already provides the top gap.
    if app.panel_padding {
        lines.remove(0);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Render the transient toast (reusable, app-wide): a small rounded notice near the bottom-center
/// that auto-dismisses. Call last so it overlays everything; no-op when no toast is active.
fn render_toast(frame: &mut Frame, app: &AppState, area: Rect) {
    let Some(message) = app.active_toast() else {
        return;
    };
    let text = format!("  {message}  ");
    let width = (UnicodeWidthStr::width(text.as_str()) as u16 + 2).clamp(8, area.width);
    let height = 3u16;
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
    clear_themed(frame, app, toast_area);
    frame.render_widget(block, toast_area);
    frame.render_widget(
        Paragraph::new(
            Line::from(Span::styled(text, Style::default().add_modifier(Modifier::BOLD))).centered(),
        ),
        inner,
    );
}

/// Render the repo-page `y` copy menu: pick what to copy — path, branch, or both.
fn render_copy_menu(frame: &mut Frame, app: &AppState, area: Rect) {
    let selected = app.copy_menu.unwrap_or(0);
    let options = ["absolute path", "branch name", "both (path + branch)"];
    let mut lines: Vec<Line> = vec![Line::from(String::new())];
    for (index, label) in options.iter().enumerate() {
        let cursor = if index == selected { "> " } else { "  " };
        let style = if index == selected {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(format!("  {cursor}{label}"), style)));
    }
    lines.push(Line::from(String::new()));
    lines.push(Line::from(Span::styled(
        "  ↑↓ move · enter copy · esc close",
        Style::default().fg(Color::DarkGray),
    )));

    let pad = if app.panel_padding { 2 } else { 0 };
    let width = 38u16.min(area.width.saturating_sub(2)).max(24) + pad;
    let height = (lines.len() as u16 + 2 + pad).min(area.height.saturating_sub(2).max(6));
    let modal = centered_rect(width, height, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Copy ");
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    clear_themed(frame, app, modal);
    frame.render_widget(block, modal);
    if app.panel_padding {
        lines.remove(0);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
