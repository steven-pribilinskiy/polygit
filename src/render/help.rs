use super::*;

/// Render the `?` help modal: clickable links, subcommands, flags/env, grouped hotkeys,
/// exit codes, and the repo list (each row clickable to open its remote). Records the
/// screen row of every clickable line into `app.help_links` for mouse hit-testing.
/// Sentinel "url" for the collapsible Notes group header — `render_help` recognizes it and
/// records the toggle row instead of treating it as an openable link.
pub(crate) const TOGGLE_NOTES: &str = "\u{1f}toggle:notes";

/// The content of the help modal's "About" tab — what polygit is, plus grouped, title-only links
/// (the URL shows on hover). `notes_expanded` controls the collapsible Notes group.
pub(crate) fn help_items_about(notes_expanded: bool) -> Vec<(Line<'static>, Option<String>)> {
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
pub(crate) const DESIGN_RADIO_PREFIX: &str = "\u{1f}designradio:";

/// The label + options (text, is-active) for a Design System radio, by the **settings** global row
/// index it mirrors (Theme=5 · Background=6 · Contrast=7 · Selection=8). Owned/`'static` so the
/// `&AppState` read ends before the caller mutably borrows `app.help_design_click`.
pub(crate) fn design_radio_data(app: &AppState, row_idx: usize) -> (&'static str, Vec<(&'static str, bool)>) {
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
pub(crate) fn help_items_design_system(app: &AppState) -> Vec<(Line<'static>, Option<String>)> {
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
pub(crate) const CLI_FLAG_PREFIX: &str = "\u{1f}cliflag:";
pub(crate) const CLI_COPY: &str = "\u{1f}clicopy";

/// The help modal's "CLI & Flags" tab — an interactive command builder. Each flag is a row you
/// toggle (boolean) or fill in (value); the constructed `polygit …` command + a `[Copy]` button
/// sit below the exit codes. Rows carry click sentinels so `render_help` can hit-test them.
pub(crate) fn help_items_cli(builder: &crate::app::CliBuilder) -> Vec<(Line<'static>, Option<String>)> {
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
            "  ↑↓ move · space/enter toggle · type a value (auto-applies) · h help".to_string(),
            meta_style,
        )),
        None,
    ));
    // Every flag is a checkbox row; value flags check on once a value is set. Children
    // (e.g. --no-recursive, --profile-out) indent under their parent. The flag+value body is
    // padded to a fixed width so the help comments line up; `h` hides the help column.
    let body_w = 28usize;
    for (idx, flag) in CLI_FLAGS.iter().enumerate() {
        let selected = idx == builder.selected;
        let cursor = if selected { "> " } else { "  " };
        let on = builder.on.get(idx).copied().unwrap_or(false);
        let value = builder.values.get(idx).cloned().unwrap_or_default();
        let editing = selected && builder.editing.is_some();
        let active = match flag.kind {
            CliFlagKind::Toggle => on,
            _ => editing || !value.is_empty(),
        };
        let indent = if flag.parent.is_some() { "  " } else { "" };
        let body = match flag.kind {
            CliFlagKind::Toggle => format!("{indent}{}", flag.flag),
            CliFlagKind::Value(placeholder) | CliFlagKind::Positional(placeholder) => {
                let shown = if editing {
                    format!("{}\u{2588}", builder.editing.clone().unwrap_or_default())
                } else if value.is_empty() {
                    placeholder.to_string()
                } else {
                    value.clone()
                };
                let label = match flag.kind {
                    CliFlagKind::Positional(_) => "[DIR]",
                    _ => flag.flag,
                };
                format!("{indent}{label} = {shown}")
            }
        };
        let mut spans = vec![
            Span::styled(cursor.to_string(), key_style),
            Span::styled(
                format!("{} ", if active { "[x]" } else { "[ ]" }),
                if active { on_style } else { meta_style },
            ),
            Span::styled(
                format!("{body:<body_w$}"),
                if active { on_style } else { key_style },
            ),
        ];
        if builder.show_help {
            spans.push(Span::styled(format!(" {}", flag.help), meta_style));
        }
        items.push((Line::from(spans), Some(format!("{CLI_FLAG_PREFIX}{idx}"))));
    }
    items.push(plain(""));

    // The constructed command (clickable → copies) + an explicit copy button.
    items.push(header("COMMAND"));
    items.push((
        Line::from(Span::styled(
            format!("  {}", builder.command()),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Some(CLI_COPY.to_string()),
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
pub(crate) fn help_items_legend() -> Vec<(Line<'static>, Option<String>)> {
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
pub(crate) enum HelpView {
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
pub(crate) fn filter_help_items(
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

pub(crate) fn help_items_hotkeys(view: HelpView) -> Vec<(Line<'static>, Option<String>)> {
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

pub(crate) fn render_help(frame: &mut Frame, app: &mut AppState, area: Rect) {
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
        ("Design", HelpTab::DesignSystem),
        ("About", HelpTab::About),
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
pub(crate) fn center_cell(label: &str, width: u16) -> String {
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
pub(crate) fn render_keyboard_modal(frame: &mut Frame, app: &mut AppState, area: Rect) {
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

