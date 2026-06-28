use super::*;

pub(crate) fn render_list(frame: &mut Frame, app: &mut AppState, area: Rect, tick: u64) -> usize {
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
    // Footer-chip-styled `t cols ▾` / `s sort ⟪col ▲⟫ ▾` triggers on the top border: the mnemonic
    // key (cyan/bold) + a dim label. Click (or press `t`/`s`) to open the dropdown; the current sort
    // + direction rides on the sort trigger. Captured for click hit-testing + dropdown anchoring.
    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::DarkGray);
    // Top-border triggers, `·`-separated and ordered `filter · sort · columns`. All three open a
    // dropdown (`f`/`s`/`t` or a click): `f status ⟪filter⟫ ▾`, `s sort ⟪col ▲⟫ ▾`, `t cols ▾`.
    // The active filter / sort rides on its trigger (mirrors the footer `{status}` reset tag);
    // when the filter is `all` the tag is omitted. The maximize button is the rightmost element.
    // (The `/` name filter is left in the status-bar footer where its active needle lives.)
    let cols_text = "t cols ▾";
    let cols_w = cols_text.chars().count() as u16;
    let sort_tag = format!("⟪{} {}⟫", app.sort_column.label(), app.sort_dir.arrow());
    let sort_label = format!(" sort {sort_tag} ▾");
    let sort_w = (1 + sort_label.chars().count()) as u16;
    let filter_label = match app.status_filter.tag() {
        Some(tag) => format!(" status ⟪{tag}⟫ ▾"),
        None => " status ▾".to_string(),
    };
    let filter_w = (1 + filter_label.chars().count()) as u16;
    let sep_w = 3u16; // " · "
    let (max_spans, chips_end) =
        max_button_spans(app, Pane::List, area.y, area.x + area.width.saturating_sub(1));
    // Place right-to-left from the maximize button: columns (rightmost), then sort, then filter.
    let cols_end = chips_end;
    let cols_start = cols_end.saturating_sub(cols_w);
    let sort_end = cols_start.saturating_sub(sep_w);
    let sort_start = sort_end.saturating_sub(sort_w);
    let filter_end = sort_start.saturating_sub(sep_w);
    let filter_start = filter_end.saturating_sub(filter_w);
    app.list_cols_click = Some((area.y, cols_start, cols_end));
    app.list_sort_click = Some((area.y, sort_start, sort_end));
    app.list_filter_click = Some((area.y, filter_start, filter_end));
    let chips = Line::from(vec![
        Span::styled("f", key_style),
        Span::styled(filter_label.clone(), label_style),
        Span::styled(" · ", label_style),
        Span::styled("s", key_style),
        Span::styled(sort_label.clone(), label_style),
        Span::styled(" · ", label_style),
        Span::styled("t", key_style),
        Span::styled(" cols ▾", label_style),
        Span::raw(" "),
        max_spans[0].clone(),
        max_spans[1].clone(),
    ])
    .right_aligned();
    let block = Block::default()
        .title(title)
        .title_top(chips)
        .title_style(pane_title_style(modal_open))
        .borders(pane_borders(app))
        .border_type(BorderType::Rounded)
        .padding(panel_pad(app))
        .border_style(pane_border_style(app.active_pane() == Pane::List, modal_open));

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
    let show_merged_prs = app.show_merged_prs;
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
            let flash_on = match app.changed_row_effect {
                crate::app::ChangedRowEffect::Off => false,
                crate::app::ChangedRowEffect::Flash => state.flash_on(),
                crate::app::ChangedRowEffect::Highlight => state.flash_active(),
            };
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
                // `#N` (a clickable link, region registered post-render) when a shown PR exists,
                // colored by lifecycle state (green=open, magenta=merged, gray=closed); blank
                // otherwise. Merged/closed PRs only render when the "Merged PRs" setting is on.
                let pr = state.pr.as_ref().filter(|pr| pr.shown(show_merged_prs));
                let text = pr.map(|pr| format!("#{}", pr.number)).unwrap_or_default();
                // The leading separator space stays unstyled so only `#N` is underlined.
                spans.push(Span::raw(" "));
                let style = match pr {
                    Some(pr) => Style::default()
                        .fg(crate::render::preview::pr_state_color(pr.state))
                        .add_modifier(Modifier::UNDERLINED),
                    None => Style::default(),
                };
                spans.push(Span::styled(pad_display(&text, pr_w), style));
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
                // Optional columns can be hidden straight from the tooltip's `[x]`.
                hide_column: sort_column_hideable(sort),
                area: crate::app::TooltipArea::Header,
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
    // Registers a draggable List hit (so a grab scrolls the list instead of the divider beside it).
    render_scrollbar(frame, app, scrollbar_area, scroll, total_items, viewport, ScrollKind::List);

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
                    let has_pr = app.repos[repo_idx]
                        .lock()
                        .unwrap()
                        .pr
                        .as_ref()
                        .is_some_and(|pr| pr.shown(show_merged_prs));
                    if has_pr {
                        // Carry the repo index; clicking opens the PR viewer modal for it.
                        clicks.push((rows_area.y + visible as u16, start, end, repo_idx));
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
                    hide_column: None,
                    area: crate::app::TooltipArea::Count,
                });
            }
        }
    }

    app.list_rows_area = rows_area;
    scroll
}

