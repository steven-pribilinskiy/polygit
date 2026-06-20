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

pub(crate) fn render_diff_modal(frame: &mut Frame, app: &mut AppState, area: Rect) {
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
    let mut title = format!(" [4] {name} · {head_branch} · {path} ");
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
        ("m".to_string(), key, Some(HintKey::Char('m'))),
        (
            if app.repo_page_maximized { " restore".to_string() } else { " maximize".to_string() },
            hint,
            Some(HintKey::Char('m')),
        ),
        sep(),
        ("esc".to_string(), key, Some(HintKey::Esc)),
        (" back".to_string(), hint, Some(HintKey::Esc)),
    ]);
    // The footer sits on the bottom border, left-aligned (starts one cell in from the corner) so
    // its click columns are predictable.
    let footer_row = area.y + area.height.saturating_sub(1);
    let footer_line = build_hint_footer(footer_segments, area.x + 1, footer_row, &mut app.hint_click);
    // Top-border window controls (Windows-style): a maximize/restore icon, then the `[esc back]`
    // close button. The icon's glyph reflects the current state (restored → maximize; maximized →
    // restore). Both are always-visible, clickable affordances.
    let win_glyph = if app.repo_page_maximized { icons.win_restore } else { icons.win_maximize };
    let back_text = "[esc back]";
    let title_top = Line::from(vec![
        Span::styled(win_glyph, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(back_text, Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
    ])
    .right_aligned();
    let back_end = area.x + area.width.saturating_sub(1);
    let back_start = back_end.saturating_sub(back_text.len() as u16);
    app.repo_page_back_click = Some((area.y, back_start, back_end));
    // The single-cell icon sits one space to the left of `[esc back]`.
    let win_end = back_start.saturating_sub(1);
    let win_start = win_end.saturating_sub(1);
    app.repo_page_window_click = Some((area.y, win_start, win_end));
    // Focused when restored and panel [4] holds focus, or always when maximized (it's the only pane).
    let focused = app.repo_page_maximized || app.focus == Pane::RepoPage;
    let modal_open = app.any_modal_open();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(focused, modal_open))
        .title(title)
        .title_top(title_top)
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

