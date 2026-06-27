use super::*;

pub(crate) fn render_preview(frame: &mut Frame, app: &mut AppState, area: Rect, _tick: u64) {
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
    // Reset the info-pane scroll geometry each frame; render_info_panel re-captures it when shown,
    // so the wheel only targets the info pane while it's actually on screen.
    app.info_area = Rect::default();
    app.info_total = 0;
    app.info_viewport = 0;

    // The preview pane stacks an info panel (`i`, top, repo-only) and the result/log panel (`I`,
    // bottom). Each hides independently; with both shown a draggable boundary splits them by
    // `preview_split_ratio`. Hidden result → info fills the pane (reads like the repo list).
    // When a sub-pane is maximized, render_preview owns the whole screen and shows only that one.
    let info_visible = match app.maximized {
        Some(Pane::Info) => true,
        Some(Pane::Result) => false,
        _ => app.info_pinned && selected_repo.is_some(),
    };
    let result_visible = match app.maximized {
        Some(Pane::Result) => true,
        Some(Pane::Info) => false,
        _ => app.show_result_panel,
    };
    app.preview_divider_row = None;
    let area = match (info_visible, result_visible) {
        (true, true) => {
            let repo_idx = selected_repo.unwrap();
            // In "dedicated" splitter mode a 1-row lane separates the info + result panes (filled by
            // render_divider); in "hover" mode they're flush and the boundary row is the result's top
            // border. Lay out info against the height left after the lane, if any.
            let dedicated = app.splitter_mode == SplitterMode::Dedicated;
            let avail = if dedicated { area.height.saturating_sub(1) } else { area.height };
            let info_h = ((f64::from(avail)) * app.preview_split_ratio).round() as u16;
            let info_h = info_h.clamp(3, avail.saturating_sub(3).max(3));
            let constraints = if dedicated {
                vec![Constraint::Length(info_h), Constraint::Length(1), Constraint::Min(0)]
            } else {
                vec![Constraint::Length(info_h), Constraint::Min(0)]
            };
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(area);
            app.preview_split_area = area;
            // The hotspot/lane row: the dedicated lane (chunks[1]) or the result pane's top border.
            let result_area = *chunks.last().unwrap();
            app.preview_divider_row = Some(if dedicated { chunks[1].y } else { result_area.y });
            render_info_panel(frame, app, chunks[0], repo_idx);
            result_area
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
    // The maximize/restore button is the rightmost top-border element; the `⧉` copy button (when the
    // repo log shows) sits to its left. Both are right-aligned into one title line.
    let (max_spans, copy_end) =
        max_button_spans(app, Pane::Result, area.y, area.x + area.width.saturating_sub(1));
    let mut top_spans: Vec<Span<'static>> = Vec::new();
    // A `⧉` copy button copies the repo's log when it has output, otherwise the repo path — so it's
    // always useful (an up-to-date repo's log is empty). Same clipboard handler as the info Path copy.
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
            // The copy button follows the icon set (⧉ in Unicode mode, 📋 in emoji mode); its click +
            // hover region is exactly the glyph's display width (1 or 2 cells), so the hover bg lands
            // squarely on the glyph instead of a fixed 2-char target offset to its left. A 2-col gap
            // (the two spaces below) separates it from the maximize button.
            let glyph = app.icons().copy;
            let glyph_w = UnicodeWidthStr::width(glyph) as u16;
            let copy_end = copy_end.saturating_sub(1); // widen the gap to 2 cols
            let col_start = copy_end.saturating_sub(glyph_w);
            top_spans.push(Span::styled(glyph, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
            top_spans.push(Span::raw("  "));
            app.info_click.push((area.y, col_start, copy_end, InfoAction::CopyText(copy_text)));
        }
    }
    top_spans.push(max_spans[0].clone());
    top_spans.push(max_spans[1].clone());
    let mut block = Block::default()
        .title(format!(" [3]{header_text}"))
        .title_top(Line::from(top_spans).right_aligned())
        .title_style(pane_title_style(modal_open))
        .borders(pane_borders(app))
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.active_pane() == Pane::Result, modal_open));

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
    // Capture scroll geometry for the event loop's wheel hit-testing; render_scrollbar registers the
    // draggable Preview hit.
    app.preview_total = total_lines;
    app.preview_viewport = inner_height;
    app.preview_scroll_area = track;
    render_scrollbar(frame, app, track, effective_scroll, total_lines, inner_height, ScrollKind::Preview);
}