/// `build_list_header` output: the 2 header lines, the clickable sort-cell regions
/// `(col_start, col_end, column)`, and the favorite column's x-range (when shown).
pub(crate) type ListHeader = (Vec<Line<'static>>, Vec<(u16, u16, SortColumn)>, Option<(u16, u16)>);

/// Build the 2-row repo-list column header: titles aligned to the row column widths with a
/// `▲`/`▼` indicator on the active sort column, plus the clickable sort-cell regions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_list_header(
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
pub(crate) fn group_header_item(
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
pub(crate) fn folder_header_item(
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
pub(crate) fn favorites_header_item(app: &AppState, inner_width: usize, tick: u64) -> ListItem<'static> {
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
pub(crate) fn header_marker(collapsible: bool, collapsed: bool) -> &'static str {
    if !collapsible {
        "  "
    } else if collapsed {
        "▸ "
    } else {
        "▾ "
    }
}

/// The optional list `Column` a sortable header maps to, for the tooltip's `[x]` hide button.
/// `Name` / `Branch` are always shown and return `None` (nothing to hide).
fn sort_column_hideable(sort: SortColumn) -> Option<Column> {
    match sort {
        SortColumn::Name | SortColumn::Branch => None,
        SortColumn::Status => Some(Column::Status),
        SortColumn::AheadBehind => Some(Column::AheadBehind),
        SortColumn::Dirty => Some(Column::Dirty),
        SortColumn::LastCommit => Some(Column::LastCommit),
        SortColumn::Worktrees => Some(Column::Worktrees),
        SortColumn::Branches => Some(Column::Branches),
        SortColumn::Stashes => Some(Column::Stashes),
        SortColumn::PulledCommits => Some(Column::PulledCommits),
        SortColumn::PulledFiles => Some(Column::PulledFiles),
        SortColumn::PullRequest => Some(Column::PullRequest),
    }
}

/// One-line description for a sortable column header (shown as a dwell tooltip).
pub(crate) fn column_header_tooltip(sort: SortColumn) -> &'static str {
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
        SortColumn::PullRequest => "pr — pull request for the current branch (click to open)",
    }
}

/// Status tallies for a set of repos `(running, updated, failed, skipped, throttled)`. Shared by
/// `status_tail_for` (the rendered glyph tail) and the group/folder count-tail tooltip text.
pub(crate) fn header_status_counts(app: &AppState, repos: &[usize]) -> (usize, usize, usize, usize, usize) {
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
pub(crate) fn header_tail_tooltip(app: &AppState, repos: &[usize], total: usize, noun: &str) -> String {
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
pub(crate) fn status_tail_for(
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
pub(crate) fn finish_header_line(
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
pub(crate) const STATUS_COL_W: usize = 14;

/// Short status-column text: the recorded failure/skip qualifier when known ("not found",
/// "auth", "diverged", "ref gone", …), else the plain status label.
pub(crate) fn status_short(state: &RepoState) -> &'static str {
    match state.status {
        RepoStatus::Failed => state.status_note.unwrap_or("failed"),
        RepoStatus::Skipped => "dirty",
        RepoStatus::NoUpstream => state.status_note.unwrap_or("no upstream"),
        ref status => status_label(status),
    }
}

/// The semantic color a status renders with (same mapping as its glyph).
pub(crate) fn status_color(status: &RepoStatus) -> Color {
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
pub(crate) fn status_label(status: &RepoStatus) -> &'static str {
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

