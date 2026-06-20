use super::*;

/// Render the yes/no confirmation dialog (keyboard-driven: y / n / Esc).
/// Render the build-info modal (opened by clicking the "built … ago" status tag): the running
/// version, the watched executable path, when it was built, and how new-build watching works.
/// Render an open header dropdown (`[cols ▾]` / `[sort ▾]`): a small floating list anchored under
/// the chip, with checkboxes (columns) or radios (sort). Captures item + close click regions.
pub(crate) fn render_dropdown(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let Some(dropdown) = app.dropdown else {
        return;
    };
    let items = app.dropdown_items();
    let is_sort = matches!(dropdown.kind, DropdownKind::ListSort | DropdownKind::PageSort);
    let title = match dropdown.kind {
        DropdownKind::ListColumns | DropdownKind::PageColumns => " columns ",
        DropdownKind::ListSort | DropdownKind::PageSort => " sort ",
    };
    let inner_w = items.iter().map(|(label, _)| label.chars().count()).max().unwrap_or(6) + 4;
    let width = (inner_w as u16 + 2).clamp(14, area.width.saturating_sub(2).max(14));
    let height = (items.len() as u16 + 2).min(area.height.saturating_sub(2).max(3));
    // Below the chip, flipping above when there's no room.
    let below = dropdown.anchor_row + 1;
    let y = if below + height <= area.y + area.height {
        below
    } else {
        dropdown.anchor_row.saturating_sub(height)
    };
    let x = dropdown.anchor_col.min((area.x + area.width).saturating_sub(width));
    let modal = Rect { x, y, width, height };
    let (close_line, close_click) = modal_close_button(modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title)
        .title_top(close_line);
    let body = block.inner(modal);
    cast_shadow(frame, modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);
    app.dropdown_area = modal;
    app.dropdown_close_click = close_click;
    app.dropdown_item_click.clear();
    let mut lines: Vec<Line> = Vec::new();
    for (index, (label, on)) in items.iter().enumerate() {
        if index as u16 >= body.height {
            break;
        }
        let marker = if is_sort {
            if *on { "● " } else { "○ " }
        } else if *on {
            "[x] "
        } else {
            "[ ] "
        };
        let selected = index == dropdown.selected;
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if *on {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Gray)
        };
        let row = body.y + index as u16;
        app.dropdown_item_click.push((row, body.x, body.x + body.width, index));
        lines.push(Line::from(Span::styled(format!("{marker}{label}"), style)));
    }
    frame.render_widget(Paragraph::new(lines), body);
}

/// Human-readable byte size (e.g. `1.2 MB`).
pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Lightweight JSON syntax highlighting for one line: keys cyan, string values green, numbers /
/// booleans / null yellow, punctuation dim. Heuristic (per-line), good enough for a preview.
pub(crate) fn highlight_json_line(line: &str) -> Line<'static> {
    let key = Style::default().fg(Color::Cyan);
    let string = Style::default().fg(Color::Green);
    let number = Style::default().fg(Color::Yellow);
    let punct = Style::default().fg(Color::DarkGray);
    let plain = Style::default().fg(Color::Gray);
    let chars: Vec<char> = line.chars().collect();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            // Consume the whole string literal (respecting `\"`).
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == '\\' {
                    index += 2;
                    continue;
                }
                if chars[index] == '"' {
                    index += 1;
                    break;
                }
                index += 1;
            }
            let text: String = chars[start..index.min(chars.len())].iter().collect();
            // A string followed by `:` (ignoring spaces) is an object key.
            let mut peek = index;
            while peek < chars.len() && chars[peek] == ' ' {
                peek += 1;
            }
            let is_key = peek < chars.len() && chars[peek] == ':';
            spans.push(Span::styled(text, if is_key { key } else { string }));
        } else if ch.is_ascii_digit() || (ch == '-' && chars.get(index + 1).is_some_and(|next| next.is_ascii_digit())) {
            let start = index;
            index += 1;
            while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.') {
                index += 1;
            }
            spans.push(Span::styled(chars[start..index].iter().collect::<String>(), number));
        } else if chars[index..].iter().collect::<String>().starts_with("true")
            || chars[index..].iter().collect::<String>().starts_with("false")
            || chars[index..].iter().collect::<String>().starts_with("null")
        {
            let word = if chars[index..].iter().collect::<String>().starts_with("false") {
                "false"
            } else if chars[index..].iter().collect::<String>().starts_with("true") {
                "true"
            } else {
                "null"
            };
            spans.push(Span::styled(word.to_string(), number));
            index += word.len();
        } else if matches!(ch, '{' | '}' | '[' | ']' | ':' | ',') {
            spans.push(Span::styled(ch.to_string(), punct));
            index += 1;
        } else {
            spans.push(Span::styled(ch.to_string(), plain));
            index += 1;
        }
    }
    Line::from(spans)
}

