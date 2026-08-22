    use super::*;

    /// A glyph the terminal draws past its cell has to carry the cell it spills
    /// onto, inside its own span. Otherwise the ink lands on whatever style the
    /// next span brought — which is how a magenta `⧉` came out half in its
    /// hover highlight and half on the panel behind it.
    #[test]
    fn an_overflowing_glyph_carries_the_cell_its_ink_lands_on() {
        assert!(draws_past_its_cell("\u{29c9}"));
        assert_eq!(icon_cols("\u{29c9}"), 2);
        assert_eq!(icon_span("\u{29c9}", Style::default()).content.as_ref(), "\u{29c9} ");

        // One column, one cell of ink: nothing to pad, and padding it would
        // leave a gap and push everything after it a column right.
        assert!(!draws_past_its_cell("\u{25a2}"));
        assert_eq!(icon_cols("\u{25a2}"), 1);
        assert_eq!(icon_span("\u{25a2}", Style::default()).content.as_ref(), "\u{25a2}");

        // Genuinely two columns, and `unicode-width` agrees, so it needs no
        // help — the budget is already the size of the glyph.
        assert!(!draws_past_its_cell("\u{1f4cb}"));
        assert_eq!(icon_cols("\u{1f4cb}"), 2);
    }

    /// The emoji window controls are the trap the icon-set rule does not catch:
    /// single codepoints, which is what that rule tests, but East_Asian_Width N
    /// — so `unicode-width` reports 1 for a glyph that inks 1.71 cells, and the
    /// comment beside them claiming "2 cells" was measuring nothing.
    #[test]
    fn the_emoji_window_controls_are_not_two_cells_however_they_look() {
        for glyph in ["\u{1f5d6}", "\u{1f5d7}", "\u{1f3f7}"] {
            assert_eq!(
                UnicodeWidthStr::width(glyph),
                1,
                "{glyph} is EAW=N, whatever it looks like"
            );
            assert!(draws_past_its_cell(glyph), "{glyph} inks past its cell");
            assert_eq!(icon_cols(glyph), 2);
        }

        // These three really are two columns and are left alone.
        for glyph in ["\u{1f4cb}", "\u{1f517}", "\u{274c}"] {
            assert_eq!(UnicodeWidthStr::width(glyph), 2, "{glyph}");
            assert!(!draws_past_its_cell(glyph), "{glyph} needs no pad");
        }
    }

    /// Every glyph either icon set can put in a fixed-width column must fit the
    /// budget that column reserves. The favourites column is the one with no
    /// slack: `pad_display` cannot add the overflow cell at width 1, because the
    /// glyph already fills it and the function returns early.
    #[test]
    fn a_fixed_width_icon_column_reserves_the_cells_its_glyph_inks() {
        for icons in [&crate::app::UNICODE_ICONS, &crate::app::EMOJI_ICONS] {
            for glyph in [icons.fav_on, icons.fav_off] {
                assert!(
                    icon_cols(glyph) <= 2,
                    "{glyph} needs more than the favourites column reserves"
                );
                assert_eq!(
                    pad_display(glyph, 2).chars().count() >= 2,
                    true,
                    "{glyph} must be padded to the column width"
                );
            }
        }
    }

    /// The table is searched by binary search and guarded by a floor, and a
    /// misplaced entry fails neither loudly — it is simply never found, and the
    /// glyph goes back to inking onto its neighbour.
    #[test]
    fn the_overflow_table_is_sorted_and_its_guard_matches() {
        assert!(
            DRAWN_PAST_THEIR_CELL.windows(2).all(|pair| pair[0] < pair[1]),
            "DRAWN_PAST_THEIR_CELL must be sorted by codepoint"
        );
        assert_eq!(
            FIRST_OVERFLOWING,
            *DRAWN_PAST_THEIR_CELL.first().expect("at least one entry")
        );
    }

    /// The Design-tab theming radios must stay keyed to their real settings rows, and the settings
    /// modal's per-row flags (the Theme autodetect underline, the emoji "Hide zeros" disable) must
    /// be matched by LABEL — not a hardcoded index. Regression: when settings sections were sorted
    /// alphabetically / a row was inserted, the magic indices drifted so the emoji "Hide zeros"
    /// disable landed on Theme (greying it out) and the Design tab's Theme chip wrote Background.
    #[test]
    fn settings_rows_are_resolved_by_label_not_drifting_indices() {
        use crate::app::{settings_row, SETTINGS_LABELS};
        // Every label the render + Design tab key on must resolve to a real, distinct row.
        for label in ["Hide zeros", "Theme", "Background", "Contrast", "List selection"] {
            let idx = settings_row(label);
            assert_ne!(idx, usize::MAX, "settings row {label:?} must exist");
            assert_eq!(SETTINGS_LABELS[idx], label, "{label:?} index round-trips");
        }
        assert_ne!(settings_row("Hide zeros"), settings_row("Theme"));
        assert_eq!(settings_row("does not exist"), usize::MAX);

        // The Design tab pulls its radios by label; each shows its own field's values and routes a
        // click (via settings_row(label)) to that same field.
        let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
            "demo",
            std::path::PathBuf::from("/tmp/demo"),
        )))];
        let mut app = AppState::new(repos, Some(4), true);
        for (key, display) in
            [("Theme", "Theme"), ("Background", "Background"), ("Contrast", "Contrast"), ("List selection", "Selection")]
        {
            let (label, options) = design_radio_data(&app, key);
            assert_eq!(label, display, "design radio for {key:?} displays {display:?}");
            assert!(!options.is_empty());
        }
        // Theme is the only radio with the auto-detect underline; it points at dark or light.
        app.theme = crate::app::Theme::Auto;
        assert!(theme_autodetect_underline(&app).is_some());
        app.theme = crate::app::Theme::Dark;
        assert!(theme_autodetect_underline(&app).is_none());
    }

    /// The user-reported bug, end to end: in EMOJI mode the settings "Theme" row must stay
    /// interactive (clickable option chips) while "Hide zeros" is the disabled/inert one — a
    /// disabled row registers no click regions. Renders the real settings modal and inspects them.
    #[test]
    fn emoji_mode_disables_hide_zeros_not_theme() {
        use crate::app::{settings_row, IconStyle, SettingsLayout};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
            "demo",
            std::path::PathBuf::from("/tmp/demo"),
        )))];
        let mut app = AppState::new(repos, Some(4), true);
        app.icon_style = IconStyle::Emoji; // emoji always hides zeros → Hide zeros is inert
        app.show_settings = true;
        app.settings_layout = SettingsLayout::Flat;
        // Select Theme so it (and the adjacent Hide zeros) scroll into view.
        app.settings_selected = settings_row("Theme");
        app.settings_ensure_visible = true;

        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|frame| crate::render::render(frame, &mut app, 0)).unwrap();

        let theme_row = settings_row("Theme");
        let hide_zeros_row = settings_row("Hide zeros");
        let has_clicks = |row: usize| app.settings_click.iter().any(|&(.., r, _)| r == row);
        assert!(has_clicks(theme_row), "Theme must be interactive (have click regions) in emoji mode");
        assert!(!has_clicks(hide_zeros_row), "Hide zeros must be disabled (no click regions) in emoji mode");
    }

    #[test]
    fn kebab_glyph_appears_on_hovered_repo_row() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
            "demo",
            std::path::PathBuf::from("/tmp/demo"),
        )))];
        let mut app = AppState::new(repos, Some(4), true);
        // Render the bare list deterministically: close any auto-popped What's New modal (a version
        // bump would otherwise overlay the list and hide the kebab), no grouping/tree, hover on.
        app.close_all_modals();
        app.grouping_enabled = false;
        app.tree_enabled = false;
        app.hover_effects = true;
        let mut term = Terminal::new(TestBackend::new(100, 20)).unwrap();
        // First render captures list_rows_area; then hover the first repo row and re-render.
        term.draw(|frame| crate::render::render(frame, &mut app, 0)).unwrap();
        let geom = app.list_rows_area;
        // Hover the first repo row. The hovered-row derivation reads LAST frame's geometry
        // (`list_rows_area_prev`), which the per-frame reset must preserve — regression: the reset
        // wiped it to empty, so the kebab/hover-★ never resolved a row.
        app.hover = Some((geom.x + 2, geom.y));
        term.draw(|frame| crate::render::render(frame, &mut app, 0)).unwrap();
        let buf = term.backend().buffer().clone();
        let found = (0..20u16)
            .flat_map(|y| (0..100u16).map(move |x| (x, y)))
            .any(|(x, y)| buf[(x, y)].symbol() == "\u{22ee}");
        assert!(found, "kebab ⋮ must render on the hovered repo row");
    }

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
            diff_modal_footer(source, focus, chips, crate::app::DiffView::Raw, true)
                .iter()
                .map(|(text, _, _)| text.as_str())
                .collect()
        };
        // A pane that fits advertises no scrolling or paging: the keys do nothing there.
        let fits = |focus: DiffFocus| -> String {
            let source = DiffSource::Branch { path: "/tmp".into(), name: "b".into() };
            diff_modal_footer(&source, focus, false, crate::app::DiffView::Raw, false)
                .iter()
                .map(|(text, _, _)| text.as_str())
                .collect()
        };
        for focus in [DiffFocus::Files, DiffFocus::Diff] {
            let footer = fits(focus);
            assert!(!footer.contains("scroll"), "{focus:?} fits, so no scroll hint: {footer:?}");
            assert!(!footer.contains("page"), "{focus:?} fits, so no paging hint: {footer:?}");
            // `tab` still switches panes, and in the file list `j/k` still picks.
            assert!(footer.contains("tab"), "{focus:?}: {footer:?}");
        }
        assert!(fits(DiffFocus::Files).contains("j/k pick"), "picking survives a list that fits");
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

    #[test]
    fn human_size_scales_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn highlight_json_line_colors_keys_strings_numbers() {
        let line = highlight_json_line("  \"theme\": \"dark\", \"n\": 42");
        let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "  \"theme\": \"dark\", \"n\": 42");
        // The first string is a key (cyan), the second a value (green), 42 a number (yellow).
        let key = line.spans.iter().find(|span| span.content.contains("theme")).unwrap();
        assert_eq!(key.style.fg, Some(Color::Cyan));
        let val = line.spans.iter().find(|span| span.content.contains("dark")).unwrap();
        assert_eq!(val.style.fg, Some(Color::Green));
        let num = line.spans.iter().find(|span| span.content.contains("42")).unwrap();
        assert_eq!(num.style.fg, Some(Color::Yellow));
    }

    // Every binding in `keymap.json` must appear in the grouped help for its view — so a new (or
    // renamed) hotkey can't silently fall out of the `?` Hotkeys list. Guards the group layout: a
    // binding tagged with a group not in `help_group_order` still renders (appended), and this
    // asserts it. (The PR-modal section has no help view, so it's excluded.)
    #[test]
    fn help_covers_every_binding() {
        for view in [HelpView::List, HelpView::RepoPage, HelpView::DiffModal, HelpView::Explorer] {
            let id = help_section_id(view);
            let section = crate::keymap::sections().iter().find(|section| section.id == id).unwrap();
            let rendered: String = help_items_hotkeys(view, 56)
                .iter()
                .map(|(line, _)| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            for binding in &section.bindings {
                // The grouped two-column layout truncates long actions with `…` to keep columns
                // narrow (the full text lives in the `K` keyboard viewer + docs). The compact view
                // always renders at least the first 14 chars, so assert that prefix is present —
                // enough to prove every binding has a row without depending on the cap width.
                let prefix: String = binding.action.chars().take(14).collect();
                assert!(
                    rendered.contains(&prefix),
                    "help for `{id}` is missing binding {:?} ({})",
                    binding.keys,
                    binding.action
                );
            }
        }
    }

    /// A one-repo app with a pending release, rendered to a `TestBackend`; returns every visible
    /// row so a test can assert on what actually reached the screen.
    fn render_rows(app: &mut AppState, width: u16, height: u16) -> Vec<String> {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|frame| crate::render::render(frame, app, 0)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect()
    }

    fn app_with_release() -> AppState {
        let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
            "demo",
            std::path::PathBuf::from("/tmp/demo"),
        )))];
        let mut app = AppState::new(repos, Some(4), true);
        // A fresh state on a version with an unseen changelog auto-pops the What's New modal, which
        // would overlay the surfaces under test — dismiss it, like the repo/pull test helpers do.
        app.close_all_modals();
        app.latest_release = Some(("99.0.0".to_string(), "2026-07-14".to_string()));
        app
    }

    /// A notified release is actionable in place: the notice names the version and offers the
    /// install, and the button registers a click region so the mouse path works too.
    #[test]
    fn release_notice_offers_install_and_reload() {
        let mut app = app_with_release();
        let rows = render_rows(&mut app, 160, 40).join("\n");
        assert!(rows.contains("v99.0.0 available"), "the notice names the version\n{rows}");
        assert!(rows.contains("[^R install & reload]"), "and offers the action\n{rows}");
        assert!(app.update_reload_click.is_some(), "the action registers a click region");
        assert!(app.update_close_click.is_some(), "so does the dismiss button");
    }

    /// A binary already staged on disk wins over a published release — its install is done, so the
    /// reload is immediate and must not be relabelled as a download.
    #[test]
    fn staged_build_notice_wins_over_release_notice() {
        let mut app = app_with_release();
        app.update_available = true;
        let rows = render_rows(&mut app, 160, 40).join("\n");
        assert!(rows.contains("new build installed"), "the staged build takes the notice\n{rows}");
        assert!(!rows.contains("install & reload"), "and not the download wording\n{rows}");
    }

    /// Dismissing the notice takes it off screen, but leaves nothing else to act on — the Settings
    /// line stays (it's the place you go to look one up).
    #[test]
    fn dismissed_release_notice_leaves_the_screen() {
        let mut app = app_with_release();
        app.release_dismissed = Some("99.0.0".to_string());
        let rows = render_rows(&mut app, 160, 40).join("\n");
        assert!(!rows.contains("v99.0.0 available"), "dismissed notices don't render\n{rows}");
        assert!(app.update_reload_click.is_none(), "and leave no stale click region");
    }

    /// Settings > Updates surfaces the release under its options, as a clickable line.
    #[test]
    fn settings_updates_section_offers_the_release() {
        use crate::app::SettingsLayout;
        let mut app = app_with_release();
        app.show_settings = true;
        app.settings_layout = SettingsLayout::Tabbed;
        // Open the tab that owns the Updates rows, so the hint's row is on screen.
        let updates_tab = AppState::settings_tab_of_row(crate::app::settings_row("Update check"));
        app.settings_select_tab(updates_tab);
        let rows = render_rows(&mut app, 160, 40).join("\n");
        assert!(
            rows.contains("↑ v99.0.0 available — click to install"),
            "the Updates section names the release, unclipped\n{rows}"
        );
        assert!(app.settings_release_click.is_some(), "and the line is clickable");
    }

    /// Build info can both find an update (`[check now]`, past the cadence gate) and apply one.
    #[test]
    fn build_info_offers_check_and_install_buttons() {
        let mut app = app_with_release();
        app.show_build_info = true;
        let rows = render_rows(&mut app, 160, 40).join("\n");
        assert!(rows.contains("[check now]"), "a manual check is always offered\n{rows}");
        assert!(rows.contains("[install & reload]"), "and the pending release is applyable\n{rows}");
        assert!(app.build_info_check_click.is_some(), "check button is clickable");
        assert!(app.build_info_install_click.is_some(), "install button is clickable");
    }

    /// Render the PR viewer to a `TestBackend` and return (every visible row, the title-bar row).
    /// The title bar is the modal's top border row (row 1 — the panes' own borders are row 0).
    fn render_pr_modal_rows(app: &mut AppState, width: u16, height: u16) -> (Vec<String>, String) {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|frame| crate::render::render(frame, app, 0)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut rows = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buf[(x, y)].symbol());
            }
            rows.push(line);
        }
        let title_bar = rows.get(1).cloned().unwrap_or_default();
        (rows, title_bar)
    }

    fn demo_pr_modal() -> AppState {
        use crate::app::{PrModalState, PrSection, PrView};
        let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
            "demo",
            std::path::PathBuf::from("/tmp/demo"),
        )))];
        let mut app = AppState::new(repos, Some(4), true);
        let title = "fix(widget): skeleton rows while the next page loads during infinite scroll so \
                     the table never shows a bare spinner anywhere"
            .to_string();
        let view = PrView {
            title: title.clone(),
            url: "https://example/pr/426".to_string(),
            state: "open".to_string(),
            head: "fix/next-page-skeletons".to_string(),
            base: "dev".to_string(),
            author: "demo-user".to_string(),
            created: "2026-06-23T14:32:10Z".to_string(),
            additions: 361,
            deletions: 24,
            labels: vec!["reviewed".to_string()],
            description: "Tables now show skeleton rows while the next page loads.".to_string(),
            comments: vec![PrSection {
                author: "reviewer".to_string(),
                kind: "approved".to_string(),
                day: "2026-06-24".to_string(),
                body: "LGTM".to_string(),
            }],
            commits: vec![crate::app::PrCommit {
                sha: "abc1234".to_string(),
                subject: "fix: skeleton rows".to_string(),
                author: "demo-user".to_string(),
                day: "2026-06-23".to_string(),
            }],
            files: vec![crate::app::PrFile {
                path: "src/table.rs".to_string(),
                additions: 40,
                deletions: 2,
            }],
            checks: vec![crate::app::PrCheck {
                name: "tests".to_string(),
                bucket: "pass".to_string(),
                state: "SUCCESS".to_string(),
                link: "https://example/checks/1".to_string(),
            }],
        };
        app.pr_modal = Some(PrModalState {
            repo_idx: 0,
            number: 426,
            url: "https://example/pr/426".to_string(),
            title,
            view: Some(view),
            scroll: 0,
            collapsed: std::collections::HashSet::new(),
            search: String::new(),
            search_focused: false,
            tab: crate::app::PrModalTab::default(),
            files_diff: None,
            files_diff_loading: false,
            files_view: crate::app::DiffView::Unified,
        });
        app
    }

    // The pinned meta line renders the created date relative ("ago") with the absolute on hover; the
    // title bar always carries `PR #N · <title>`; and the default Description tab shows the body.
    #[test]
    fn pr_modal_shows_meta_timeago_and_title() {
        let mut app = demo_pr_modal();
        let (rows, title_bar) = render_pr_modal_rows(&mut app, 200, 24);
        let body = rows.join("\n");
        assert!(body.contains(" ago"), "created renders as a relative 'time ago' label\n{body}");
        assert!(!body.contains("2026-06-23"), "the raw date is not shown inline (only on hover)");
        let region = app.pr_created_region.clone().expect("created region captured (meta is pinned)");
        assert_eq!(region.3, "2026-06-23 14:32 UTC");
        assert!(title_bar.contains("PR #426"), "title bar shows the number\n{title_bar}");
        assert!(title_bar.contains("fix(widget)"), "title bar carries the title\n{title_bar}");
        assert!(
            body.contains("skeleton rows while the next page loads"),
            "the default Description tab shows the body\n{body}"
        );
    }

    // The tab bar switches the body: Conversation shows comments, Commits/Checks/Files their data.
    #[test]
    fn pr_modal_tabs_switch_the_body() {
        let mut app = demo_pr_modal();
        let body = render_pr_modal_rows(&mut app, 200, 24).0.join("\n");
        assert!(body.contains("Description"), "tab bar lists Description\n{body}");
        assert!(body.contains("Conversation 1"), "Conversation badge counts the comment\n{body}");

        app.pr_modal_select_tab(crate::app::PrModalTab::Conversation);
        let body = render_pr_modal_rows(&mut app, 200, 24).0.join("\n");
        assert!(body.contains("@reviewer") && body.contains("LGTM"), "Conversation shows the comment\n{body}");

        app.pr_modal_select_tab(crate::app::PrModalTab::Commits);
        let body = render_pr_modal_rows(&mut app, 200, 24).0.join("\n");
        assert!(body.contains("abc1234"), "Commits lists the commit sha\n{body}");

        app.pr_modal_select_tab(crate::app::PrModalTab::Checks);
        let body = render_pr_modal_rows(&mut app, 200, 24).0.join("\n");
        assert!(body.contains("tests"), "Checks lists the check name\n{body}");

        app.pr_modal_select_tab(crate::app::PrModalTab::Files);
        let body = render_pr_modal_rows(&mut app, 200, 24).0.join("\n");
        assert!(body.contains("src/table.rs"), "Files lists the changed file\n{body}");
    }

    // settings_search_items interleaves an inert section-title header before each run of matches
    // from a new section, walking the (already ascending/alphabetical) filtered row list.
    #[test]
    fn settings_search_items_empty_input_is_empty() {
        assert_eq!(settings_search_items(&[]), Vec::new());
    }

    #[test]
    fn settings_search_items_one_header_per_section_run() {
        // Rows 0 and 1 are both in the first SETTINGS_TABS section ("Agent", 2 rows) — a
        // consecutive run from the same section gets exactly one header.
        let items = settings_search_items(&[0, 1]);
        assert_eq!(items, vec![SearchItem::Header(0), SearchItem::Row(0), SearchItem::Row(1)]);
    }

    #[test]
    fn settings_search_items_new_section_gets_its_own_header() {
        // Row 0 ("Agent") and row 33 (last row, "Workers", the final SETTINGS_TABS section) are in
        // different sections — a gap between matches emits a header for each.
        let items = settings_search_items(&[0, 33]);
        assert_eq!(
            items,
            vec![SearchItem::Header(0), SearchItem::Row(0), SearchItem::Header(9), SearchItem::Row(33)]
        );
    }


    /// A one-repo app whose pull brought in tags + branch updates, rendered to a `TestBackend`.
    fn render_ui(app: &mut AppState, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|frame| crate::render::render(frame, app, 0)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..height).map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    fn app_with_pull() -> AppState {
        let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
            "demo", std::path::PathBuf::from("/tmp/demo"),
        )))];
        let mut app = AppState::new(repos, Some(4), true);
        app.close_all_modals();
        // `AppState::new` reads the real `state-v3.json`, so anything a manual TUI session left
        // behind lands in these tests. Pin the prefs the render assertions depend on — driving the
        // app under a pty to verify a change persisted `help_tab` and the repo page's maximized flag,
        // and a test that asserted a flat section header started failing on a tabbed page.
        app.repo_page_tabbed_override = Some(false);
        app.maximized = None;
        app.help_tab = crate::app::HelpTab::Hotkeys;
        app.help_maximized = false;
        app.help_scroll = 0;
        app.settings_tab = 0;
        app.settings_search.clear();
        // The layout decides whether the settings body scrolls at all, and it is persisted.
        app.settings_layout = crate::app::SettingsLayout::Flat;
        // The accordion's fold state is persisted too — an unfolded release changes the row count.
        app.changelog_collapsed.clear();
        app.changelog_maximized = false;
        {
            let mut state = app.repos[0].lock().unwrap();
            state.pull_result = Some(crate::app::PullResult {
                prev_head: "aaa".into(), new_head: "bbb".into(),
                commits: 3, files: 5, insertions: 10, deletions: 2,
                new_tags: 2, new_branches: 1,
                new_tag_names: vec!["v1.152.1".into(), "v1.152.0".into()],
                fetched_branches: vec![
                    crate::app::FetchedRef { name: "origin/main".into(), detail: "abc1234..def5678".into() },
                    crate::app::FetchedRef { name: "origin/feat-x".into(), detail: "new branch".into() },
                ],
            });
            state.status = crate::app::RepoStatus::Updated;
        }
        app.selected = 0;
        app
    }

    /// The Command log's Tags/Branches tabs list ONLY what the pull fetched (the delta), and the
    /// pane carries the `D category · d …` mnemonic footer.
    #[test]
    fn command_log_tabs_are_scoped_to_the_pull_delta() {
        let mut app = app_with_pull();
        app.set_result_category(crate::app::RightView::Tags);
        let tags = render_ui(&mut app, 150, 30);
        assert!(tags.contains("+ v1.152.1") && tags.contains("+ v1.152.0"), "Tags tab lists fetched tags\n{tags}");
        assert!(tags.contains("D category") && tags.contains("d log/raw/unified/split"), "mnemonic footer present\n{tags}");

        app.set_result_category(crate::app::RightView::Branches);
        let branches = render_ui(&mut app, 150, 30);
        assert!(branches.contains("origin/main") && branches.contains("abc1234..def5678"), "advanced ref shown\n{branches}");
        assert!(branches.contains("origin/feat-x") && branches.contains("new branch"), "new branch shown\n{branches}");
    }

    /// The repo page ([4]) gets a Tags tab listing the full inventory, each row selectable with an
    /// info panel.
    #[test]
    fn repo_page_has_a_tags_tab() {
        let mut app = app_with_pull();
        {
            let mut state = app.repos[0].lock().unwrap();
            state.page = Some(crate::app::RepoPageData {
                branches: vec![], worktrees: vec![], stashes: vec![], commits: vec![],
                tags: vec![
                    crate::app::TagInfo { name: "v1.152.1".into(), sha: "def5678".into(), subject: "chore(release): 1.152.1".into(), author: "ci-bot".into(), rel_date: "7 hours ago".into() },
                    crate::app::TagInfo { name: "v1.151.9".into(), sha: "999aaaa".into(), subject: "fix(Tags): portal render".into(), author: "Steven P".into(), rel_date: "3 days ago".into() },
                ],
                head_dirty_count: 0, dirty_worktrees: vec![], fetched: true, fetch_error: None,
                base_branch: Some("origin/main".into()),
            });
        }
        app.repo_page = Some(0);
        app.repo_page_tab = crate::app::RepoTab::Tags;
        // Full-screen page: this used to come from the user's persisted `maximized`, so the test
        // passed or failed depending on how the app was last left.
        app.maximized = Some(crate::app::Pane::RepoPage);
        assert!(app.repo_page_present_tabs().contains(&crate::app::RepoTab::Tags), "Tags is a present tab");
        let page = render_ui(&mut app, 150, 34);
        assert!(page.contains("TAGS (2)"), "Tags section renders with its count\n{page}");
        assert!(page.contains("v1.152.1") && page.contains("def5678") && page.contains("ci-bot"), "tag row shows name/sha/author\n{page}");
    }

    /// Release notes re-flow to the modal width instead of re-breaking wherever CHANGELOG.md
    /// happened to hard-wrap. Regression: each source line was wrapped on its own, so the last
    /// word of a ~90-col line landed alone on the next row ("…and the very" / "thing"), and a
    /// continuation starting with `-` (`--write-tree)`) was promoted to a bullet of its own.
    #[test]
    fn release_notes_reflow_across_source_line_breaks() {
        let plain = |rows: &[(usize, Vec<(String, Style)>)]| -> Vec<String> {
            rows.iter()
                .map(|(indent, segs)| {
                    format!("{}{}", " ".repeat(*indent), segs.iter().map(|(text, _)| text.as_str()).collect::<String>())
                })
                .collect()
        };
        let notes = [
            "Squash-merged branches are now recognised",
            "Until now polygit decided whether a merged branch was safe to delete with a single",
            "`git merge-base --is-ancestor` check. A **squash merge** — GitHub's default, and the very thing",
            "that leaves a branch with a `gone` upstream — produces a fresh commit.",
            "- Unchanged where it matters: the ladder still runs **only** for the handful of gone-upstream repos",
            "  in a scan, never per-branch on the repo page. On git older than 2.38 (no `merge-tree",
            "  --write-tree`) the expensive rungs are skipped silently rather than erroring.",
            "Docs (README, keymap.json) updated to match.",
        ];
        let rows = wrap_release_notes(&notes, Style::default(), Style::default(), 60);
        let lines = plain(&rows);
        let body = lines.join("\n");

        // The whole block re-flows to the wrap width: no row is an orphan fragment of the
        // sentence above it, and a bullet's wrapped rows hang under its text.
        let expected = "    Squash-merged branches are now recognised
    Until now polygit decided whether a merged branch was safe
    to delete with a single git merge-base --is-ancestor check.
    A squash merge — GitHub's default, and the very thing that
    leaves a branch with a gone upstream — produces a fresh
    commit.
    - Unchanged where it matters: the ladder still runs only for
      the handful of gone-upstream repos in a scan, never
      per-branch on the repo page. On git older than 2.38 (no
      merge-tree --write-tree) the expensive rungs are skipped
      silently rather than erroring.
    Docs (README, keymap.json) updated to match.";
        assert_eq!(body, expected, "notes re-flow to the wrap width\n{body}");
        // Every row still fits the wrap width (indent included).
        for line in &lines {
            assert!(line.chars().count() <= 66, "row fits 60 + hanging indent: {line:?}\n{body}");
        }
        // The bullet's `--write-tree)` continuation stays inside the bullet, never its own item.
        assert!(body.contains("merge-tree --write-tree) the expensive rungs"), "bullet continuation rejoins\n{body}");
        assert_eq!(lines.iter().filter(|line| line.trim_start().starts_with("- ")).count(), 1, "exactly one bullet\n{body}");
        // Headline stands alone (bold) and does not swallow the paragraph under it.
        assert_eq!(lines[0].trim(), "Squash-merged branches are now recognised");
        let bolded: Vec<_> = rows[0].1.iter().filter(|(text, _)| !text.trim().is_empty()).collect();
        assert!(!bolded.is_empty() && bolded.iter().all(|(_, style)| style.add_modifier.contains(Modifier::BOLD)), "headline is bold");
        // The trailing flush-left note after the bullets is its own paragraph, not bullet text.
        assert!(lines.last().unwrap().trim().starts_with("Docs (README"), "trailing note stays separate\n{body}");
    }

    /// A blank line in a release's notes is a paragraph break: the two paragraphs must not weld
    /// into one block when the renderer re-flows hard-wrapped source lines.
    #[test]
    fn blank_note_line_separates_paragraphs() {
        let notes = ["Headline", "First paragraph line one", "still paragraph one.", "", "Second paragraph."];
        let rows = wrap_release_notes(&notes, Style::default(), Style::default(), 60);
        let lines: Vec<String> =
            rows.iter().map(|(_, segs)| segs.iter().map(|(text, _)| text.as_str()).collect()).collect();
        assert!(lines.iter().any(|line| line == "First paragraph line one still paragraph one."), "{lines:?}");
        assert!(lines.iter().any(|line| line.is_empty()), "the blank renders as a spacer row: {lines:?}");
        assert!(lines.iter().any(|line| line == "Second paragraph."), "not welded to the first: {lines:?}");
    }

    /// `- ` opens a list item; a hyphen or asterisk that merely starts a word does not.
    #[test]
    fn list_item_detection_requires_a_marker_space() {
        assert!(opens_list_item("- a bullet"));
        assert!(opens_list_item("* a bullet"));
        assert!(!opens_list_item("--write-tree`) the expensive rungs"));
        assert!(!opens_list_item("*merging adds nothing*"));
        assert!(!opens_list_item("-d <branch> keeps the safe delete"));
    }

    /// The changelog modal's scrollbar is drawn OVER the last column of the content area, so the
    /// note wrap width has to reserve that column. Regression: once notes re-flowed to fill the
    /// width, a bullet's continuation row ran to the very edge and the bar painted over its last
    /// character ("…so an unmerged branch still can't b║").
    ///
    /// Checked per rendered row, with the bars erased: each row's words must appear as a contiguous
    /// run in the release's own text. A character the bar painted over leaves a word that is in no
    /// run — and unlike a "is the column empty" check, this sees the loss, because the bar
    /// overwrites the glyph rather than pushing it. Row-wise so it does not depend on how long the
    /// newest release's notes happen to be.
    #[test]
    fn changelog_notes_never_reach_the_scrollbar_column() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        for padded in [false, true] {
            let mut app = app_with_pull();
            app.panel_padding = padded;
            app.open_changelog(false);
            let (width, height) = (120u16, 40u16);
            let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
            term.draw(|frame| crate::render::render(frame, &mut app, 0)).unwrap();
            let buffer = term.backend().buffer().clone();
            let screen: String = (0..height)
                .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            // The modal's own rect, captured by the render — scraping for a corner glyph would find
            // the list pane's border first when pane borders are on.
            let modal = app.changelog_area;
            assert!(modal.width > 20, "the changelog modal rendered\n{screen}");
            let strip = |text: &str| -> Vec<String> {
                text.split_whitespace()
                    .map(|word| word.replace(['*', '`'], ""))
                    .filter(|word| !word.is_empty())
                    .collect()
            };
            // Every word of every rendered release, so a row can be checked against its own source.
            let source: Vec<String> = strip(
                &crate::changelog::releases()
                    .iter()
                    .map(|release| release.notes.join(" "))
                    .collect::<Vec<_>>()
                    .join(" "),
            );

            let mut checked = 0usize;
            for row in modal.y + 1..modal.bottom().saturating_sub(1) {
                let text: String = (modal.x + 1..modal.right().saturating_sub(1))
                    .map(|col| {
                        let ch = buffer[(col, row)].symbol().chars().next().unwrap_or(' ');
                        // Erase the bar: a character it painted over leaves a mangled word.
                        if ch == '\u{2588}' || ch == '\u{2551}' { ' ' } else { ch }
                    })
                    .collect();
                let words = strip(&text);
                // Skip release headers and blank rows: they are not note text.
                if words.len() < 3 || !source.windows(2).any(|pair| pair[0] == words[0] && pair[1] == words[1]) {
                    continue;
                }
                assert!(
                    source.windows(words.len()).any(|window| window == words.as_slice()),
                    "padding={padded}: row {row} is not intact — a character was painted over: \
                     {words:?}\n{screen}"
                );
                checked += 1;
            }
            assert!(checked >= 8, "padding={padded}: only {checked} note rows checked\n{screen}");
        }
    }

    /// The changelog modal has to survive every terminal size its own clamps allow — the note wrap
    /// width bottoms out at 8 columns, so a tiny pane must still lay out rather than panic.
    #[test]
    fn changelog_renders_at_any_terminal_size() {
        for (width, height) in [(40u16, 12u16), (50, 20), (62, 24), (120, 40), (200, 60)] {
            let mut app = app_with_pull();
            app.open_changelog(false);
            let screen = render_ui(&mut app, width, height);
            assert!(screen.contains("Changelog"), "modal renders at {width}x{height}\n{screen}");
        }
    }


