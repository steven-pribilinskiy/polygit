use super::*;

/// The accent color for a file's git status char in the diff-modal file list.
pub(crate) fn diff_status_color(status: &str) -> Color {
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
pub(crate) fn diff_modal_footer(
    source: &DiffSource,
    focus: DiffFocus,
    chips: bool,
    view: crate::app::DiffView,
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
    // View toggle: raw / unified / split.
    seg.push(sep.clone());
    seg.push(("v".to_string(), key, Some(HintKey::Char('v'))));
    seg.push((format!(" {}", view.label()), hint, Some(HintKey::Char('v'))));
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

/// Color for a syntax token in the unified/split diff views (semantic ANSI so `apply_palette`
/// themes them; an optional `bg` tints the whole line for added/removed rows).
fn tok_style(tok: crate::diffview::Tok, bg: Option<Color>) -> Style {
    use crate::diffview::Tok;
    let style = match tok {
        Tok::Keyword => Style::default().fg(Color::Magenta),
        Tok::Str => Style::default().fg(Color::Green),
        Tok::Num => Style::default().fg(Color::Yellow),
        Tok::Comment => Style::default().fg(Color::DarkGray),
        Tok::Punct => Style::default().fg(Color::Gray),
        Tok::Plain => Style::default(),
    };
    match bg {
        Some(bg) => style.bg(bg),
        None => style,
    }
}

/// Render the diff as a unified view: `old new ± code` rows, line-numbered, syntax-highlighted,
/// with a faint green/red wash on added/removed lines (filled to `width`).
fn diff_unified_lines(
    raw: &[String],
    path: &str,
    width: u16,
    palette: &crate::theme::Palette,
) -> Vec<Line<'static>> {
    use crate::diffview::{self, DiffLineKind};
    let rows = diffview::parse(raw);
    let num_w = rows
        .iter()
        .filter_map(|row| row.old.max(row.new))
        .max()
        .map(|n| n.to_string().len())
        .unwrap_or(2)
        .max(2);
    let faint = Style::default().fg(Color::DarkGray);
    rows.iter()
        .map(|row| match row.kind {
            DiffLineKind::Hunk => Line::from(Span::styled(
                row.text.clone(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            DiffLineKind::Meta => Line::from(Span::styled(row.text.clone(), faint)),
            _ => {
                let (bg, sign, sign_color) = match row.kind {
                    DiffLineKind::Add => (Some(palette.diff_add_bg()), '+', Color::Green),
                    DiffLineKind::Del => (Some(palette.diff_del_bg()), '-', Color::Red),
                    _ => (None, ' ', Color::DarkGray),
                };
                let oldn = row.old.map(|n| n.to_string()).unwrap_or_default();
                let newn = row.new.map(|n| n.to_string()).unwrap_or_default();
                let base = |style: Style| if let Some(bg) = bg { style.bg(bg) } else { style };
                let mut spans = vec![
                    Span::styled(format!("{oldn:>num_w$} {newn:>num_w$} "), base(faint)),
                    Span::styled(format!("{sign} "), base(Style::default().fg(sign_color))),
                ];
                let mut used = num_w * 2 + 2 + 2;
                for (text, tok) in diffview::highlight(&row.text, path) {
                    used += UnicodeWidthStr::width(text.as_str());
                    spans.push(Span::styled(text, tok_style(tok, bg)));
                }
                if let Some(bg) = bg {
                    let pad = (width as usize).saturating_sub(used);
                    spans.push(Span::styled(" ".repeat(pad), Style::default().bg(bg)));
                }
                Line::from(spans)
            }
        })
        .collect()
}

/// One side of a split-diff row: `num code` padded to `code_w`, washed by the line's add/del bg.
fn split_half(
    row: Option<&crate::diffview::DiffRow>,
    path: &str,
    num_w: usize,
    code_w: usize,
    use_new_number: bool,
    palette: &crate::theme::Palette,
) -> Vec<Span<'static>> {
    use crate::diffview::{self, DiffLineKind};
    let Some(row) = row else {
        return vec![Span::raw(" ".repeat(num_w + 1 + code_w))];
    };
    let bg = match row.kind {
        DiffLineKind::Add => Some(palette.diff_add_bg()),
        DiffLineKind::Del => Some(palette.diff_del_bg()),
        _ => None,
    };
    let base = |style: Style| if let Some(bg) = bg { style.bg(bg) } else { style };
    let number = if use_new_number { row.new } else { row.old };
    let numstr = number.map(|n| n.to_string()).unwrap_or_default();
    let mut spans =
        vec![Span::styled(format!("{numstr:>num_w$} "), base(Style::default().fg(Color::DarkGray)))];
    let mut used = 0usize;
    for (text, tok) in diffview::highlight(&row.text, path) {
        let take = UnicodeWidthStr::width(text.as_str());
        if used + take > code_w {
            break;
        }
        used += take;
        spans.push(Span::styled(text, tok_style(tok, bg)));
    }
    spans.push(Span::styled(" ".repeat(code_w.saturating_sub(used)), base(Style::default())));
    spans
}

/// Render the diff side-by-side: old lines on the left, new on the right, each line-numbered and
/// syntax-highlighted; changed lines wash green/red.
fn diff_split_lines(
    raw: &[String],
    path: &str,
    width: u16,
    palette: &crate::theme::Palette,
) -> Vec<Line<'static>> {
    use crate::diffview::{self, DiffLineKind};
    let rows = diffview::parse(raw);
    let split = diffview::to_split(&rows);
    let num_w = rows
        .iter()
        .filter_map(|row| row.old.max(row.new))
        .max()
        .map(|n| n.to_string().len())
        .unwrap_or(2)
        .max(2);
    let half = (width as usize).saturating_sub(1) / 2;
    let code_w = half.saturating_sub(num_w + 1);
    split
        .iter()
        .map(|srow| {
            if srow.full {
                let row = srow.left.as_ref().or(srow.right.as_ref());
                let (text, style) = match row {
                    Some(row) if row.kind == DiffLineKind::Hunk => (
                        row.text.clone(),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Some(row) => (row.text.clone(), Style::default().fg(Color::DarkGray)),
                    None => (String::new(), Style::default()),
                };
                return Line::from(Span::styled(text, style));
            }
            let mut spans = split_half(srow.left.as_ref(), path, num_w, code_w, false, palette);
            spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
            spans.extend(split_half(srow.right.as_ref(), path, num_w, code_w, true, palette));
            Line::from(spans)
        })
        .collect()
}

pub(crate) fn render_diff_modal(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let modal_width = (area.width * 9 / 10).max(20);
    let modal_height = (area.height * 9 / 10).max(8);
    let modal_area = centered_rect(modal_width, modal_height, area);

    let view = app.diff_modal.as_ref().map(|modal| modal.view).unwrap_or_default();
    let diff_path = app
        .diff_modal
        .as_ref()
        .and_then(|modal| modal.files.get(modal.selected).map(|file| file.path.clone()))
        .unwrap_or_default();
    let palette = app.palette();
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
        let footer = diff_modal_footer(&modal.source, modal.focus, modal.chips_active(), modal.view);
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
        render_scrollbar(frame, app, list_inner, file_scroll, visible.len(), view_rows, ScrollKind::DiffFiles);
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
    // Build the full rendered lines for the active view, then window by scroll. Unified/split parse
    // + syntax-highlight the diff (GitHub-PR style); raw keeps git's own colored output.
    let rendered: Vec<Line> = match view {
        DiffView::Raw => diff_lines.iter().map(|line| ansi_line_to_ratatui(line)).collect(),
        DiffView::Unified => diff_unified_lines(&diff_lines, &diff_path, diff_content.width, &palette),
        DiffView::Split => diff_split_lines(&diff_lines, &diff_path, diff_content.width, &palette),
    };
    let diff_total = rendered.len();
    let diff_scroll = diff_scroll_req.min(diff_total.saturating_sub(diff_view_h));
    let diff_view: Vec<Line> =
        rendered[diff_scroll..(diff_scroll + diff_view_h).min(diff_total)].to_vec();
    frame.render_widget(Paragraph::new(diff_view), diff_content);
    render_scrollbar(frame, app, diff_inner, diff_scroll, diff_total, diff_view_h, ScrollKind::DiffBody);

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
pub(crate) fn ahead_behind_spans(
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
pub(crate) fn build_repo_page_info_lines(
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
                    lines.push(pair(
                        "pull request",
                        format!("#{} ({}) {}", pr.number, pr.state.label(), pr.title),
                    ));
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

/// The repo page's footer hints (`↑↓ move · enter diff · …`) as styled, clickable segments. Shared
/// by the maximized page (drawn on its bottom border) and the docked layout (drawn in the status
/// bar when panel [4] holds focus) so both stay in lock-step. The `d` verb is dynamic to the
/// selected row, and `m` reflects the maximize/restore state.
pub(crate) fn repo_page_footer_segments(app: &AppState) -> Vec<(String, Style, Option<HintKey>)> {
    let rows = app.repo_page_rows();
    let selected = app.repo_page_selected.min(rows.len().saturating_sub(1));
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
    // `t cols` and `m maximize/restore` are intentionally NOT in the footer — they live on the
    // page's top border (`t cols ▾` / `m▢`), so repeating them here is redundant.
    footer_segments.extend([
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
    footer_segments
}

/// Render the full-screen dedicated repo page: branches + worktrees + fresh ahead/behind.
pub(crate) fn render_repo_page(frame: &mut Frame, app: &mut AppState, area: Rect, tick: u64) {
    let tabbed = app.repo_page_tabbed();
    let active_tab = app.repo_page_tab;
    let (full_branches, full_worktrees, full_stashes, full_commits) =
        app.repo_page_section_counts();
    let rows = app.repo_page_rows();
    let Some(idx) = app.repo_page else {
        return;
    };
    let selected = app.repo_page_selected.min(rows.len().saturating_sub(1));

    let (name, path, loading, fetched, fetch_error, pulling, head_pr) = {
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
            // The current branch's PR, shown on the HEAD row. This is a single-repo detail view, so
            // it always shows the PR when available, in any state — the "Merged PRs" setting gates
            // only the dense list column.
            state
                .pr
                .as_ref()
                .map(|pr| (format!("#{}", pr.number), pr.url.clone())),
        )
    };
    let head_branch = rows
        .iter()
        .find(|row| row.is_head)
        .map(|row| row.branch.clone())
        .unwrap_or_else(|| "—".to_string());

    // Animated spinner in the title while a pull runs or the page (re)fetches branches.
    let icons = app.icons();
    let mut title = format!(" [4] {name} · {head_branch} · {path} ");
    if pulling {
        title.push_str(&format!("· {} pulling… ", spinner_frame(tick, icons)));
    } else if loading || !fetched {
        title.push_str(&format!("· {} fetching… ", spinner_frame(tick, icons)));
    }
    // The footer hints live on the bottom border ONLY when maximized (there's no status bar then).
    // Docked, the footer is carried by the bottom status bar (it swaps to these hints when panel [4]
    // holds focus), so drawing it here too would duplicate it — see `render_status_bar`.
    let footer_row = area.y + area.height.saturating_sub(1);
    let footer_line = (app.maximized == Some(Pane::RepoPage)).then(|| {
        build_hint_footer(repo_page_footer_segments(app), area.x + 1, footer_row, &mut app.hint_click)
    });
    // Top-border window controls (rightmost): a maximize/restore button (`m`+▢/▣) and a close
    // button (`esc`+✕) — the same compact window-control style as every other pane, in the active
    // icon set (emoji glyphs in emoji mode). The `t cols ▾` / `s sort ▾` triggers sit to their left.
    // Maximize registers into `max_click` (the universal handler toggles it); close into
    // `repo_page_back_click` (the repo-page mouse block closes the page).
    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::DarkGray);
    let cols_text = "t cols ▾";
    // `s sort ▾`, with the active sort + direction shown when a sort is set.
    let sort_label = match app.repo_page_sort {
        Some(sort) => format!(" sort ⟪{} {}⟫ ▾", sort.label(), app.repo_page_sort_dir.arrow()),
        None => " sort ▾".to_string(),
    };
    let sort_text_len = 1 + sort_label.chars().count();
    let sep_w = 3u16; // " · " between every top-border item
    let right_end = area.x + area.width.saturating_sub(1);
    // Close button (rightmost), then maximize/restore a ` · ` to its left.
    let (close_spans, close_region, _) =
        window_button("esc", app.icons().close, area.y, right_end);
    app.repo_page_back_click = Some(close_region);
    app.repo_page_window_click = None;
    let (max_spans, after_max) =
        max_button_spans(app, Pane::RepoPage, area.y, close_region.1.saturating_sub(sep_w));
    let max_start = after_max + 1;
    let sep = || Span::styled(" · ", label_style);
    // The `t cols ▾` / `s sort ▾` triggers apply to the branch-column layout (also shared by the
    // worktrees/stashes rows). The Commits tab uses a fixed sha·date·author·subject layout that
    // those columns/sorts don't touch, so hide the triggers there (and `None` the click regions,
    // which also disables the `t`/`s` keys, since they open the dropdown only when the region is set).
    let show_col_triggers = !(tabbed && active_tab == crate::app::RepoTab::Commits);
    let mut title_spans: Vec<Span> = Vec::new();
    if show_col_triggers {
        let sort_end = max_start.saturating_sub(sep_w);
        let sort_start = sort_end.saturating_sub(sort_text_len as u16);
        let cols_end = sort_start.saturating_sub(sep_w);
        let cols_start = cols_end.saturating_sub(cols_text.chars().count() as u16);
        app.page_sort_click = Some((area.y, sort_start, sort_end));
        app.page_cols_click = Some((area.y, cols_start, cols_end));
        title_spans.extend([
            Span::styled("t", key_style),
            Span::styled(" cols ▾", label_style),
            sep(),
            Span::styled("s", key_style),
            Span::styled(sort_label.clone(), label_style),
            sep(),
        ]);
    } else {
        app.page_sort_click = None;
        app.page_cols_click = None;
    }
    title_spans.extend([
        max_spans[0].clone(),
        max_spans[1].clone(),
        sep(),
        close_spans[0].clone(),
        close_spans[1].clone(),
    ]);
    let title_top = Line::from(title_spans).right_aligned();
    // Focused when restored and panel [4] holds focus, or always when maximized (it's the only pane).
    let focused = (app.maximized == Some(Pane::RepoPage)) || app.focus == Pane::RepoPage;
    let modal_open = app.any_modal_open();
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(focused, modal_open))
        .title(title)
        .title_top(title_top);
    if let Some(footer_line) = footer_line {
        block = block.title_bottom(footer_line);
    }
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
        + if columns.age { 16 } else { 0 } // "  " + 14
        + if columns.pull_request { 9 } else { 0 }; // "  " + 7
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
                      dirty: Option<u32>,
                      upstream: &str,
                      base: &str,
                      base_override: bool,
                      base_clickable: bool,
                      pr: Option<&str>,
                      age: &str,
                      subject: &str|
     -> (Vec<Span<'static>>, Option<usize>, Option<usize>) {
        let mut spans: Vec<Span> = Vec::new();
        let mut base_index = None;
        let mut pr_index = None;
        if columns.ahead_behind {
            spans.push(Span::raw("  "));
            spans.extend(ahead_behind_spans(ahead, behind, 10, icons));
        }
        if columns.dirty {
            spans.push(count_cell(icons.dirty, dirty, count_w, Color::Yellow));
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
            if base_clickable {
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
            } else {
                // Stash rows have no base branch — keep the column slot blank (and not clickable).
                spans.push(Span::styled(format!("  {:<28}", ""), label));
            }
        }
        if columns.age {
            spans.push(Span::styled(format!("  {:<14}", truncate_str(age, 14)), label));
        }
        if columns.pull_request {
            // `#N` on the current-branch row (clickable → opens the PR); blank elsewhere. Fixed
            // 7-cell width (after a 2-space gap) so the following subject stays aligned.
            let text = pr.unwrap_or("");
            if pr.is_some() {
                pr_index = Some(spans.len());
            }
            let style = if pr.is_some() {
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                label
            };
            spans.push(Span::styled(format!("  {text:<7}"), style));
        }
        if columns.subject {
            spans.push(Span::styled(format!("  {}", truncate_str(subject, subject_w)), label));
        }
        (spans, base_index, pr_index)
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
        if columns.pull_request {
            // Static (PR isn't sortable — only the current branch carries one).
            spans.push(Span::styled(format!("  {:<7}", "pr"), label));
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
    // The single clickable PR cell (current-branch row): (absolute item index, start, end, url).
    let mut pr_item: Option<(usize, u16, u16, String)> = None;

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
        let pr_text = if row.is_head { head_pr.as_ref().map(|(text, _)| text.as_str()) } else { None };
        let (cells, base_index, pr_index) = data_cells(
            row.ahead,
            row.behind,
            row.stats,
            Some(row.dirty_count),
            &row.upstream.clone().unwrap_or_default(),
            &row.base.clone().unwrap_or_default(),
            row.base_is_override,
            true,
            pr_text,
            &row.last_commit_rel,
            &row.subject,
        );
        let span_range = |index: usize| {
            let start = prefix_width + cells[..index].iter().map(|span| span.width()).sum::<usize>();
            (start as u16, (start + cells[index].width()) as u16)
        };
        let base_range = base_index.map(span_range);
        // The HEAD row's `#N` PR cell is clickable (opens the PR). Recorded by absolute item index
        // so the scroll-windowed draw loop can register its screen-row click target.
        if let (Some(index), Some((_, url))) = (pr_index, head_pr.as_ref()) {
            let (start, end) = span_range(index);
            pr_item = Some((items.len(), start, end, url.clone()));
        }
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
            let (cells, base_index, _) = data_cells(
                row.ahead,
                row.behind,
                row.stats,
                Some(row.dirty_count),
                &row.upstream.clone().unwrap_or_default(),
                &row.base.clone().unwrap_or_default(),
                row.base_is_override,
                true,
                None,
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
            // Stash rows flow through the same column system as branches: a `stash@{N}` name, the
            // change stats (added/modified/deleted/total, from `git stash show`), and the stash
            // label as the subject. Branch-only columns (ahead/behind, dirty, upstream, base, age)
            // stay blank, and the base cell isn't clickable.
            let stash_ref = format!("stash@{{{}}}", row.stash_index.unwrap_or(0));
            let name_span = Span::styled(
                format!("  {:<name_pad$}", truncate_str(&stash_ref, name_pad)),
                Style::default().fg(Color::Magenta),
            );
            let (cells, _, _) = data_cells(
                None,
                None,
                row.stats,
                None,
                "",
                "",
                false,
                false,
                None,
                "",
                &row.branch,
            );
            let mut line_spans = vec![name_span];
            line_spans.extend(cells);
            items.push((Line::from(line_spans), Some(sel_index), None));
        }
    }

    // Commits: a read-only list (sha · date · author · subject). Rendered here (not via the row
    // machinery — commits aren't PageRows): on the Commits tab, and stacked under its own header in
    // the maximized single view.
    let show_commits = full_commits > 0
        && (!tabbed || active_tab == crate::app::RepoTab::Commits);
    if show_commits {
        let commits = app
            .repos
            .get(idx)
            .and_then(|repo| repo.lock().unwrap().page.as_ref().map(|page| page.commits.clone()))
            .unwrap_or_default();
        if !tabbed {
            items.push((Line::from(String::new()), None, None));
            items.push(section_header("\u{25b4}", Color::Yellow, format!("COMMITS ({full_commits})")));
        }
        // The author column grows to the longest name (not truncated to a fixed width); the subject
        // then fills whatever horizontal space is left.
        let sha_w = 9usize;
        let age_w =
            commits.iter().map(|c| UnicodeWidthStr::width(c.rel_date.as_str())).max().unwrap_or(10).clamp(8, 16);
        let author_w =
            commits.iter().map(|c| UnicodeWidthStr::width(c.author.as_str())).max().unwrap_or(10).clamp(6, 40);
        let used = 2 + sha_w + 2 + age_w + 2 + author_w + 2;
        let subject_w = (inner.width as usize).saturating_sub(used).max(10);
        for commit in &commits {
            items.push((
                Line::from(vec![
                    Span::styled(
                        format!("  {:<sha_w$}", truncate_str(&commit.sha, sha_w)),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(format!("  {:<age_w$}", truncate_str(&commit.rel_date, age_w)), label),
                    Span::styled(format!("  {:<author_w$}", truncate_str(&commit.author, author_w)), cyan),
                    Span::raw(format!("  {}", truncate_str(&commit.subject, subject_w))),
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
    app.repo_page_pr_click = None;
    let mut lines: Vec<Line> = Vec::with_capacity(end.saturating_sub(start));
    for (offset, (line, sel, base_range)) in items[start..end].iter().enumerate() {
        let mut line = line.clone();
        // Register the clickable PR cell when the current-branch row is on screen.
        if let Some((item_index, pr_start, pr_end, url)) = &pr_item {
            if start + offset == *item_index {
                let screen_row = inner.y + offset as u16;
                app.repo_page_pr_click =
                    Some((screen_row, inner.x + *pr_start, inner.x + *pr_end, url.clone()));
            }
        }
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
    render_scrollbar(frame, app, track, app.repo_page_scroll, items.len(), inner_height, ScrollKind::RepoPage);

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