pub(crate) fn render_build_info(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let built = app
        .binary_built
        .and_then(|built| built.elapsed().ok())
        .map(|age| crate::app::format_ago(age.as_secs()))
        .unwrap_or_else(|| "unknown".to_string());
    let label = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
    let value = Style::default().fg(Color::Gray);
    let dim = Style::default().fg(Color::DarkGray);
    let field = |name: &str, text: String| {
        Line::from(vec![Span::styled(format!("{name:<10}"), label), Span::styled(text, value)])
    };

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
    let header: Vec<Line> = vec![
        field("Version", concat!("v", env!("CARGO_PKG_VERSION")).to_string()),
        field("Built", built),
        field("Binary", format!("{} ({})", human_size(app.build_info_binary_size), app.exe_path)),
        field(
            "Settings",
            format!("{}  ({} files in config)", app.build_info_settings_path, app.build_info_config_count),
        ),
        Line::from(status),
        Line::from(String::new()),
        Line::from(Span::styled("Settings preview (state.json)", label)),
    ];

    // A roomy modal: header + a scrollable JSON preview filling the rest.
    let pad = if app.panel_padding { 2 } else { 0 };
    let width = area.width.saturating_sub(8).clamp(40, 100);
    let height = area.height.saturating_sub(4).clamp(12, 36);
    let modal = centered_rect(width, height, area);
    let (close_line, close_click) = modal_close_button(modal);
    let mut footer: Vec<(String, Style, Option<HintKey>)> = Vec::new();
    footer.extend(footer_chip("j/k", " scroll", HintKey::Char('j')));
    footer.push(footer_sep());
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

    // Header rows at the top, then the scrollable preview in whatever's left.
    let header_h = header.len() as u16;
    let header_area = Rect { height: header_h.min(inner.height), ..inner };
    frame.render_widget(Paragraph::new(header).wrap(Wrap { trim: false }), header_area);
    if inner.height <= header_h + 1 {
        return;
    }
    let preview = Rect {
        y: inner.y + header_h,
        height: inner.height - header_h,
        // Leave the last column for a scrollbar.
        width: inner.width.saturating_sub(1),
        ..inner
    };
    let total = app.build_info_settings_preview.len();
    let viewport = preview.height as usize;
    let max_scroll = total.saturating_sub(viewport);
    if app.build_info_scroll > max_scroll {
        app.build_info_scroll = max_scroll;
    }
    let start = app.build_info_scroll;
    let visible: Vec<Line> = app.build_info_settings_preview[start..(start + viewport).min(total)]
        .iter()
        .map(|line| highlight_json_line(line))
        .collect();
    frame.render_widget(Paragraph::new(visible), preview);
    let track = Rect { x: preview.x + preview.width, width: 1, ..preview };
    render_scrollbar(frame, track, app.build_info_scroll, total, viewport, false);
    let _ = pad;
}