/// The perf overlay's LAST line is its verdict — the one line it exists to show. A height computed
/// one row short clips exactly that line while leaving a box that still looks complete, so assert
/// the verdict text reached the screen rather than that the box rendered.
#[test]
fn perf_overlay_shows_its_verdict_and_close_button() {
    let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
        "demo",
        std::path::PathBuf::from("/tmp/demo"),
    )))];
    let mut app = AppState::new(repos, Some(4), true);
    app.perf.toggle_overlay();
    // A sampled lag well under the alarm thresholds, so the verdict is the quiet one.
    for _ in 0..8 {
        app.perf.lag.record_us(500.0);
        app.perf.backlog.record_us(0.0);
    }

    let rows = render_rows(&mut app, 150, 44);
    let screen = rows.join("\n");
    assert!(screen.contains("perf ^T"), "the overlay renders its title");
    assert!(
        screen.contains("hover is keeping up"),
        "the verdict line must not be clipped off the bottom of the box:\n{screen}"
    );
    assert!(screen.contains("hover lag"), "the lag channel is listed");
    // Every clickable needs its region captured, or the `[x]` is drawn but dead.
    assert!(app.perf_close_click.is_some(), "the close button registers a click region");

    // Toggling off must clear the region, or a stale rect keeps swallowing clicks.
    app.perf.toggle_overlay();
    let rows = render_rows(&mut app, 150, 44);
    assert!(!rows.join("\n").contains("perf ^T"), "overlay is gone when toggled off");
    assert!(app.perf_close_click.is_none(), "the stale click region is cleared");
}