/// Render the per-repo info view (status, branch, ahead/behind, remote, last commit,
/// worktrees, changes, path) plus a command-hint footer, for the selected repo.
/// Build the per-repo info content lines (status, branch, ahead/behind, commit, changes,
/// remote, worktrees, path) — shared by the full info view and the pinned info section.
/// A browsable https base for a remote URL (strips a trailing `.git`), or None for non-web remotes.
pub(crate) fn web_remote(remote: &str) -> Option<String> {
    let trimmed = remote.trim().trim_end_matches('/');
    let base = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    base.starts_with("https://").then(|| base.to_string())
}

/// Semantic color for a PR's lifecycle state — green=open, magenta=merged, gray=closed.
pub(crate) fn pr_state_color(state: crate::app::PrState) -> Color {
    match state {
        crate::app::PrState::Open => Color::Green,
        crate::app::PrState::Merged => Color::Magenta,
        crate::app::PrState::Closed => Color::DarkGray,
    }
}

/// Split `text` into chunks of at most `width` display columns, on char boundaries.
pub(crate) fn wrap_chars(text: &str, width: usize) -> Vec<String> {
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


/// Parse inline markdown — `**bold**` and `` `code` `` — over `base`, returning styled runs with
/// the markers stripped. Code spans get a distinct color; bold adds the bold modifier to `base`.
/// A lone `*` and unmatched markers render literally. Good enough for release-note prose.
fn parse_inline_md(text: &str, base: Style) -> Vec<(String, Style)> {
    let bold = base.add_modifier(Modifier::BOLD);
    let code = Style::default().fg(Color::Yellow);
    let chars: Vec<char> = text.chars().collect();
    let mut runs: Vec<(String, Style)> = Vec::new();
    let mut buf = String::new();
    let mut in_bold = false;
    let mut in_code = false;
    let mut idx = 0;
    while idx < chars.len() {
        let style = if in_code { code } else if in_bold { bold } else { base };
        if !in_code && chars[idx] == '*' && chars.get(idx + 1) == Some(&'*') {
            if !buf.is_empty() {
                runs.push((std::mem::take(&mut buf), style));
            }
            in_bold = !in_bold;
            idx += 2;
            continue;
        }
        if chars[idx] == '`' {
            if !buf.is_empty() {
                runs.push((std::mem::take(&mut buf), style));
            }
            in_code = !in_code;
            idx += 1;
            continue;
        }
        buf.push(chars[idx]);
        idx += 1;
    }
    if !buf.is_empty() {
        let style = if in_code { code } else if in_bold { bold } else { base };
        runs.push((buf, style));
    }
    runs
}

/// Parse inline markdown in `text` over `base`, then word-wrap to `width` columns preserving each
/// run's style. Returns wrapped lines, each a list of `(text, style)` segments. The single shared
/// release-note renderer for the changelog, What's New, and version-picker modals.
pub(crate) fn wrap_markdown(text: &str, base: Style, width: usize) -> Vec<Vec<(String, Style)>> {
    let runs = parse_inline_md(text, base);
    if width == 0 {
        return vec![runs];
    }
    // Split the styled runs into whitespace-delimited words (a word keeps its segments' styles).
    let mut words: Vec<Vec<(String, Style)>> = Vec::new();
    let mut word: Vec<(String, Style)> = Vec::new();
    for (run_text, style) in &runs {
        for (part_idx, part) in run_text.split(' ').enumerate() {
            if part_idx > 0 && !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            if !part.is_empty() {
                word.push((part.to_string(), *style));
            }
        }
    }
    if !word.is_empty() {
        words.push(word);
    }

    let seg_width = |segs: &[(String, Style)]| -> usize {
        segs.iter().map(|(text, _)| unicode_width::UnicodeWidthStr::width(text.as_str())).sum()
    };
    let mut lines: Vec<Vec<(String, Style)>> = Vec::new();
    let mut line: Vec<(String, Style)> = Vec::new();
    let mut line_width = 0;
    for word in words {
        let word_width = seg_width(&word);
        if word_width > width {
            // A word too long to ever fit: hard-split on chars, keeping its first segment's style.
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            let style = word.first().map(|(_, style)| *style).unwrap_or(base);
            let joined: String = word.into_iter().map(|(text, _)| text).collect();
            let mut chunks = wrap_chars(&joined, width);
            if let Some(last) = chunks.pop() {
                for chunk in chunks {
                    lines.push(vec![(chunk, style)]);
                }
                line_width = unicode_width::UnicodeWidthStr::width(last.as_str());
                line = vec![(last, style)];
            }
            continue;
        }
        let sep = usize::from(!line.is_empty());
        if line_width + sep + word_width > width {
            lines.push(std::mem::take(&mut line));
            line.extend(word);
            line_width = word_width;
        } else {
            if sep == 1 {
                line.push((" ".to_string(), Style::default()));
                line_width += 1;
            }
            line.extend(word);
            line_width += word_width;
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// Wrap a link / URL across `width`-wide lines, preferring to break right AFTER a separator
/// (`/ - . : _ @`) so it splits at natural boundaries; falls back to a hard char break when no
/// separator fits on the line. Each returned segment is ≤ `width` display columns.
pub(crate) fn wrap_link(text: &str, width: usize) -> Vec<String> {
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
pub(crate) type InfoClick = (usize, u16, u16, InfoAction);

pub(crate) fn build_info_lines(
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
    // Reserve 2 trailing cols for a ` ⧉` copy button on copyable rows.
    let copy_avail = value_width.saturating_sub(2).max(1);
    // The copy-button glyph: a standout magenta on whole-line-copy rows (Path/Worktrees/plain
    // Branch); the existing `dim` style is reused on link rows, where the value's own click opens
    // the link and copy is a separate, secondary button.
    let copy_icon = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);

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

    // A plain (non-link) field whose value + trailing `⧉` copy its content. The label is NOT part
    // of the click/highlight region — the region spans the value through the 2-char copy button
    // (` ⧉`), so hovering the value or the button highlights it and a click copies. `display` is the
    // (already-truncated) text shown; `copy` is the full value copied.
    let push_copyable = |lines: &mut Vec<Line<'static>>,
                         clicks: &mut Vec<InfoClick>,
                         name: &str,
                         display: String,
                         copy: String| {
        let line_idx = lines.len();
        let value_w = UnicodeWidthStr::width(display.as_str()) as u16;
        let end = LABEL_W as u16 + value_w + 2; // value + the 2-char copy button (" " + "⧉")
        clicks.push((line_idx, LABEL_W as u16, end, InfoAction::CopyText(copy)));
        lines.push(Line::from(vec![
            Span::styled(format!("{name:<13}"), label),
            Span::styled(display, value),
            Span::raw(" "),
            Span::styled("⧉".to_string(), copy_icon),
        ]));
    };

    // The Branch row when it links to its remote page: the name opens the link, and a SEPARATE,
    // dim `⧉` copies it — copy isn't the line's primary action here, so the line isn't a whole-line
    // copy target and the icon stays subdued. A branch long enough to wrap falls back to the plain
    // link (no icon) so the button never lands on a wrapped continuation.
    let push_branch_link = |lines: &mut Vec<Line<'static>>, clicks: &mut Vec<InfoClick>, branch: &str, url: &str| {
        if UnicodeWidthStr::width(branch) <= copy_avail {
            let line_idx = lines.len();
            let width = UnicodeWidthStr::width(branch) as u16;
            clicks.push((line_idx, LABEL_W as u16, LABEL_W as u16 + width, InfoAction::OpenUrl(url.to_string())));
            // The copy button is a 2-char target: the space before the glyph plus the glyph itself.
            let copy_start = LABEL_W as u16 + width;
            clicks.push((line_idx, copy_start, copy_start + 2, InfoAction::CopyText(branch.to_string())));
            lines.push(Line::from(vec![
                Span::styled(format!("{:<13}", "Branch"), label),
                Span::styled(branch.to_string(), link),
                Span::raw(" "),
                Span::styled("⧉".to_string(), dim),
            ]));
        } else {
            push_link(lines, clicks, "Branch", branch, url);
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

    // Branch — clickable to its page on the remote, but ONLY when the branch is actually on the
    // remote. A no-upstream / "ref gone" branch (e.g. its PR was merged and the remote branch
    // deleted) has no `/tree/<branch>` page — linking it just 404s — so render it as plain text.
    let branch = state.branch.clone().unwrap_or_else(|| "—".to_string());
    let on_remote = !matches!(state.status, RepoStatus::NoUpstream);
    let branch_link = (branch != "—" && on_remote)
        .then(|| state.remote_url.as_deref())
        .flatten()
        .and_then(web_remote)
        .map(|base| format!("{base}/tree/{branch}"));
    match branch_link {
        Some(url) => push_branch_link(&mut lines, &mut clicks, &branch, &url),
        None => {
            let display = if UnicodeWidthStr::width(branch.as_str()) > copy_avail {
                truncate_str(&branch, copy_avail)
            } else {
                branch.clone()
            };
            push_copyable(&mut lines, &mut clicks, "Branch", display, branch);
        }
    }

    // Pull Request — the open PR for the current branch (via `gh`), clickable to the PR on the
    // remote. Shown only when one exists; a dim "checking…" appears while the lookup is in flight.
    if let Some(pr) = state.pr.as_ref().filter(|pr| pr.shown(app.show_merged_prs)) {
        let text = format!("#{} {}", pr.number, pr.title);
        push_link(&mut lines, &mut clicks, "Pull Request", &text, &pr.url);
        // Sub-line: a colored state badge (open/merged/closed) + a dim "checked … ago" (per-entry
        // cache timestamp). The badge clarifies why a "ref gone" branch has no link — it merged.
        let mut sub: Vec<Span> = vec![
            Span::raw(format!("{:<13}", "")),
            Span::styled(pr.state.label(), Style::default().fg(pr_state_color(pr.state))),
        ];
        if let Some(checked_at) = state.pr_checked_at {
            let age = crate::app::format_cache_age(checked_at);
            let when = if age == "now" { "just now".to_string() } else { format!("{age} ago") };
            sub.push(Span::styled(format!(" · checked {when}"), dim));
        }
        lines.push(Line::from(sub));
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
    // One line per worktree so each branch copies individually (not all concatenated). The first
    // line carries the label; continuations indent to the value column. Each whole line copies its
    // own branch.
    for (index, branch) in worktrees.iter().enumerate() {
        let display = if UnicodeWidthStr::width(branch.as_str()) > copy_avail {
            truncate_str(branch, copy_avail)
        } else {
            branch.clone()
        };
        let name = if index == 0 { "Worktrees" } else { "" };
        push_copyable(&mut lines, &mut clicks, name, display, branch.clone());
    }

    // Path — value left-truncated to keep the filename tail. The whole line copies the full path
    // (hover highlights it); a trailing standout `⧉` marks it as copyable.
    let path = state.path.display().to_string();
    let display = if UnicodeWidthStr::width(path.as_str()) > copy_avail {
        truncate_left(&path, copy_avail)
    } else {
        path.clone()
    };
    push_copyable(&mut lines, &mut clicks, "Path", display, path);

    (lines, clicks)
}

/// Render an info block (border + pre-wrapped lines + scrollbar) into `area`, and translate each
/// clickable region's in-line columns into absolute screen rects on `app.info_click`.
/// Render the pinned info panel for `repo_idx` into `area` (sized by the caller — full pane or the
/// top half of a split). Clips to fit; the info content is short.
pub(crate) fn render_info_panel(frame: &mut Frame, app: &mut AppState, area: Rect, repo_idx: usize) {
    let name = app.repos[repo_idx].lock().unwrap().name.clone();
    let info_width = area.width.saturating_sub(if app.panel_padding { 4 } else { 2 }) as usize;
    let (lines, clicks) = build_info_lines(app, repo_idx, info_width);
    let scroll = app.repos[repo_idx].lock().unwrap().info_scroll;
    let scroll = render_info_block(frame, app, area, format!(" [2] {name} · info "), lines, clicks, scroll);
    // Write back the clamped offset so the wheel/drag never sit past the content.
    app.repos[repo_idx].lock().unwrap().info_scroll = scroll;
}

/// Render the placeholder shown when the result/log panel is hidden and there's no info panel to
/// fill the pane — a bordered box with a centered hint on how to bring the panel back.
pub(crate) fn render_preview_hidden_hint(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let modal_open = app.any_modal_open();
    let block = Block::default()
        .title(" [3] ")
        .title_style(pane_title_style(modal_open))
        .borders(pane_borders(app))
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.focus == Pane::Result, modal_open));
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

/// Render the info block (border + scrollable pre-wrapped lines + draggable scrollbar), translating
/// each clickable region to an absolute screen rect on `app.info_click`. Returns the clamped scroll
/// offset (the caller persists it on the repo). `scroll` is the requested top line.
pub(crate) fn render_info_block(
    frame: &mut Frame,
    app: &mut AppState,
    area: Rect,
    title: String,
    lines: Vec<Line<'static>>,
    clicks: Vec<InfoClick>,
    scroll: usize,
) -> usize {
    let modal_open = app.any_modal_open();
    let (max_spans, _) = max_button_spans(app, Pane::Info, area.y, area.x + area.width.saturating_sub(1));
    let block = Block::default()
        .title(title)
        .title_top(Line::from(max_spans.to_vec()).right_aligned())
        .title_style(pane_title_style(modal_open))
        .borders(pane_borders(app))
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.active_pane() == Pane::Info, modal_open));
    let inner = block.inner(area);
    let total = lines.len();
    let viewport = inner.height as usize;
    let scroll = scroll.min(total.saturating_sub(viewport));
    frame.render_widget(block, area);
    // Lines are already wrapped to the inner width, so render them verbatim (no Paragraph wrap)
    // — that keeps line N at row inner.y + (N - scroll), which the click translation below relies on.
    let visible_lines: Vec<Line<'static>> = lines.into_iter().skip(scroll).take(viewport).collect();
    frame.render_widget(Paragraph::new(visible_lines), inner);
    for (line_idx, start, end, action) in clicks {
        if line_idx >= scroll && line_idx < scroll + viewport {
            app.info_click.push((
                inner.y + (line_idx - scroll) as u16,
                inner.x + start,
                inner.x + end,
                action,
            ));
        }
    }
    let track = scrollbar_track(area, inner);
    // Capture geometry for the wheel; render_scrollbar registers the draggable Info hit (it was
    // decorative before — scroll hardcoded to 0 and overflow clipped unreachably).
    app.info_area = area;
    app.info_total = total;
    app.info_viewport = viewport;
    render_scrollbar(frame, app, track, scroll, total, viewport, ScrollKind::Info);
    scroll
}

/// Convert a string that may contain ANSI escape codes to a ratatui Line.
/// We use a simple parser for the common SGR codes git produces.
pub(crate) fn ansi_line_to_ratatui(line: &str) -> Line<'static> {
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

pub(crate) fn apply_sgr(style: Style, code_str: &str) -> Style {
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

pub(crate) fn build_result_summary(app: &AppState) -> Vec<String> {
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
pub(crate) fn build_error_summary(app: &AppState) -> Vec<String> {
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
pub(crate) fn build_folder_summary(app: &AppState, node_idx: usize) -> Vec<String> {
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

pub(crate) fn build_group_summary(app: &AppState, group_idx: usize) -> Vec<String> {
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


#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Vec<(String, Style)>]) -> Vec<String> {
        lines.iter().map(|segs| segs.iter().map(|(text, _)| text.clone()).collect()).collect()
    }

    #[test]
    fn wrap_markdown_breaks_on_spaces_within_width() {
        let base = Style::default();
        let out = wrap_markdown("the quick brown fox jumps", base, 11);
        let widths: Vec<usize> = plain(&out)
            .iter()
            .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
            .collect();
        assert!(widths.iter().all(|&w| w <= 11), "every line fits: {widths:?}");
        assert_eq!(plain(&out).join(" ").replace("  ", " "), "the quick brown fox jumps");
    }

    #[test]
    fn wrap_markdown_strips_markers_and_styles_runs() {
        let base = Style::default();
        let out = wrap_markdown("a **bold** and `code` end", base, 80);
        // One line; markers gone from the text.
        let text: String = plain(&out).concat();
        assert!(!text.contains('*') && !text.contains('`'), "markers stripped: {text:?}");
        // The bold run carries the BOLD modifier; the code run is recolored.
        let segs = &out[0];
        assert!(segs.iter().any(|(t, s)| t == "bold" && s.add_modifier == Modifier::BOLD));
        assert!(segs.iter().any(|(t, s)| t == "code" && s.fg == Some(Color::Yellow)));
    }

    #[test]
    fn wrap_markdown_hard_splits_overlong_word() {
        let out = wrap_markdown("supercalifragilistic", Style::default(), 5);
        assert!(out.len() > 1);
        assert_eq!(plain(&out).concat(), "supercalifragilistic");
    }
}