pub(crate) fn render_confirm(frame: &mut Frame, app: &mut AppState, area: Rect) {
    let Some(confirm) = &app.confirm else {
        return;
    };
    // Cap how many files we enumerate so a huge dirty tree can't overflow the screen.
    let max_per_list = 10usize;
    let has_files = !confirm.restore_files.is_empty()
        || !confirm.delete_files.is_empty()
        || !confirm.detail_lines.is_empty();

    // Widen to fit the longest file / detail line (with its two-space indent).
    let file_width = confirm
        .restore_files
        .iter()
        .chain(confirm.delete_files.iter())
        .chain(confirm.detail_lines.iter())
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
    // Generic detail lines (e.g. the settings a reset will change), under an optional header.
    if !confirm.detail_lines.is_empty() {
        if let Some(title) = &confirm.detail_title {
            detail_lines.push(Line::from(Span::styled(
                format!("  {title}"),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
        }
        for line in confirm.detail_lines.iter().take(max_per_list) {
            detail_lines.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(Color::Gray),
            )));
        }
        if confirm.detail_lines.len() > max_per_list {
            detail_lines.push(Line::from(Span::styled(
                format!("    … and {} more", confirm.detail_lines.len() - max_per_list),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

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
pub(crate) const SETTINGS_LABEL_W: u16 = 22;

/// Render one settings row — `> Label   ● value  ○ value` — and capture its label/chip click
/// regions (keyed by the global `row_idx`). `left_x` is the row's left edge.
/// The option index to underline for a radio row (Theme only): when `auto` is selected, underline
/// the autodetected option it resolves to (`dark`=1 / `light`=2). `None` for every other row/state.
pub(crate) fn radio_underline_idx(app: &AppState, row_idx: usize) -> Option<usize> {
    if row_idx == 5 && app.theme == crate::app::Theme::Auto {
        Some(if app.auto_dark { 1 } else { 2 })
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn settings_row_line(
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
pub(crate) fn render_settings(frame: &mut Frame, app: &mut AppState, area: Rect) {
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
    // (Interaction), 16 padding · 17 borders · 18 splitter · 19 repo-page tabs ·
    // 20 repo-page (restored/maximized) · 21 branch-check (Layout).
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
                    "Repo page",
                    vec![
                        ("restored", !app.repo_page_maximized),
                        ("maximized", app.repo_page_maximized),
                    ],
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
    footer.extend(footer_chip("R", " reset", HintKey::Char('R')));
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
        // A logical line in the accordion (built as metadata first, so we can scroll the window
        // and only capture click regions for the visible lines).
        enum AccItem {
            CollapseAll,
            Blank,
            Header(usize),
            Row(usize),
            GroupsHint,
        }
        let groups_empty = app.groups.is_empty();
        let mut items: Vec<AccItem> = vec![AccItem::CollapseAll, AccItem::Blank];
        let mut row_idx = 0usize;
        for (tab_idx, (_, count)) in SETTINGS_TABS.iter().enumerate() {
            if tab_idx > 0 {
                items.push(AccItem::Blank);
            }
            items.push(AccItem::Header(tab_idx));
            let collapsed = section_collapsed[tab_idx];
            for offset in 0..*count {
                if !collapsed {
                    items.push(AccItem::Row(row_idx));
                    // The "no groups defined" hint sits under the Grouping row (global row 0).
                    if row_idx == 0 && offset == 0 && groups_empty {
                        items.push(AccItem::GroupsHint);
                    }
                }
                row_idx += 1;
            }
        }
        // Scroll the window to keep the selected position (header or row) visible.
        let selection = app.accordion_selection();
        let sel_line = items.iter().position(|item| match (item, selection) {
            (AccItem::Header(section), crate::app::AccPos::Header(target)) => section == &target,
            (AccItem::Row(row), crate::app::AccPos::Row(target)) => row == &target,
            _ => false,
        });
        let viewport = inner.height as usize;
        let mut scroll = app.settings_scroll.min(items.len().saturating_sub(viewport));
        if let Some(sel) = sel_line {
            if sel < scroll {
                scroll = sel;
            } else if viewport > 0 && sel >= scroll + viewport {
                scroll = sel + 1 - viewport;
            }
        }
        app.settings_scroll = scroll;
        app.settings_collapse_all_click = None;
        let active_header = Style::default()
            .fg(Color::Black)
            .bg(Color::LightCyan)
            .add_modifier(Modifier::BOLD);
        let mut lines: Vec<Line> = Vec::new();
        for (offset, item) in items.iter().skip(scroll).take(viewport).enumerate() {
            let screen_y = inner.y + offset as u16;
            match item {
                AccItem::CollapseAll => {
                    let btn_label = if all_collapsed { "[+ expand all]" } else { "[- collapse all]" };
                    let btn_w = UnicodeWidthStr::width(btn_label) as u16;
                    app.settings_collapse_all_click =
                        Some((screen_y, inner.x + 2, inner.x + 2 + btn_w));
                    lines.push(Line::from(Span::styled(
                        format!("  {btn_label}"),
                        Style::default().fg(Color::Cyan),
                    )));
                }
                AccItem::Blank => lines.push(Line::from(String::new())),
                AccItem::Header(tab_idx) => {
                    let (name, _) = SETTINGS_TABS[*tab_idx];
                    let collapsed = section_collapsed[*tab_idx];
                    let chevron = if collapsed { "\u{25b8}" } else { "\u{25be}" }; // ▸ / ▾
                    let active = app.settings_on_header == Some(*tab_idx);
                    let header = format!(" {chevron} {name} ");
                    let header_w = UnicodeWidthStr::width(header.as_str()) as u16;
                    app.settings_section_click.push((screen_y, inner.x, inner.x + header_w, *tab_idx));
                    let style = if active { active_header } else { section_style };
                    lines.push(Line::from(Span::styled(header, style)));
                }
                AccItem::Row(row) => {
                    let (label, options) = &all_rows[*row];
                    let underline_idx = if *row == 5 { theme_underline } else { None };
                    let disabled = *row == 4 && emoji_icons;
                    lines.push(settings_row_line(
                        *row,
                        app.settings_on_header.is_none() && app.settings_selected == *row,
                        label,
                        options,
                        (inner.x, screen_y),
                        true,
                        underline_idx,
                        disabled,
                        None,
                        &mut app.settings_click,
                    ));
                }
                AccItem::GroupsHint => lines.push(Line::from(Span::styled(
                    "      no groups defined — ~/.config/polygit/groups.json",
                    Style::default().fg(Color::DarkGray),
                ))),
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);
        // A scrollbar when the content overflows the modal.
        if items.len() > viewport {
            let track = Rect { x: inner.x + inner.width.saturating_sub(1), width: 1, ..inner };
            render_scrollbar(frame, track, scroll, items.len(), viewport, false);
        }
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
pub(crate) fn render_update_notice(frame: &mut Frame, app: &mut AppState, area: Rect, tick: u64) {
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
pub(crate) fn render_throttle_banner(frame: &mut Frame, app: &AppState, area: Rect) {
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
pub(crate) fn render_toast(frame: &mut Frame, app: &AppState, area: Rect) {
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
pub(crate) fn render_copy_menu(frame: &mut Frame, app: &mut AppState, area: Rect) {
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
pub(crate) fn render_base_picker(frame: &mut Frame, app: &mut AppState, area: Rect) {
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