/// The panel draws AFTER `apply_hover` and `Clear`s its own cells, which resets any style that
/// pass applied — so a highlight registered there is computed and then wiped, leaving a button
/// that is clickable and dead on hover. It has to paint its own. Asserting the background changed
/// is the discrimination: an `apply_hover`-only implementation renders an identical-looking box.
#[test]
fn perf_panel_paints_its_own_hover() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
        "demo",
        std::path::PathBuf::from("/tmp/demo"),
    )))];
    let mut app = AppState::new(repos, Some(4), true);
    app.perf.toggle_overlay();
    app.perf.lag.record_us(500.0);

    // First frame with no cursor: learn where the `[x]` landed.
    app.hover = None;
    let mut term = Terminal::new(TestBackend::new(150, 44)).unwrap();
    term.draw(|frame| crate::render::render(frame, &mut app, 0)).unwrap();
    let (row, start, _end) = app.perf_close_click.expect("the close button registers a region");
    let unhovered = term.backend().buffer()[(start, row)].bg;

    // Second frame with the cursor on it.
    app.hover = Some((start, row));
    let mut term = Terminal::new(TestBackend::new(150, 44)).unwrap();
    term.draw(|frame| crate::render::render(frame, &mut app, 1)).unwrap();
    let hovered = term.backend().buffer()[(start, row)].bg;

    assert_ne!(hovered, unhovered, "the close button must change background under the cursor");
}

