use super::*;

pub(crate) fn build_status_row(
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
pub(crate) fn build_hint_footer(
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
pub(crate) fn modal_border_footer(
    segments: Vec<(String, Style, Option<HintKey>)>,
    modal_area: Rect,
    hint_click: &mut Vec<HintClick>,
) -> Line<'static> {
    let footer_row = modal_area.y + modal_area.height.saturating_sub(1);
    build_hint_footer(segments, modal_area.x + 1, footer_row, hint_click)
}

/// A `key`-styled / `hint`-styled `[key, label]` segment pair for `modal_border_footer`, both
/// clickable as `key`. The common shape for footer chips like `esc close` / `r restart`.
pub(crate) fn footer_chip(key_text: &str, label: &str, key: HintKey) -> [(String, Style, Option<HintKey>); 2] {
    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(Color::DarkGray);
    [
        (key_text.to_string(), key_style, Some(key)),
        (label.to_string(), hint_style, Some(key)),
    ]
}

/// A non-clickable ` · ` separator segment for footer chips.
pub(crate) fn footer_sep() -> (String, Style, Option<HintKey>) {
    (" · ".to_string(), Style::default().fg(Color::DarkGray), None)
}

/// Pack `chips` (each an indivisible segment group) into as many rows as needed so each fits
/// `area.width`, separated by ` · `. Row 0 starts with `prefix`; later rows start flush left.
/// Click regions are registered per row at `base_y + row`. Used by the column picker, which has
/// more chips than fit on one status row.
pub(crate) fn pack_chips_into_rows(
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
pub(crate) fn clip_to_width(text: &str, max: usize) -> String {
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
pub(crate) fn clip_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
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
pub(crate) fn compose_status_row(
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
pub(crate) fn style_footer(
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

pub(crate) fn render_status_bar(frame: &mut Frame, app: &mut AppState, area: Rect) {
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
    let right_version: Vec<(String, Style, Option<Command>)> = vec![(
        concat!("v", env!("CARGO_PKG_VERSION")).to_string(),
        hint,
        Some(Command::ShowChangelog),
    )];
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
pub(crate) fn modal_close_button(modal: Rect) -> (Line<'static>, Option<(u16, u16, u16)>) {
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
pub(crate) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