/// The panel is opaque on screen, so it must be opaque to the mouse: its whole rect is registered,
/// not just the close button. Without it a click in the middle selects the repo row behind it.
#[test]
fn perf_panel_registers_its_whole_rect_not_just_the_close_button() {
    let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
        "demo",
        std::path::PathBuf::from("/tmp/demo"),
    )))];
    let mut app = AppState::new(repos, Some(4), true);
    app.perf.toggle_overlay();
    app.perf.lag.record_us(500.0);
    let _ = render_rows(&mut app, 150, 44);

    let rect = app.perf_panel_rect;
    assert!(!rect.is_empty(), "the panel registers a rect");
    let (row, start, _) = app.perf_close_click.expect("close button region");
    assert!(
        crate::app::point_in(rect, start, row),
        "the close button must lie inside the registered panel rect"
    );
    // A point in the panel's body — not the button — must still be inside it.
    let mid = (rect.x + rect.width / 2, rect.y + rect.height / 2);
    assert!(crate::app::point_in(rect, mid.0, mid.1), "the body is covered too");

    // Closed again, the rect is cleared — a stale one would keep swallowing clicks over the list.
    app.perf.toggle_overlay();
    let _ = render_rows(&mut app, 150, 44);
    assert!(app.perf_panel_rect.is_empty(), "the stale rect is cleared when the panel closes");
}

/// The panel reports what it costs to draw, because that cost is subtracted from `flush` and a
/// reader who cannot see the correction has to take it on trust.
#[test]
fn perf_panel_reports_its_own_cost() {
    let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
        "demo",
        std::path::PathBuf::from("/tmp/demo"),
    )))];
    let mut app = AppState::new(repos, Some(4), true);
    app.perf.toggle_overlay();
    app.perf.lag.record_us(500.0);
    let screen = render_rows(&mut app, 150, 44).join("\n");
    assert!(screen.contains("overlay"), "the overlay-cost row is listed:\n{screen}");
    // And the render path actually sampled it, rather than leaving the row permanently blank.
    assert!(!app.perf.overlay_cost.is_empty(), "rendering the panel records its own cost");
}

/// A terminal too short for the overlay must simply not draw it — never panic, and never leave a
/// click region pointing at a box that was not rendered.
#[test]
fn perf_overlay_declines_to_draw_when_the_terminal_is_too_small() {
    let repos = vec![std::sync::Arc::new(std::sync::Mutex::new(RepoState::new(
        "demo",
        std::path::PathBuf::from("/tmp/demo"),
    )))];
    let mut app = AppState::new(repos, Some(4), true);
    app.perf.toggle_overlay();
    app.perf.lag.record_us(500.0);

    for (width, height) in [(40_u16, 10_u16), (20, 40), (150, 8)] {
        let rows = render_rows(&mut app, width, height);
        assert!(
            !rows.join("\n").contains("perf ^T"),
            "overlay must be suppressed at {width}x{height}"
        );
        assert!(app.perf_close_click.is_none(), "no click region at {width}x{height}");
    }
}

    /// Every scrollable surface registers its track in `app.scroll_hits`, so ONE test covers all of
    /// them: the column immediately left of a bar has to be blank on every row the bar spans.
    ///
    /// That column is the whole defect. A bar is drawn over the content area's last column, so a row
    /// that fills the width loses its final character to it — and the loss is invisible in the
    /// buffer, because the bar overwrote the glyph rather than pushing it. What IS visible is the
    /// column beside it: text pressed against a scrollbar means the surface never reserved a gutter,
    /// and the next character it wraps lands under the bar. `tuilith::scroll::Area` carves the gutter
    /// out of the content, and this asserts every surface actually went through it.
    #[test]
    fn no_scrollable_surface_puts_text_against_its_scrollbar() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // Each case opens one surface and names the track it must register — a surface that silently
        // failed to open would otherwise pass this test by having nothing to check.
        use crate::app::ScrollKind as SK;
        type Open = fn(&mut AppState);
        let surfaces: Vec<(&str, Open, SK)> = vec![
            ("panes", |_app| {}, SK::Info),
            ("changelog", |app| app.open_changelog(false), SK::Changelog),
            (
                "what's new",
                |app| {
                    app.whats_new_since = "0.0.1".to_string();
                    app.open_changelog(true);
                },
                SK::Changelog,
            ),
            ("settings", |app| app.show_settings = true, SK::Settings),
            (
                "help",
                |app| {
                    // The Design tab is the tall one — the others fit and would prove nothing.
                    app.help_tab = crate::app::HelpTab::DesignSystem;
                    app.show_help = true;
                },
                SK::Help,
            ),
            ("keybindings", |app| app.show_keybindings = true, SK::Keybindings),
            (
                "build info",
                |app| {
                    // The preview has to overflow, or the modal opens with nothing to scroll and the
                    // case proves nothing. Long lines so a row can fill the width.
                    app.build_info_settings_preview =
                        (0..200).map(|n| format!("  \"key_{n}\": \"{}\",", "v".repeat(160))).collect();
                    app.show_build_info = true;
                },
                SK::BuildInfo,
            ),
            ("repo page", |app| {
                let tags: Vec<crate::app::TagInfo> = (0..60)
                    .map(|n| crate::app::TagInfo {
                        name: format!("v1.0.{n}"),
                        sha: format!("{n:07}a"),
                        subject: "a".repeat(140),
                        author: "someone with a long name".into(),
                        rel_date: "3 days ago".into(),
                    })
                    .collect();
                app.repos[0].lock().unwrap().page = Some(crate::app::RepoPageData {
                    branches: vec![], worktrees: vec![], stashes: vec![], commits: vec![], tags,
                    head_dirty_count: 0, dirty_worktrees: vec![], fetched: true, fetch_error: None,
                    base_branch: Some("origin/main".into()),
                });
                app.repo_page = Some(0);
                app.repo_page_tab = crate::app::RepoTab::Tags;
                app.maximized = Some(crate::app::Pane::RepoPage);
            }, SK::RepoPage),
            ("diff modal", |app| {
                app.diff_modal = Some(crate::app::DiffModal {
                    source: crate::app::DiffSource::Branch {
                        path: std::path::PathBuf::from("/tmp/demo"),
                        name: "feature".into(),
                    },
                    mode: crate::app::DiffMode::Uncommitted,
                    view: crate::app::DiffView::Raw,
                    focus: crate::app::DiffFocus::Diff,
                    files: (0..40)
                        .map(|n| crate::app::DiffFile {
                            status: "M".into(),
                            path: format!("src/{}{n}.rs", "deeply/nested/".repeat(6)),
                            untracked: false,
                        })
                        .collect(),
                    selected: 0,
                    file_scroll: 0,
                    // Lines wider than any pane, so a row that can fill the width does.
                    lines: (0..200).map(|n| format!("+ {n} {}", "x".repeat(200))).collect(),
                    scroll: 0,
                    loading: false,
                    diff_loading: false,
                    status_filter: None,
                });
            }, SK::DiffBody),
        ];

        for (name, open, expect) in surfaces {
            // Both padding settings: with padding the gutter is the pane's own blank column, without
            // it the content has to give one up — the case that used to have no gap at all.
            for padded in [false, true] {
                for (width, height) in [(120u16, 40u16), (80, 24)] {
                    let mut app = app_with_pull();
                    app.panel_padding = padded;
                    open(&mut app);
                    let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
                    term.draw(|frame| crate::render::render(frame, &mut app, 0)).unwrap();
                    let buffer = term.backend().buffer().clone();
                    let overflowing: Vec<crate::app::ScrollKind> = app
                        .scroll_hits
                        .iter()
                        .filter(|hit| hit.total > hit.viewport)
                        .map(|hit| hit.kind)
                        .collect();
                    assert!(
                        overflowing.contains(&expect),
                        "{name} at {width}x{height}: expected an overflowing {expect:?} track, got \
                         {overflowing:?} — did the surface open?"
                    );
                    let modal_open = app.any_modal_open();
                    for hit in &app.scroll_hits {
                        // Only vertical bars that actually overflow are drawn.
                        if hit.total <= hit.viewport || hit.track.width != 1 || hit.track.x == 0 {
                            continue;
                        }
                        // A pane under an open modal still registers its track, but the column beside
                        // it now holds the modal's frame — nothing observable about the pane.
                        let pane = matches!(
                            hit.kind,
                            crate::app::ScrollKind::List
                                | crate::app::ScrollKind::Info
                                | crate::app::ScrollKind::Preview
                                | crate::app::ScrollKind::RepoPage
                        );
                        if modal_open && pane {
                            continue;
                        }
                        let gutter = hit.track.x - 1;
                        for row in hit.track.y..hit.track.bottom().min(height) {
                            let symbol = buffer[(gutter, row)].symbol().to_string();
                            assert_eq!(
                                symbol.trim(),
                                "",
                                "{name} at {width}x{height} (padding={padded}): {:?} has text in the \
                                 column beside its scrollbar at row {row} — the next character it \
                                 wraps lands under the bar\n{}",
                                hit.kind,
                                (0..height)
                                    .map(|y| (0..width)
                                        .map(|x| buffer[(x, y)].symbol())
                                        .collect::<String>())
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            );
                        }
                    }
                }
            }
        }
    }


    /// A scroll hint is only shown while the surface can actually scroll. A footer that advertises
    /// `j/k scroll` on content that fits teaches the reader to stop reading footers — and the keys
    /// genuinely do nothing there.
    ///
    /// The exception is a **collapsible** surface: the changelog accordion's `j/k` moves the release
    /// selection, and the build-info tree's moves the cursor, whether or not anything overflows.
    /// Those keep their hint.
    #[test]
    fn a_scroll_hint_appears_only_where_something_scrolls() {
        // The helper both halves are built from.
        assert!(scroll_hint("j/k", false, crate::app::HintKey::Char('j')).is_empty());
        assert_eq!(scroll_hint("j/k", true, crate::app::HintKey::Char('j')).len(), 3);

        // A surface whose content I control: the build-info raw preview.
        let render_preview = |lines: usize| -> String {
            let mut app = app_with_pull();
            app.build_info_settings_preview = (0..lines).map(|n| format!("line {n}")).collect();
            app.show_build_info = true;
            render_ui(&mut app, 120, 40)
        };
        let fits = render_preview(3);
        assert!(!fits.contains("j/k scroll"), "3 lines fit, so no scroll hint\n{fits}");
        assert!(fits.contains("esc") || fits.contains("close"), "the rest of the footer stays\n{fits}");
        let overflows = render_preview(400);
        assert!(overflows.contains("j/k scroll"), "400 lines scroll, so the hint shows\n{overflows}");

        // The collapsible exceptions keep their hint whatever fits.
        let mut app = app_with_pull();
        app.open_changelog(false);
        let accordion = render_ui(&mut app, 120, 40);
        assert!(accordion.contains("j/k scroll"), "the accordion always offers j/k\n{accordion}");
    }
