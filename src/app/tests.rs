    use super::*;

    #[test]
    fn branch_check_interval_scales_and_clamps() {
        assert_eq!(AppState::branch_check_interval_secs(0), 1); // floor 1s
        assert_eq!(AppState::branch_check_interval_secs(10), 1);
        assert_eq!(AppState::branch_check_interval_secs(100), 10);
        assert_eq!(AppState::branch_check_interval_secs(250), 25);
        assert_eq!(AppState::branch_check_interval_secs(10_000), 60); // ceiling 60s
    }

    fn fresh_cli_builder() -> CliBuilder {
        CliBuilder {
            selected: 0,
            on: vec![false; CLI_FLAGS.len()],
            values: vec![String::new(); CLI_FLAGS.len()],
            use_short: vec![false; CLI_FLAGS.len()],
            editing: None,
            help_mode: crate::app::CliHelpMode::OnHover,
        }
    }

    fn cli_index(flag: &str) -> usize {
        CLI_FLAGS.iter().position(|candidate| candidate.flag == flag).unwrap()
    }

    #[test]
    fn cli_builder_command_assembles_flags() {
        let mut builder = fresh_cli_builder();
        assert_eq!(builder.command(), "polygit");
        let dir = 0; // positional DIR
        let depth = cli_index("--depth");
        let no_tui = cli_index("--no-tui");
        let jobs = cli_index("--jobs");
        // Every flag (value flags included) needs its checkbox on to be emitted.
        builder.values[dir] = "~/projects".to_string();
        builder.set_on(dir, true);
        builder.values[depth] = "3".to_string();
        builder.set_on(depth, true);
        builder.set_on(no_tui, true);
        assert_eq!(builder.command(), "polygit ~/projects --depth 3 --no-tui");
        // --jobs has a short form; `use_short` swaps it.
        builder.values[jobs] = "8".to_string();
        builder.set_on(jobs, true);
        assert!(builder.command().contains("--jobs 8"));
        builder.toggle_short(jobs);
        assert!(builder.command().contains("-j 8") && !builder.command().contains("--jobs 8"));
    }

    #[test]
    fn cli_builder_parent_child_cascade() {
        let mut builder = fresh_cli_builder();
        let profile = cli_index("--profile");
        let profile_out = cli_index("--profile-out");
        // The child is disabled while the parent is off; checking it is a no-op.
        assert!(!builder.enabled(profile_out));
        builder.set_on(profile_out, true);
        assert!(!builder.on[profile_out]);
        // Check the parent → the child becomes enabled and can be set.
        builder.set_on(profile, true);
        assert!(builder.enabled(profile_out));
        builder.values[profile_out] = "out.txt".to_string();
        builder.set_on(profile_out, true);
        assert!(builder.command().contains("--profile-out out.txt"));
        // Unchecking the parent cascades: the child is force-unchecked and disabled again.
        builder.set_on(profile, false);
        assert!(!builder.on[profile_out] && !builder.enabled(profile_out));
        assert!(!builder.command().contains("--profile"));
    }

    #[test]
    fn accordion_nav_includes_headers_and_skips_collapsed_rows() {
        let mut state = state_named(&["a"]);
        state.settings_layout = SettingsLayout::Accordion;
        state.collapsed_settings.clear();
        state.settings_on_header = Some(0); // start focused on the first section header (Lists)
        // Positions include every header; expanded sections also contribute their rows.
        let positions = state.accordion_positions();
        assert!(matches!(positions[0], AccPos::Header(0)));
        assert!(positions.iter().any(|pos| matches!(pos, AccPos::Row(0))));
        // Down from the Lists header lands on its first row (global row 0).
        state.settings_move(1);
        assert_eq!(state.settings_on_header, None);
        assert_eq!(state.settings_selected, 0);
        // Collapse Lists via its header → its rows drop out of the nav sequence.
        state.settings_on_header = Some(0);
        state.toggle_focused_accordion_section();
        assert!(state.settings_section_collapsed(0));
        assert!(!state.accordion_positions().iter().any(|pos| matches!(pos, AccPos::Row(0))));
        // Down from the collapsed Lists header skips to the next header (Theming).
        state.settings_on_header = Some(0);
        state.settings_move(1);
        assert_eq!(state.settings_on_header, Some(1));
        // ←/→ helpers collapse / expand the focused section.
        state.set_selected_settings_section(true);
        assert!(state.settings_section_collapsed(1));
        state.set_selected_settings_section(false);
        assert!(!state.settings_section_collapsed(1));
        // Collapse-all then expand-all round-trips.
        state.toggle_all_settings_sections();
        assert!(state.settings_all_collapsed());
        state.toggle_all_settings_sections();
        assert!(state.collapsed_settings.is_empty());
    }

    #[test]
    fn settings_layout_cycles_three_ways() {
        assert_eq!(SettingsLayout::Tabbed.cycle(), SettingsLayout::Accordion);
        assert_eq!(SettingsLayout::Accordion.cycle(), SettingsLayout::Flat);
        assert_eq!(SettingsLayout::Flat.cycle(), SettingsLayout::Tabbed);
    }

    #[test]
    fn status_token_filter_matches_status_and_attributes() {
        let mut repo = RepoState::new("alpha", std::path::PathBuf::from("/tmp/alpha"));
        repo.status = RepoStatus::Failed;
        assert!(AppState::status_token_matches(&repo, "fail"));
        assert!(AppState::status_token_matches(&repo, "failed"));
        assert!(!AppState::status_token_matches(&repo, "updated"));
        assert!(AppState::status_token_matches(&repo, "")); // bare '@' matches all
        repo.status = RepoStatus::UpToDate;
        repo.details = Some(RepoDetails {
            ahead: Some(0),
            behind: Some(3),
            dirty_count: 2,
            stash_count: 0,
            branch_count: 0,
            commit_hash: String::new(),
            commit_subject: String::new(),
            commit_author: String::new(),
            commit_rel_date: String::new(),
            commit_timestamp: 0,
        });
        assert!(AppState::status_token_matches(&repo, "dirty"));
        assert!(!AppState::status_token_matches(&repo, "clean"));
        assert!(AppState::status_token_matches(&repo, "behind"));
        assert!(!AppState::status_token_matches(&repo, "ahead"));
        assert!(AppState::status_token_matches(&repo, "up-to-date"));
    }

    #[test]
    fn help_tab_about_is_not_persisted() {
        let mut state = state_named(&["a"]);
        // Switching to a useful tab tracks it for persistence.
        state.set_help_tab(HelpTab::Legend);
        assert_eq!(state.help_tab_persist, HelpTab::Legend);
        // Switching to About shows it but does NOT change the persisted tab.
        state.set_help_tab(HelpTab::About);
        assert_eq!(state.help_tab, HelpTab::About);
        assert_eq!(state.help_tab_persist, HelpTab::Legend);
    }

    /// `AppState::new` restores the user's real persisted preferences (sort, grouping, …) —
    /// reset everything view-affecting so tests are hermetic regardless of state.json.
    fn normalized(mut state: AppState) -> AppState {
        state.sort_column = SortColumn::Name;
        state.sort_dir = SortDir::Asc;
        state.status_filter = StatusFilter::All;
        state.filter = None;
        state.grouping_enabled = false;
        state.collapsed_groups.clear();
        state.tree_enabled = false;
        state.collapsed_folders.clear();
        state.favorites.clear();
        state.favorites_first = false;
        // Window/focus state also comes from the real state.json — pin it so focus/pane tests are
        // hermetic.
        state.focus = Pane::List;
        state.maximized = None;
        state.repo_page = None;
        // Workspace state comes from the real state.json too — pin it for hermetic tests.
        state.workspaces.clear();
        state.active_workspace = None;
        state.info_pinned = false;
        state.show_result_panel = true;
        state.dock_ratio = AppState::DOCK_DEFAULT;
        // Auto-pull policy comes from the user's real state.json — pin it to the defaults so the
        // gate/settle tests are hermetic.
        state.auto_pull_on_launch = true;
        state.auto_pull_max_repos = 100;
        state.auto_pull_in_tree = false;
        state.auto_pull_suppressed = false;
        // Tooltip prefs also persist — pin them to the all-on default for hermetic settings tests.
        state.tooltips = crate::app::TooltipPrefs::default();
        // The "Merged PRs" toggle persists too — pin it off (default) so the PR settings tests are
        // hermetic regardless of what's enabled in the real state.json.
        state.show_merged_prs = false;
        // Design System tab layout persists too — pin it for hermetic help tests.
        state.design_layout = crate::app::DesignLayout::Flat;
        state.design_section = 0;
        // The "What's New" modal can auto-open from the real state.json (version drift) — close it.
        state.show_changelog = false;
        state
    }

    #[test]
    fn settings_search_filters_and_navigates() {
        let mut state = state_named(&["a"]);
        // No query → every row matches.
        assert_eq!(state.settings_filtered_rows().len(), AppState::SETTINGS_ROWS);
        // "auto" matches the four Auto-* rows (fuzzy, case-insensitive).
        state.settings_search = "auto".to_string();
        let matches = state.settings_filtered_rows();
        assert!(matches.iter().all(|&idx| SETTINGS_LABELS[idx].to_lowercase().contains("auto")));
        assert_eq!(matches.len(), 4);
        // Navigation moves within the filtered set.
        state.settings_selected = matches[0];
        state.settings_move(1);
        assert_eq!(state.settings_selected, matches[1]);
        state.settings_move(-5); // clamps to the first match
        assert_eq!(state.settings_selected, matches[0]);
        // Clearing the search restores the full set.
        state.settings_clear_search();
        assert!(state.settings_search.is_empty());
        assert_eq!(state.settings_filtered_rows().len(), AppState::SETTINGS_ROWS);
    }

    #[test]
    fn result_panel_toggle_and_split_ratio() {
        let mut state = state_named(&["a"]);
        // Toggling flips the flag (default on → off → on).
        let initial = state.show_result_panel;
        state.toggle_result_panel();
        assert_eq!(state.show_result_panel, !initial);
        state.toggle_result_panel();
        assert_eq!(state.show_result_panel, initial);
        // Dragging the boundary sets the ratio from an absolute row, clamped.
        state.preview_split_area = Rect { x: 0, y: 10, width: 40, height: 20 };
        state.set_preview_split_from_row(20); // 10 rows above of 20 → 0.5
        assert!((state.preview_split_ratio - 0.5).abs() < 1e-9);
        state.set_preview_split_from_row(10); // at the top → clamps to MIN
        assert!((state.preview_split_ratio - AppState::PREVIEW_SPLIT_MIN).abs() < 1e-9);
        state.set_preview_split_from_row(40); // past the bottom → clamps to MAX
        assert!((state.preview_split_ratio - AppState::PREVIEW_SPLIT_MAX).abs() < 1e-9);
    }

    #[test]
    fn visible_panes_reflect_visibility_and_cycle_skips_hidden() {
        let mut state = state_named(&["a", "b"]);
        state.selected = 0;
        // Default: list + result (info off, no repo page).
        state.info_pinned = false;
        state.show_result_panel = true;
        assert_eq!(state.visible_panes(), vec![Pane::List, Pane::Result]);
        // Info appears when pinned over a selected repo; hiding result drops it.
        state.info_pinned = true;
        state.show_result_panel = false;
        assert_eq!(state.visible_panes(), vec![Pane::List, Pane::Info]);
        // Cycling wraps across only the visible panels (Result is skipped).
        state.focus = Pane::List;
        state.cycle_focus(true);
        assert_eq!(state.focus, Pane::Info);
        state.cycle_focus(true);
        assert_eq!(state.focus, Pane::List);
        state.cycle_focus(false);
        assert_eq!(state.focus, Pane::Info);
        // focus_pane ignores a hidden panel.
        state.focus = Pane::List;
        state.focus_pane(Pane::Result);
        assert_eq!(state.focus, Pane::List);
    }

    #[test]
    fn repo_page_window_state_and_focus() {
        let mut state = state_named(&["a", "b"]);
        state.selected = 0;
        state.open_repo_page();
        assert_eq!(state.repo_page, Some(0));
        assert_eq!(state.focus, Pane::RepoPage);
        // Default restored → the page is one of several panels.
        assert_ne!(state.maximized, Some(Pane::RepoPage));
        assert!(state.visible_panes().contains(&Pane::RepoPage));
        assert!(state.visible_panes().contains(&Pane::List));
        // Maximized → the page is the sole focusable panel (full-screen).
        state.toggle_maximized(Pane::RepoPage);
        assert_eq!(state.maximized, Some(Pane::RepoPage));
        assert_eq!(state.visible_panes(), vec![Pane::RepoPage]);
        // Closing returns focus to the list.
        state.close_repo_page();
        assert_eq!(state.repo_page, None);
        assert_eq!(state.focus, Pane::List);
    }

    #[test]
    fn retarget_repo_page_reuses_the_cached_page() {
        let mut state = state_named(&["a", "b"]);
        state.selected = 0;
        state.open_repo_page(); // clears repo 0's page to force a fresh fetch
        // Repo 1 has a cached page; retargeting must not nuke it (unlike open_repo_page).
        state.repos[1].lock().unwrap().page = Some(RepoPageData::default());
        state.repo_page_selected = 5;
        state.retarget_repo_page(1);
        assert_eq!(state.repo_page, Some(1));
        assert_eq!(state.repo_page_selected, 0, "selection resets to the top");
        assert!(state.repos[1].lock().unwrap().page.is_some(), "cache preserved");
    }

    #[test]
    fn title_button_hit_only_on_the_buttons() {
        let mut state = state_named(&["a"]);
        state.repo_page_back_click = Some((4, 30, 40));
        state.repo_page_window_click = Some((4, 28, 29));
        assert!(state.title_button_hit(35, 4)); // inside [esc back]
        assert!(state.title_button_hit(28, 4)); // the window icon cell
        assert!(!state.title_button_hit(28, 5)); // wrong row
        assert!(!state.title_button_hit(10, 4)); // left of both buttons (the drag handle)
        assert!(!state.title_button_hit(40, 4)); // half-open: end is exclusive
        // Also covers the pane maximize / copy buttons and the cols/sort triggers, so a click on
        // any of them on a splitter-handle row isn't stolen by the splitter grab.
        state.max_click.push((4, 50, 52, crate::app::Pane::Result));
        state.info_click.push((4, 44, 46, crate::app::InfoAction::CopyText("x".into())));
        state.page_cols_click = Some((4, 18, 25));
        assert!(state.title_button_hit(51, 4)); // pane maximize button
        assert!(state.title_button_hit(45, 4)); // copy button
        assert!(state.title_button_hit(20, 4)); // t cols ▾ trigger
        assert!(!state.title_button_hit(60, 4)); // empty gap → still a drag handle
    }

    #[test]
    fn pulled_columns_latch_on_and_dont_flicker() {
        let mut state = state_named(&["a", "b"]);
        state.columns.pulled_files = true;
        state.columns.pulled_commits = true;
        // Nothing pulled yet → the columns are hidden (auto-hide).
        state.refresh_pulled_seen();
        assert!(!state.pulled_seen);
        assert!(!state.column_available(Column::PulledFiles));
        assert!(!state.effective_columns().pulled_files);
        // A pull lands a delta → the columns latch on.
        state.repos[0].lock().unwrap().pull_result =
            Some(PullResult { files: 3, ..Default::default() });
        state.refresh_pulled_seen();
        assert!(state.pulled_seen);
        assert!(state.column_available(Column::PulledFiles));
        assert!(state.effective_columns().pulled_files);
        // A retry clears every pull_result mid-flight — the columns must NOT flicker out.
        state.repos[0].lock().unwrap().pull_result = None;
        state.refresh_pulled_seen();
        assert!(state.column_available(Column::PulledFiles), "latched on, no flicker");
    }

    #[test]
    fn settings_reset_clears_every_diff() {
        let mut state = state_named(&["a"]);
        // Diverge a few settings from their defaults.
        state.theme = Theme::Dark;
        state.icon_style = IconStyle::Emoji;
        state.grouping_enabled = true;
        state.show_borders = false;
        let plan = state.settings_reset_plan();
        assert!(plan.iter().any(|line| line.starts_with("Theme: dark")), "{plan:?}");
        assert!(plan.iter().any(|line| line.contains("emoji")));
        assert!(!plan.is_empty());
        // After a reset, every row matches its default and the plan is empty.
        state.apply_settings_reset();
        for (row, label) in SETTINGS_LABELS.iter().enumerate() {
            assert_eq!(
                state.settings_active_option(row),
                AppState::settings_default_option(row),
                "row {row} ({label}) not at default after reset",
            );
        }
        assert!(state.settings_reset_plan().is_empty());
    }

    #[test]
    fn workspace_state_defaults_and_is_addressable() {
        let mut state = state_named(&["a"]);
        // Ad-hoc session by default — no active workspace.
        assert_eq!(state.active_workspace, None);
        assert!(state.workspaces.is_empty());
        // The save path keys off active_workspace; simulate a named session.
        state.active_workspace = Some("work".to_string());
        state.workspaces.insert("work".to_string(), vec!["/x".to_string()]);
        assert_eq!(state.workspaces.get("work"), Some(&vec!["/x".to_string()]));
    }

    #[test]
    fn auto_pull_limit_cycles_through_choices() {
        assert_eq!(next_auto_pull_limit(50), 100);
        assert_eq!(next_auto_pull_limit(100), 250);
        assert_eq!(next_auto_pull_limit(250), 0); // ∞
        assert_eq!(next_auto_pull_limit(0), 50); // ∞ wraps to 50
        assert_eq!(next_auto_pull_limit(999), 50); // any stray value → 50
    }

    #[test]
    fn should_auto_pull_respects_master_threshold_and_tree() {
        let mut state = state_with(&[]); // normalized: on, limit 100, not in tree, flat view
        assert!(state.should_auto_pull(10));
        assert!(state.should_auto_pull(100)); // at the limit is allowed
        assert!(!state.should_auto_pull(101)); // over the limit

        state.auto_pull_max_repos = 0; // ∞ — no limit
        assert!(state.should_auto_pull(100_000));

        state.auto_pull_max_repos = 100;
        state.auto_pull_on_launch = false; // master off
        assert!(!state.should_auto_pull(1));

        state.auto_pull_on_launch = true;
        state.tree_enabled = true; // toggle on, but a flat scan has no folders…
        assert!(
            state.should_auto_pull(5),
            "a flat scan renders no tree, so tree_enabled alone must not suppress auto-pull"
        );

        // …only an *active* tree (toggle on AND nested folders present) suppresses.
        state.tree_nodes = vec![TreeNode {
            rel_path: "sub".to_string(),
            name: "sub".to_string(),
            depth: 0,
            parent: None,
            children: Vec::new(),
            repos: vec![0],
        }];
        assert!(state.tree_active());
        assert!(!state.should_auto_pull(5));
        state.auto_pull_in_tree = true; // unless explicitly allowed
        assert!(state.should_auto_pull(5));
    }

    #[test]
    fn copy_preview_short_text_keeps_all_lines() {
        assert_eq!(copy_preview("/home/user/repo"), vec!["/home/user/repo"]);
        assert_eq!(copy_preview("one\ntwo"), vec!["one", "two"]);
        assert_eq!(copy_preview("one\ntwo\nthree"), vec!["one", "two", "three"]);
    }

    #[test]
    fn copy_preview_long_text_truncates_with_marker() {
        assert_eq!(
            copy_preview("one\ntwo\nthree\nfour\nfive"),
            vec!["one", "two", "three", "… +2 more lines"]
        );
    }

    // After a reset, every setting must actually BE at its default — i.e. the reset-plan (what still
    // differs) is empty. Catches `apply_settings_reset` / `settings_default_option` drifting from the
    // real field defaults (e.g. when a default flips and one of the two tables isn't updated).
    #[test]
    fn stash_columns_default_on_and_toggle() {
        use crate::app::{DropdownKind, RepoPageStashColumn};
        let mut state = state_with(&[RepoStatus::UpToDate]);
        // Both optional Stashes-tab columns default on.
        assert!(state.repo_page_stash_columns.age);
        assert!(state.repo_page_stash_columns.stats);
        state.toggle_repo_page_stash_column(RepoPageStashColumn::Age);
        assert!(!state.repo_page_stash_columns.age);
        assert!(state.repo_page_stash_columns.stats);
        // The StashColumns dropdown lists exactly the two toggleable columns.
        state.open_dropdown(DropdownKind::StashColumns, 0, 0);
        assert_eq!(state.dropdown_len(), 2);
        let items = state.dropdown_items();
        assert_eq!(items.len(), 2);
        assert!(!items[0].on, "age now off"); // age is the first row
        assert!(items[1].on, "stats still on");
    }

    #[test]
    fn reset_reaches_defaults_and_plan_is_empty() {
        let mut state = state_with(&[RepoStatus::UpToDate]);
        // Flip a bunch of settings off-default first.
        state.panel_padding = false;
        state.hover_effects = false;
        state.grouping_enabled = false;
        state.splitter_mode = crate::app::SplitterMode::Dedicated;
        state.repo_page_tabs = crate::app::RepoTabsMode::Off;
        state.apply_settings_reset();
        assert!(
            state.settings_reset_plan().is_empty(),
            "after reset nothing should still differ from default: {:?}",
            state.settings_reset_plan()
        );
        // And every row's active option equals its declared default option.
        for row in 0..AppState::SETTINGS_ROWS {
            assert_eq!(
                state.settings_active_option(row),
                AppState::settings_default_option(row),
                "row {row} ({}) not at default after reset",
                crate::app::SETTINGS_LABELS[row]
            );
        }
    }

    fn state_with(statuses: &[RepoStatus]) -> AppState {
        let repos: Vec<SharedRepoState> = statuses
            .iter()
            .enumerate()
            .map(|(index, status)| {
                let mut repo = RepoState::new(format!("repo{index}"), PathBuf::from("/tmp"));
                repo.status = status.clone();
                Arc::new(Mutex::new(repo))
            })
            .collect();
        normalized(AppState::new(repos, 4, true))
    }

    #[test]
    fn is_retryable_covers_failed_skipped_and_throttled() {
        assert!(RepoStatus::Failed.is_retryable());
        assert!(RepoStatus::Skipped.is_retryable());
        assert!(RepoStatus::Throttled.is_retryable());
        assert!(!RepoStatus::UpToDate.is_retryable());
        assert!(!RepoStatus::Updated.is_retryable());
        assert!(!RepoStatus::Queued.is_retryable());
        assert!(!RepoStatus::Running { pid: 1 }.is_retryable());
    }

    #[test]
    fn issues_filter_and_counts_cover_throttled() {
        let state = state_with(&[RepoStatus::Throttled, RepoStatus::UpToDate, RepoStatus::Failed]);
        assert!(StatusFilter::Issues.matches(&RepoStatus::Throttled));
        assert_eq!(state.retryable_repos(), vec![0, 2]);
        let counts = state.counts();
        assert_eq!(counts.7, 1, "throttled is the appended 8th element");
        assert_eq!(counts.5, 1, "failed stays at .5");
        assert!(state.has_errors());
        // Throttled is terminal (so the run can settle) but counts toward done.
        assert!(RepoStatus::Throttled.is_terminal());
        assert_eq!(state.done_count(), 3);
    }

    #[test]
    fn throttle_control_halves_debounces_and_floors_at_one() {
        let control = ThrottleControl::new(16);
        assert_eq!(control.effective(), 16);
        assert!(!control.reduced());
        assert_eq!(control.on_throttle(), 8);
        // An immediate second event is debounced — no further halving.
        assert_eq!(control.on_throttle(), 8);
        assert!(control.reduced());
        assert!(control.recently_throttled());

        let tiny = ThrottleControl::new(1);
        assert_eq!(tiny.on_throttle(), 1); // (1/2).max(1)
    }

    #[test]
    fn throttle_control_drains_due_retries_only() {
        let control = ThrottleControl::new(4);
        control.schedule_retry(2, Instant::now() - Duration::from_secs(1)); // already due
        control.schedule_retry(3, Instant::now() + Duration::from_secs(60)); // not yet
        assert_eq!(control.take_due_retries(), vec![2]);
        assert!(control.take_due_retries().is_empty(), "the future retry stays queued");
    }

    #[test]
    fn retry_targets_are_failed_and_skipped() {
        let state = state_with(&[
            RepoStatus::UpToDate,
            RepoStatus::Failed,
            RepoStatus::Skipped,
            RepoStatus::Running { pid: 1 },
        ]);
        assert_eq!(state.retryable_repos(), vec![1, 2]);
        assert!(state.any_retryable());
    }

    #[test]
    fn refetch_targets_every_repo_not_running() {
        // Refetch = "pull regardless of status", so it now includes idle/cached Queued repos
        // (so a suppressed-auto-pull launch can pull them); only in-flight repos are excluded.
        let state = state_with(&[
            RepoStatus::UpToDate,
            RepoStatus::Failed,
            RepoStatus::Skipped,
            RepoStatus::Running { pid: 1 },
            RepoStatus::Queued,
        ]);
        assert_eq!(state.refetchable_repos(), vec![0, 1, 2, 4]);
        assert!(state.any_refetchable());
    }

    #[test]
    fn selected_helpers_track_the_current_row() {
        let mut state = state_with(&[
            RepoStatus::UpToDate,
            RepoStatus::Failed,
            RepoStatus::Skipped,
            RepoStatus::Running { pid: 1 },
        ]);

        state.selected = 0; // clean success: refetchable but not retryable
        assert!(!state.selected_repo_retryable());
        assert!(state.selected_repo_refetchable());

        state.selected = 1; // failed: both
        assert!(state.selected_repo_retryable());
        assert!(state.selected_repo_refetchable());

        state.selected = 2; // skipped: both
        assert!(state.selected_repo_retryable());
        assert!(state.selected_repo_refetchable());

        state.selected = 3; // running: neither
        assert!(!state.selected_repo_retryable());
        assert!(!state.selected_repo_refetchable());

        state.selected = 4; // Result item (no repo)
        assert!(!state.selected_repo_retryable());
        assert!(!state.selected_repo_refetchable());
    }

    fn state_named(names: &[&str]) -> AppState {
        let repos: Vec<SharedRepoState> = names
            .iter()
            .map(|name| Arc::new(Mutex::new(RepoState::new(*name, PathBuf::from(format!("/tmp/{name}"))))))
            .collect();
        normalized(AppState::new(repos, 4, true))
    }

    #[test]
    fn favorites_first_pins_a_top_section() {
        let mut state = state_named(&["alpha", "beta", "gamma"]);
        state.favorites_first = true;
        // No favorites yet → no pinned section.
        assert!(!matches!(state.visible_rows().first(), Some(ListRow::FavoritesHeader)));
        // Favorite beta (absolute index 1).
        state.toggle_favorite(1);
        assert!(state.is_favorite(1));
        assert!(state.has_favorites());
        let rows = state.visible_rows();
        assert_eq!(rows[0], ListRow::FavoritesHeader);
        assert!(matches!(rows[1], ListRow::Repo { repo_idx: 1, .. }));
        assert_eq!(rows[2], ListRow::Spacer);
        // beta appears only in the pinned section, not again in the body.
        let body_betas = rows
            .iter()
            .skip(3)
            .filter(|row| matches!(row, ListRow::Repo { repo_idx: 1, .. }))
            .count();
        assert_eq!(body_betas, 0);
        // Un-favoriting beta removes the pinned section.
        state.toggle_favorite(1);
        assert!(!state.has_favorites());
        assert!(!matches!(state.visible_rows().first(), Some(ListRow::FavoritesHeader)));
    }

    #[test]
    fn favorite_column_toggles_and_persists_in_flags() {
        let mut state = state_named(&["a"]);
        state.columns.favorite = false;
        state.toggle_column(Column::Favorite);
        assert!(state.columns.favorite);
        assert!(state.effective_columns().favorite);
        state.toggle_column(Column::Favorite);
        assert!(!state.columns.favorite);
    }

    #[test]
    fn wheel_scroll_is_independent_of_selection() {
        let mut state = state_named(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        let viewport = 3;
        state.selected = 0;
        state.list_scroll = 0;
        // Plain wheel scrolls the view without touching the selection.
        state.scroll_list(2, viewport);
        assert_eq!(state.list_scroll, 2);
        assert_eq!(state.selected, 0);
        // Scrolling is clamped to the content (8 repos + separator + Result = 10 rows).
        let max = state.max_list_scroll(viewport);
        state.scroll_list(100, viewport);
        assert_eq!(state.list_scroll, max);
        // Bringing the selection into view only scrolls as far as needed.
        state.ensure_list_selection_visible(viewport);
        assert_eq!(state.list_scroll, 0); // selection 0 was above → snap up to it
        // A selection already on screen never moves the view (the bottom-row-press-up case).
        state.selected = 1;
        state.list_scroll = 0;
        state.ensure_list_selection_visible(viewport);
        assert_eq!(state.list_scroll, 0);
        // A selection below the viewport scrolls just enough to reveal it.
        state.selected = 5;
        state.ensure_list_selection_visible(viewport);
        assert_eq!(state.list_scroll, 5 + 1 - viewport);
    }

    #[test]
    fn filter_input_previews_first_match_and_esc_restores() {
        let mut state = state_named(&["alpha", "beta", "gamma"]);
        state.selected = 2; // gamma (name-asc order)
        assert_eq!(state.selected_repo_index(), Some(2));
        state.begin_filter_input();
        assert_eq!(state.filter_prev_selection, Some(2));
        // Typing narrows to beta; the selection previews the first (only) match.
        state.filter = Some("be".to_string());
        state.select_first_filtered_row();
        assert_eq!(state.selected_repo_index(), Some(1));
        // Esc clears the filter and restores the original selection.
        state.cancel_filter_input();
        assert_eq!(state.filter, None);
        assert!(!state.filter_input_mode);
        assert_eq!(state.selected_repo_index(), Some(2));
    }

    #[test]
    fn filter_commit_keeps_previewed_selection() {
        let mut state = state_named(&["alpha", "beta", "gamma"]);
        state.selected = 0;
        state.begin_filter_input();
        state.filter = Some("gam".to_string());
        state.select_first_filtered_row();
        assert_eq!(state.selected_repo_index(), Some(2)); // gamma
        state.commit_filter_input();
        assert_eq!(state.filter_prev_selection, None);
        assert!(!state.filter_input_mode);
        assert_eq!(state.selected_repo_index(), Some(2)); // kept
    }

    #[test]
    fn sort_by_name_orders_visible_indices() {
        let mut state = state_named(&["charlie", "alpha", "bravo"]);
        // Name asc is the default sort.
        state.sort_column = SortColumn::Name;
        state.sort_dir = SortDir::Asc;
        assert_eq!(state.visible_indices(), vec![1, 2, 0]); // alpha, bravo, charlie

        state.sort_dir = SortDir::Desc;
        assert_eq!(state.visible_indices(), vec![0, 2, 1]); // charlie, bravo, alpha
    }

    #[test]
    fn sort_breaks_ties_by_name_ascending() {
        // Insertion order is deliberately non-alphabetical; three share a branch, one differs.
        let mut state = state_named(&["charlie", "alpha", "bravo", "zulu"]);
        state.repos[0].lock().unwrap().branch = Some("dev".into()); // charlie
        state.repos[1].lock().unwrap().branch = Some("dev".into()); // alpha
        state.repos[2].lock().unwrap().branch = Some("dev".into()); // bravo
        state.repos[3].lock().unwrap().branch = Some("fix".into()); // zulu
        state.sort_column = SortColumn::Branch;

        // Asc: "dev" group first, sorted by name (alpha, bravo, charlie), then "fix" (zulu).
        state.sort_dir = SortDir::Asc;
        assert_eq!(state.visible_indices(), vec![1, 2, 0, 3]);

        // Desc: "fix" (zulu) leads, but the "dev" group's name tiebreak stays ascending.
        state.sort_dir = SortDir::Desc;
        assert_eq!(state.visible_indices(), vec![3, 1, 2, 0]);
    }

    fn diff_modal_with(statuses: &[&str]) -> DiffModal {
        DiffModal {
            source: DiffSource::Branch { path: PathBuf::from("/tmp"), name: "x".into() },
            mode: DiffMode::Uncommitted,
            view: crate::app::DiffView::Raw,
            focus: DiffFocus::Files,
            files: statuses
                .iter()
                .enumerate()
                .map(|(index, status)| DiffFile {
                    status: (*status).to_string(),
                    path: format!("file{index}.rs"),
                    untracked: false,
                })
                .collect(),
            selected: 0,
            file_scroll: 0,
            lines: Vec::new(),
            scroll: 0,
            loading: false,
            diff_loading: false,
            status_filter: None,
        }
    }

    #[test]
    fn diff_chips_active_needs_enough_files_and_variety() {
        // 11 files but one status → no chips.
        let single = diff_modal_with(&["M"; 11]);
        assert!(!single.chips_active());
        // 11 files, two statuses → chips.
        let mut statuses = vec!["M"; 10];
        statuses.push("D");
        let varied = diff_modal_with(&statuses);
        assert!(varied.chips_active());
        // 10 files (not > 10) → no chips even with variety.
        let small = diff_modal_with(&["M", "D", "A", "M", "D", "A", "M", "D", "A", "M"]);
        assert!(!small.chips_active());
    }

    #[test]
    fn diff_status_chips_count_and_order() {
        let mut statuses = vec!["M"; 5];
        statuses.extend(vec!["A"; 3]);
        statuses.extend(vec!["D"; 2]);
        statuses.push("??");
        let modal = diff_modal_with(&statuses);
        // Order is M, A, D, then untracked (?) last; counts are over the full list.
        assert_eq!(modal.status_chips(), vec![('M', 5), ('A', 3), ('D', 2), ('?', 1)]);
    }

    fn branch_info(name: &str, upstream: Option<&str>, stats: Option<BranchStats>) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            is_head: false,
            upstream: upstream.map(str::to_string),
            ahead: upstream.map(|_| 0),
            behind: upstream.map(|_| 0),
            last_commit_rel: "1 day ago".into(),
            last_commit_secs: 0,
            subject: "work".into(),
            commit_sha: "abc1234".into(),
            author: "Ada".into(),
            stats,
            merge_base_short: Some("def5678".into()),
            base: Some("origin/main".into()),
            base_is_override: false,
        }
    }

    #[test]
    fn branch_stats_total_sums_fields() {
        let stats = BranchStats { added: 2, modified: 3, deleted: 1 };
        assert_eq!(stats.total(), 6);
    }

    #[test]
    fn repo_page_row_cmp_sorts_by_column() {
        let row = |name: &str, secs: i64, base: Option<&str>| PageRow {
            kind: PageRowKind::Branch,
            branch: name.to_string(),
            path: PathBuf::from("/tmp"),
            deletable: false,
            is_head: false,
            dirty: false,
            dirty_count: 0,
            stash_index: None,
            ahead: None,
            behind: None,
            upstream: None,
            last_commit_rel: String::new(),
            last_commit_secs: secs,
            subject: String::new(),
            stats: None,
            commit_sha: String::new(),
            author: String::new(),
            merge_base_short: None,
            base: base.map(str::to_string),
            base_is_override: false,
        };
        let zed = row("zed", 100, Some("origin/main"));
        let abe = row("abe", 200, Some("origin/dev"));
        use std::cmp::Ordering;
        // Name sorts ascending; Age sorts by timestamp; Base sorts by the base branch string.
        assert_eq!(repo_page_row_cmp(RepoPageSort::Name, &abe, &zed), Ordering::Less);
        assert_eq!(repo_page_row_cmp(RepoPageSort::Age, &zed, &abe), Ordering::Less);
        assert_eq!(repo_page_row_cmp(RepoPageSort::Base, &abe, &zed), Ordering::Less);
    }

    #[test]
    fn repo_page_column_available_reflects_loaded_stats() {
        let mut state = state_named(&["a"]);
        state.repos[0].lock().unwrap().page = Some(RepoPageData {
            branches: vec![
                branch_info("main", Some("origin/main"), Some(BranchStats::default())),
                branch_info("feat", None, Some(BranchStats { added: 4, modified: 0, deleted: 0 })),
            ],
            base_branch: Some("origin/main".into()),
            ..Default::default()
        });
        state.repo_page = Some(0);
        // Added has a non-zero somewhere → available; Deleted is all-zero-loaded → hidden.
        assert!(state.repo_page_column_available(RepoPageColumn::Added));
        assert!(!state.repo_page_column_available(RepoPageColumn::Deleted));
        // An upstream exists on `main` → ahead/behind + upstream available.
        assert!(state.repo_page_column_available(RepoPageColumn::AheadBehind));
        // Age/subject always available.
        assert!(state.repo_page_column_available(RepoPageColumn::Age));

        // A branch with unknown (still-loading) stats keeps stat columns available.
        state.repos[0].lock().unwrap().page.as_mut().unwrap().branches[1].stats = None;
        assert!(state.repo_page_column_available(RepoPageColumn::Deleted));
    }

    #[test]
    fn dropdown_columns_toggle_and_sort_picks() {
        let mut state = state_named(&["a"]);
        state.columns.dirty = false;
        // Columns dropdown: items reflect the flags; activating a column flips it and stays open.
        state.open_dropdown(DropdownKind::ListColumns, 0, 0);
        let items = state.dropdown_items();
        let dirty_idx = items.iter().position(|item| item.label == "dirty").unwrap();
        assert!(!items[dirty_idx].on);
        assert!(!state.dropdown_activate(dirty_idx), "columns stay open");
        assert!(state.columns.dirty);
        // Sort dropdown: activating picks the sort and closes.
        state.open_dropdown(DropdownKind::ListSort, 0, 0);
        let items = state.dropdown_items();
        let branch_idx = items.iter().position(|item| item.label == "branch").unwrap();
        assert!(state.dropdown_activate(branch_idx), "sort closes on pick");
        assert_eq!(state.sort_column, SortColumn::Branch);
    }

    #[test]
    fn dropdown_opens_with_nothing_highlighted_and_arrows_wrap_from_none() {
        let mut state = state_named(&["a"]);
        state.open_dropdown(DropdownKind::ListSort, 0, 0);
        assert_eq!(state.dropdown.unwrap().selected, None, "nothing pre-highlighted on open");
        // From None, up lands on the last row; from None, down lands on the first.
        state.dropdown_move(-1);
        let last = state.dropdown_items().len() - 1;
        assert_eq!(state.dropdown.unwrap().selected, Some(last));
        state.open_dropdown(DropdownKind::ListSort, 0, 0);
        state.dropdown_move(1);
        assert_eq!(state.dropdown.unwrap().selected, Some(0));
    }

    #[test]
    fn dropdown_mnemonic_keys_pick_items() {
        let mut state = state_named(&["a"]);
        state.columns.status = true;
        // `u` is the status column's mnemonic — toggles it, stays open.
        state.open_dropdown(DropdownKind::ListColumns, 0, 0);
        assert!(!state.dropdown_activate_key('u'), "columns stay open");
        assert!(!state.columns.status, "status toggled off via its mnemonic");
        // `s` is the sort dropdown's status mnemonic — picks it and closes.
        state.open_dropdown(DropdownKind::ListSort, 0, 0);
        assert!(state.dropdown_activate_key('s'), "sort closes on pick");
        assert_eq!(state.sort_column, SortColumn::Status);
        // An unknown key is a no-op (stays open).
        state.open_dropdown(DropdownKind::ListColumns, 0, 0);
        assert!(!state.dropdown_activate_key('Q'));
    }

    #[test]
    fn dropdown_disabled_column_mnemonic_is_inert() {
        // Scan complete with no worktrees anywhere → the worktrees column is unavailable + inert.
        let mut state = state_named(&["a"]);
        state.discovery_done = true;
        state.worktrees_done = true;
        state.open_dropdown(DropdownKind::ListColumns, 0, 0);
        let worktrees = state
            .dropdown_items()
            .into_iter()
            .find(|item| item.label == "worktrees")
            .unwrap();
        assert!(!worktrees.enabled, "no repo has worktrees → disabled");
        let before = state.columns.worktrees;
        assert!(!state.dropdown_activate_key(worktrees.mnemonic), "disabled key does nothing");
        assert_eq!(state.columns.worktrees, before);
    }

    #[test]
    fn repo_page_pr_column_available_only_with_a_pr() {
        let mut state = state_named(&["a"]);
        state.repos[0].lock().unwrap().page = Some(RepoPageData::default());
        state.repo_page = Some(0);
        assert!(!state.repo_page_column_available(RepoPageColumn::PullRequest));
        state.repos[0].lock().unwrap().pr =
            Some(PrInfo { number: 42, title: "x".into(), url: "http://e/42".into(), state: PrState::Open });
        assert!(state.repo_page_column_available(RepoPageColumn::PullRequest));
        // Toggling flips the stored flag (default on).
        assert!(state.repo_page_columns.pull_request);
        state.toggle_repo_page_column(RepoPageColumn::PullRequest);
        assert!(!state.repo_page_columns.pull_request);
    }

    #[test]
    fn pr_shown_gates_merged_and_closed_on_the_setting() {
        let pr = |state| PrInfo { number: 1, title: "x".into(), url: "u".into(), state };
        // Open PRs always show, regardless of the setting.
        assert!(pr(PrState::Open).shown(false));
        assert!(pr(PrState::Open).shown(true));
        // Merged/closed only show when "Merged PRs" is on.
        assert!(!pr(PrState::Merged).shown(false));
        assert!(!pr(PrState::Closed).shown(false));
        assert!(pr(PrState::Merged).shown(true));
        assert!(pr(PrState::Closed).shown(true));
    }

    #[test]
    fn merged_prs_setting_persists_and_resets() {
        let mut state = state_named(&["a"]);
        assert!(!state.show_merged_prs); // off by default
        // The "Merged PRs" settings row (index 12) toggles + reports its active option.
        let merged_row = crate::app::SETTINGS_LABELS.iter().position(|&l| l == "Merged PRs").unwrap();
        assert_eq!(state.settings_active_option(merged_row), 1); // "off"
        state.settings_selected = merged_row;
        state.toggle_selected_setting();
        assert!(state.show_merged_prs);
        assert_eq!(state.settings_active_option(merged_row), 0); // "on"
        // Reset restores the default (off).
        state.apply_settings_reset();
        assert!(!state.show_merged_prs);
    }

    #[test]
    fn settings_tables_stay_consistent() {
        // The single-source `SETTINGS` table drives labels + tips; the section counts
        // (`SETTINGS_TABS`), option labels, defaults, and read/write dispatch are keyed by the
        // SAME global row index. This test fails loudly if any of them drifts — so adding or
        // reordering a setting can't silently desync a tooltip / option / handler again.
        assert_eq!(SETTINGS.len(), AppState::SETTINGS_ROWS);
        assert_eq!(SETTINGS_LABELS.len(), AppState::SETTINGS_ROWS);
        let tab_total: usize = SETTINGS_TABS.iter().map(|(_, count)| count).sum();
        assert_eq!(tab_total, AppState::SETTINGS_ROWS, "SETTINGS_TABS counts must cover every row");

        let mut state = state_named(&["a"]);
        for row in 0..AppState::SETTINGS_ROWS {
            // Reset per row: the Icons round-trip leaves emoji mode on, which would gate the
            // emoji-dependent Hide-zeros row. Unicode lets every row's options round-trip.
            state.icon_style = crate::app::IconStyle::Unicode;
            assert!(!SETTINGS[row].label.is_empty(), "row {row} has no label");
            assert_eq!(SETTINGS[row].label, SETTINGS_LABELS[row], "label desync at row {row}");
            assert!(AppState::settings_tip(row, None).is_some(), "row {row} has no tooltip");
            let options = AppState::settings_option_labels(row);
            assert!(!options.is_empty(), "row {row} has no options");
            assert!(
                AppState::settings_default_option(row) < options.len(),
                "row {row} default option out of range"
            );
            assert!(state.settings_active_option(row) < options.len(), "row {row} active out of range");
            // Every option must round-trip set → active so the write/read dispatch agree.
            for opt in 0..options.len() {
                state.set_setting_option(row, opt);
                assert_eq!(
                    state.settings_active_option(row),
                    opt,
                    "row {row} ({}) option {opt} did not round-trip",
                    SETTINGS_LABELS[row]
                );
            }
        }
    }

    #[test]
    fn stash_rows_carry_their_change_stats() {
        let mut state = state_named(&["a"]);
        state.repos[0].lock().unwrap().page = Some(RepoPageData {
            stashes: vec![StashInfo {
                index: 0,
                label: "WIP on main".into(),
                date_rel: "2 days ago".into(),
                created_secs: 0,
                stats: Some(BranchStats { added: 1, modified: 2, deleted: 0 }),
            }],
            ..Default::default()
        });
        state.repo_page = Some(0);
        let rows = state.repo_page_rows();
        let stash = rows.iter().find(|row| row.kind == PageRowKind::Stash).unwrap();
        assert_eq!(stash.stats.map(|stat| stat.total()), Some(3));
    }

    #[test]
    fn diff_select_steps_through_visible_list() {
        let statuses = ["M", "D", "A", "M", "D", "A", "M", "D", "A", "M", "D", "A"];
        let mut state = state_named(&["a"]);
        state.diff_modal = Some(diff_modal_with(&statuses));
        state.diff_files_viewport = 20;
        // Visible order is grouped: [0,3,6,9, 2,5,8,11, 1,4,7,10]. Start at 0, step +1 → 3.
        assert!(state.diff_modal_select(1));
        assert_eq!(state.diff_modal.as_ref().unwrap().selected, 3);

        // Filtering to D, with selection 3 (an M) filtered out, reselects the first D (index 1).
        assert!(state.diff_modal_set_filter(Some('D')));
        assert_eq!(state.diff_modal.as_ref().unwrap().selected, 1);
        // Stepping +1 within the D group goes 1 → 4.
        assert!(state.diff_modal_select(1));
        assert_eq!(state.diff_modal.as_ref().unwrap().selected, 4);

        // Clearing the filter keeps the current selection (still visible) — no refetch.
        assert!(!state.diff_modal_set_filter(None));
        assert_eq!(state.diff_modal.as_ref().unwrap().selected, 4);
    }

    #[test]
    fn diff_visible_indices_filter_and_group() {
        // 12 files, interleaved statuses → chips active, so the list groups by status.
        let statuses = ["M", "D", "A", "M", "D", "A", "M", "D", "A", "M", "D", "A"];
        let mut modal = diff_modal_with(&statuses);
        // No filter: grouped M*4, A*4, D*4 (stable within each group).
        let grouped = modal.visible_file_indices();
        assert_eq!(grouped, vec![0, 3, 6, 9, 2, 5, 8, 11, 1, 4, 7, 10]);
        // Filter to D: only the deleted files, in original order.
        modal.status_filter = Some('D');
        assert_eq!(modal.visible_file_indices(), vec![1, 4, 7, 10]);
    }

    #[test]
    fn column_available_hides_empty_columns_once_loaded() {
        let mut state = state_named(&["a", "b"]);
        // Mid-scan: nothing is "done", so columns stay available (no flicker).
        assert!(state.column_available(Column::Worktrees));
        assert!(state.column_available(Column::Stashes));

        // Discovery + worktree scan complete, no worktrees and no stashes anywhere → hidden.
        state.discovery_done = true;
        state.worktrees_done = true;
        for repo in &state.repos {
            let mut locked = repo.lock().unwrap();
            let details = locked.details.get_or_insert_with(Default::default);
            details.branch_count = 1;
            details.stash_count = 0;
        }
        assert!(!state.column_available(Column::Worktrees));
        assert!(!state.column_available(Column::Stashes));
        assert!(!state.column_available(Column::Branches)); // only the current branch
        // Always-on columns never hide.
        assert!(state.column_available(Column::Dirty));

        // One repo gains a second branch → branches column becomes available again.
        state.repos[0].lock().unwrap().details.as_mut().unwrap().branch_count = 3;
        assert!(state.column_available(Column::Branches));
        let effective = state.effective_columns();
        assert!(!effective.worktrees || !state.columns.worktrees);
    }

    #[test]
    fn sort_by_branch_orders_visible_indices() {
        let mut state = state_named(&["one", "two", "three"]);
        state.repos[0].lock().unwrap().branch = Some("main".into());
        state.repos[1].lock().unwrap().branch = Some("dev".into());
        state.repos[2].lock().unwrap().branch = Some("feature".into());
        state.set_sort(SortColumn::Branch);
        // dev, feature, main
        assert_eq!(state.visible_indices(), vec![1, 2, 0]);
    }

    #[test]
    fn set_sort_toggles_direction_on_repeat() {
        let mut state = state_named(&["a", "b"]);
        // Switching to a fresh column resets to Asc.
        state.set_sort(SortColumn::Status);
        assert_eq!((state.sort_column, state.sort_dir), (SortColumn::Status, SortDir::Asc));
        // Re-pressing the active column flips direction.
        state.set_sort(SortColumn::Status);
        assert_eq!(state.sort_dir, SortDir::Desc);
        state.set_sort(SortColumn::Branch);
        assert_eq!((state.sort_column, state.sort_dir), (SortColumn::Branch, SortDir::Asc));
    }

    #[test]
    fn all_clean_successes_have_no_retry_targets() {
        let state = state_with(&[RepoStatus::UpToDate, RepoStatus::Updated]);
        assert!(!state.any_retryable());
        assert!(state.retryable_repos().is_empty());
        assert!(state.any_refetchable());
        assert_eq!(state.refetchable_repos(), vec![0, 1]);
    }

    /// A named-repos state with groups from a JSON config (already normalized by
    /// `state_named`) and grouping switched on.
    fn grouped_state(names: &[&str], groups_json: &str) -> AppState {
        let mut state = state_named(names);
        state.grouping_enabled = true;
        let config: GroupsConfig = serde_json::from_str(groups_json).unwrap();
        let errors = state.init_groups(config, &GroupsCache::default());
        assert!(errors.is_empty(), "unexpected config errors: {errors:?}");
        state
    }

    /// A tree-view state from explicit relative paths (name = last component). Tree on.
    fn tree_state(rel_paths: &[&str]) -> AppState {
        let repos: Vec<SharedRepoState> = rel_paths
            .iter()
            .map(|rel| {
                let name = rel.rsplit('/').next().unwrap_or(rel);
                let mut repo = RepoState::new(name, PathBuf::from(format!("/tmp/{rel}")));
                repo.rel_path = rel.to_string();
                Arc::new(Mutex::new(repo))
            })
            .collect();
        let mut state = normalized(AppState::new(repos, 4, true));
        state.tree_enabled = true;
        state.rebuild_tree();
        state
    }

    #[test]
    fn remove_root_hides_repos_and_re_add_unhides() {
        let mut state = tree_state(&["x", "y"]);
        state.root_dirs = vec![PathBuf::from("/work/alpha"), PathBuf::from("/work/beta")];
        state.repos[0].lock().unwrap().root = PathBuf::from("/work/alpha");
        state.repos[1].lock().unwrap().root = PathBuf::from("/work/beta");
        state.rebuild_tree();
        // Select repo 0 (under alpha) and remove its root.
        state.selected = state
            .visible_rows()
            .iter()
            .position(|row| matches!(row, ListRow::Repo { repo_idx: 0, .. }))
            .unwrap();
        state.remove_selected_root();
        assert!(state.repos[0].lock().unwrap().hidden);
        assert!(!state.repos[1].lock().unwrap().hidden);
        assert_eq!(state.root_dirs, vec![PathBuf::from("/work/beta")]);
        assert!(!state.visible_indices().contains(&0));
        // Re-adding the root un-hides its repos (canonicalize fails on the fake path → kept as-is).
        state.add_root(PathBuf::from("/work/alpha"));
        assert!(!state.repos[0].lock().unwrap().hidden);
        assert!(state.visible_indices().contains(&0));
    }

    #[test]
    fn multi_root_tree_is_a_forest() {
        // Two roots, one repo each — the tree must give each root its own top-level folder node.
        let mut state = tree_state(&["x", "y"]);
        state.root_dirs = vec![PathBuf::from("/work/alpha"), PathBuf::from("/work/beta")];
        state.repos[0].lock().unwrap().root = PathBuf::from("/work/alpha");
        state.repos[1].lock().unwrap().root = PathBuf::from("/work/beta");
        state.rebuild_tree();
        let desc = describe(&state);
        assert!(desc.contains(&"folder:alpha".to_string()), "{desc:?}");
        assert!(desc.contains(&"folder:beta".to_string()), "{desc:?}");
        // x nests under alpha, y under beta (depth-1 repos).
        let alpha = desc.iter().position(|d| d == "folder:alpha").unwrap();
        assert_eq!(desc[alpha + 1], "  repo:x");
        // A single root keeps the flat (no synthetic root node) layout.
        let single = tree_state(&["a/b", "a/c"]);
        assert!(!describe(&single).iter().any(|d| d.starts_with("folder:") && d.contains("tmp")));
    }

    /// Render the visible rows as readable `kind:label` strings (indented by depth) for asserts.
    fn describe(state: &AppState) -> Vec<String> {
        state
            .visible_rows()
            .iter()
            .map(|row| match *row {
                ListRow::Repo { repo_idx, depth } => format!(
                    "{}repo:{}",
                    "  ".repeat(depth as usize),
                    state.repos[repo_idx].lock().unwrap().name
                ),
                ListRow::FolderHeader { node_idx, depth } => format!(
                    "{}folder:{}",
                    "  ".repeat(depth as usize),
                    state.tree_nodes[node_idx].name
                ),
                ListRow::GroupHeader { group_idx, depth, .. } => {
                    format!("{}group:{}", "  ".repeat(depth as usize), state.group_name(group_idx))
                }
                ListRow::FavoritesHeader => "favorites".to_string(),
                ListRow::Spacer => "spacer".to_string(),
            })
            .collect()
    }

    #[test]
    fn scoped_fetch_targets_selected_folder_subtree() {
        let mut state = tree_state(&["groupA/r1", "groupA/r2", "groupB/r3"]);
        // Select the groupA folder header.
        let rows = state.visible_rows();
        let folder_a = rows
            .iter()
            .position(|row| {
                matches!(row, ListRow::FolderHeader { node_idx, .. }
                    if state.tree_nodes[*node_idx].name == "groupA")
            })
            .expect("groupA folder header present");
        state.selected = folder_a;
        // The header covers groupA's subtree (r1, r2) — not groupB's r3.
        let mut covered = state.selected_header_repos().expect("folder header selected");
        covered.sort();
        assert_eq!(covered, vec![0, 1]);
        assert!(!covered.contains(&2));
        // Nothing retryable yet; marking r1 failed makes the scoped retry meaningful.
        assert!(!state.selected_header_retryable());
        state.repos[0].lock().unwrap().status = RepoStatus::Failed;
        assert!(state.selected_header_retryable());
        // Selecting a repo row instead yields no header scope (falls back to single-repo actions).
        let repo_row = rows.iter().position(|row| matches!(row, ListRow::Repo { .. })).unwrap();
        state.selected = repo_row;
        assert!(state.selected_header_repos().is_none());
    }

    #[test]
    fn build_tree_nests_folders_and_assigns_repos() {
        let nodes = build_tree(&[
            (0, "root-repo".to_string()),
            (1, "work/api".to_string()),
            (2, "work/web".to_string()),
            (3, "work/sub/deep".to_string()),
        ]);
        // root-repo has no '/', so it gets no node; folders: work, work/sub.
        let work = nodes.iter().find(|node| node.rel_path == "work").unwrap();
        assert_eq!(work.depth, 0);
        assert_eq!(work.repos, vec![1, 2]);
        let sub = nodes.iter().find(|node| node.rel_path == "work/sub").unwrap();
        assert_eq!(sub.depth, 1);
        assert_eq!(sub.repos, vec![3]);
        assert_eq!(sub.parent.and_then(|idx| nodes.get(idx)).map(|n| n.rel_path.as_str()), Some("work"));
    }

    #[test]
    fn tree_view_shows_root_repos_then_sorted_folders() {
        let state = tree_state(&["root1", "work/api", "work/web", "personal/notes"]);
        assert_eq!(
            describe(&state),
            vec![
                "repo:root1",
                "folder:personal", // folders sorted by name: personal before work
                "  repo:notes",
                "folder:work",
                "  repo:api",
                "  repo:web",
            ]
        );
    }

    #[test]
    fn tree_collapsed_folder_hides_its_subtree() {
        let mut state = tree_state(&["work/api", "work/sub/deep"]);
        // Collapse "work" → only its header remains.
        state.collapsed_folders.insert("work".to_string());
        assert_eq!(describe(&state), vec!["folder:work"]);
        // Collapsing only the nested "work/sub" keeps work open, hides deep.
        state.collapsed_folders.clear();
        state.collapsed_folders.insert("work/sub".to_string());
        assert_eq!(
            describe(&state),
            vec!["folder:work", "  folder:sub", "  repo:api"]
        );
    }

    #[test]
    fn tree_plus_groups_subdivides_repos_inside_folders() {
        let mut state = tree_state(&["work/mfe-a", "work/mfe-b", "work/core"]);
        state.grouping_enabled = true;
        let config: GroupsConfig =
            serde_json::from_str(r#"{"groups": [{"name": "frontend", "pattern": "mfe-*"}]}"#)
                .unwrap();
        state.init_groups(config, &GroupsCache::default());
        // Inside "work": a frontend group (mfe-a, mfe-b) then the ungrouped section (core).
        assert_eq!(
            describe(&state),
            vec![
                "folder:work",
                "  group:frontend",
                "  repo:mfe-a",
                "  repo:mfe-b",
                "  group:ungrouped",
                "  repo:core",
            ]
        );
    }

    #[test]
    fn tree_plus_groups_collapse_key_is_folder_scoped() {
        let mut state = tree_state(&["work/mfe-a", "work/mfe-b", "other/mfe-c", "other/mfe-d"]);
        state.grouping_enabled = true;
        // threshold 1 (via config) makes the multi-member fe sections collapsible.
        let config: GroupsConfig = serde_json::from_str(
            r#"{"collapse_threshold": 1, "groups": [{"name": "fe", "pattern": "mfe-*"}]}"#,
        )
        .unwrap();
        state.init_groups(config, &GroupsCache::default());
        // Collapsing fe under "other" must not collapse fe under "work" (composite keys).
        state.collapsed_groups.insert("other::fe".to_string());
        let rows = describe(&state);
        assert!(rows.contains(&"  repo:mfe-a".to_string()), "work/fe stays expanded: {rows:?}");
        assert!(!rows.contains(&"  repo:mfe-c".to_string()), "other/fe is collapsed: {rows:?}");
    }

    fn repo_rows(indices: &[usize]) -> Vec<ListRow> {
        indices.iter().map(|&idx| ListRow::repo(idx)).collect()
    }

    #[test]
    fn grouping_off_rows_match_visible_indices() {
        let mut state = grouped_state(
            &["a-one", "b-two", "a-two"],
            r#"{"groups": [{"name": "a", "pattern": "a-*"}]}"#,
        );
        state.grouping_enabled = false;
        assert_eq!(state.visible_rows(), repo_rows(&state.visible_indices()));
        state.sort_column = SortColumn::Name;
        assert_eq!(state.visible_rows(), repo_rows(&state.visible_indices()));
    }

    #[test]
    fn grouped_sections_keep_config_order_with_ungrouped_last() {
        let state = grouped_state(
            &["zeta", "mfe-a", "core", "mfe-b"],
            r#"{"groups": [{"name": "frontend", "pattern": "mfe-*"}]}"#,
        );
        // Groups follow config order; repos within each section follow the active sort (name asc):
        // frontend → mfe-a (1), mfe-b (3); ungrouped → core (2), zeta (0).
        assert_eq!(
            state.visible_rows(),
            vec![
                ListRow::group(0, false),
                ListRow::repo(1),
                ListRow::repo(3),
                ListRow::Spacer,
                ListRow::group(1, false),
                ListRow::repo(2),
                ListRow::repo(0),
            ]
        );
        assert_eq!(state.group_name(0), "frontend");
        assert_eq!(state.group_name(1), "ungrouped");
    }

    #[test]
    fn first_matching_group_wins_in_config_order() {
        let state = grouped_state(
            &["mfe-core"],
            r#"{"groups": [
                {"name": "first", "pattern": "mfe-*"},
                {"name": "second", "repos": ["mfe-core"]}
            ]}"#,
        );
        assert_eq!(state.repo_group_map, vec![Some(0)]);
    }

    #[test]
    fn flat_list_when_nothing_matches_any_group() {
        let state = grouped_state(
            &["alpha", "beta"],
            r#"{"groups": [{"name": "frontend", "pattern": "mfe-*"}]}"#,
        );
        assert_eq!(state.visible_rows(), repo_rows(&[0, 1]));
    }

    #[test]
    fn empty_groups_are_hidden_under_a_status_filter() {
        let mut state = grouped_state(
            &["a-1", "b-1"],
            r#"{"groups": [
                {"name": "a", "pattern": "a-*"},
                {"name": "b", "pattern": "b-*"}
            ]}"#,
        );
        state.repos[0].lock().unwrap().status = RepoStatus::Failed;
        state.repos[1].lock().unwrap().status = RepoStatus::UpToDate;
        state.status_filter = StatusFilter::Failed;
        assert_eq!(
            state.visible_rows(),
            vec![ListRow::group(0, false), ListRow::repo(0)]
        );
    }

    #[test]
    fn collapse_threshold_boundary_decides_collapsibility() {
        // threshold 2: a 2-member group gets a static header, a 3-member group a collapsible one.
        let state = grouped_state(
            &["a-1", "a-2", "b-1", "b-2", "b-3"],
            r#"{"collapse_threshold": 2, "groups": [
                {"name": "a", "pattern": "a-*"},
                {"name": "b", "pattern": "b-*"}
            ]}"#,
        );
        let rows = state.visible_rows();
        assert_eq!(rows[0], ListRow::group(0, false));
        assert_eq!(rows[3], ListRow::Spacer);
        assert_eq!(rows[4], ListRow::group(1, true));
    }

    #[test]
    fn collapsed_group_hides_members_but_keeps_its_header() {
        let mut state = grouped_state(
            &["b-1", "b-2", "b-3", "other"],
            r#"{"collapse_threshold": 2, "groups": [{"name": "b", "pattern": "b-*"}]}"#,
        );
        state.collapsed_groups.insert("b".to_string());
        assert_eq!(
            state.visible_rows(),
            vec![
                ListRow::group(0, true),
                ListRow::Spacer,
                ListRow::group(1, false),
                ListRow::repo(3),
            ]
        );
    }

    #[test]
    fn nav_skips_static_headers_and_spacers_in_both_directions() {
        // Layout: [static header, repo(1), repo(3), spacer, static header, repo(0), repo(2)],
        // then Result at 7.
        let mut state = grouped_state(
            &["zeta", "mfe-a", "core", "mfe-b"],
            r#"{"groups": [{"name": "frontend", "pattern": "mfe-*"}]}"#,
        );
        state.nav_top();
        assert_eq!(state.selected, 1); // snapped past the static header
        state.selected = 2;
        assert!(state.nav_down());
        assert_eq!(state.selected, 5); // skipped the spacer at 3 and the header at 4
        assert!(state.nav_up());
        assert_eq!(state.selected, 2);
        state.selected = 1;
        assert!(!state.nav_up()); // nothing selectable above the first repo
        assert_eq!(state.selected, 1);
        state.selected = 6;
        assert!(state.nav_down());
        assert_eq!(state.selected, 7); // the Result row stays reachable
    }

    #[test]
    fn collapsible_headers_are_selectable_and_report_no_repo() {
        let mut state = grouped_state(
            &["b-1", "b-2", "b-3"],
            r#"{"collapse_threshold": 2, "groups": [{"name": "b", "pattern": "b-*"}]}"#,
        );
        state.selected = 0;
        assert_eq!(
            state.selected_row(),
            Some(ListRow::group(0, true))
        );
        assert_eq!(state.selected_repo_index(), None);
        assert!(!state.selected_repo_retryable());
    }

    #[test]
    fn toggle_group_collapsed_keeps_selection_valid() {
        let mut state = grouped_state(
            &["b-1", "b-2", "b-3"],
            r#"{"collapse_threshold": 2, "groups": [{"name": "b", "pattern": "b-*"}]}"#,
        );
        // Rows: [header, b-1, b-2, b-3, Result]. Select the last repo, then collapse.
        state.selected = 3;
        state.toggle_group_collapsed(0, None);
        assert!(state.collapsed_groups.contains("b"));
        // Rows now: [header, Result] — the selection landed on a selectable row.
        assert!(state.selected < state.list_len());
        let rows = state.visible_rows();
        assert!(AppState::row_selectable_in(&rows, state.list_len(), state.selected));
        state.toggle_group_collapsed(0, None);
        assert!(!state.collapsed_groups.contains("b"));
    }

    #[test]
    fn reselect_repo_follows_the_repo_across_layout_changes() {
        let mut state = grouped_state(
            &["zeta", "mfe-a", "core"],
            r#"{"groups": [{"name": "frontend", "pattern": "mfe-*"}]}"#,
        );
        // Grouped rows (ungrouped sorted name asc): [header, mfe-a(1), spacer, header, core(2), zeta(0)].
        // Select core at row 4.
        state.selected = 4;
        let prev = state.selected_repo_index();
        assert_eq!(prev, Some(2));
        state.grouping_enabled = false;
        state.reselect_repo(prev);
        assert_eq!(state.selected_repo_index(), Some(2));
        state.grouping_enabled = true;
        state.reselect_repo(Some(2));
        assert_eq!(state.selected, 4);
    }

    #[test]
    fn sort_applies_within_each_group() {
        let mut state = grouped_state(
            &["mfe-c", "plain-b", "mfe-a", "plain-a"],
            r#"{"groups": [{"name": "frontend", "pattern": "mfe-*"}]}"#,
        );
        state.sort_column = SortColumn::Name;
        assert_eq!(
            state.visible_rows(),
            vec![
                ListRow::group(0, false),
                ListRow::repo(2), // mfe-a
                ListRow::repo(0), // mfe-c
                ListRow::Spacer,
                ListRow::group(1, false),
                ListRow::repo(3), // plain-a
                ListRow::repo(1), // plain-b
            ]
        );
    }

    #[test]
    fn nav_left_jumps_to_header_then_collapses_and_nav_right_expands() {
        // Rows: [collapsible header, b-1, b-2, b-3, spacer, static header, other].
        let mut state = grouped_state(
            &["b-1", "b-2", "b-3", "other"],
            r#"{"collapse_threshold": 2, "groups": [{"name": "b", "pattern": "b-*"}]}"#,
        );
        state.selected = 3; // b-3
        state.nav_left();
        assert_eq!(state.selected, 0); // jumped to the group's header
        assert!(!state.collapsed_groups.contains("b"));
        state.nav_left();
        assert!(state.collapsed_groups.contains("b")); // second ← collapses
        state.nav_left(); // already collapsed — no-op
        assert!(state.collapsed_groups.contains("b"));
        state.nav_right();
        assert!(!state.collapsed_groups.contains("b")); // → expands
        state.nav_right(); // already expanded — no-op
        assert!(!state.collapsed_groups.contains("b"));
    }

    #[test]
    fn nav_left_is_inert_under_a_static_header() {
        // "other" sits under the static ungrouped header — not selectable, so ← stays put.
        let mut state = grouped_state(
            &["b-1", "b-2", "b-3", "other"],
            r#"{"collapse_threshold": 2, "groups": [{"name": "b", "pattern": "b-*"}]}"#,
        );
        state.selected = 6; // "other", under the static header at 5
        state.nav_left();
        assert_eq!(state.selected, 6);
        assert!(state.collapsed_groups.is_empty());
    }

    #[test]
    fn init_groups_reports_invalid_and_duplicate_defs() {
        let mut state = state_named(&["a"]);
        let config: GroupsConfig = serde_json::from_str(
            r#"{"groups": [
                {"name": "ok", "pattern": "a*"},
                {"name": "ok", "pattern": "b*"},
                {"name": "broken"}
            ]}"#,
        )
        .unwrap();
        let errors = state.init_groups(config, &GroupsCache::default());
        assert_eq!(state.groups.len(), 1);
        assert_eq!(errors.len(), 2);
    }

    // Resolve a settings row by its label, so these tests don't hard-code the (alphabetical) row
    // indices and survive any future reorder of the settings sections.
    fn srow(label: &str) -> usize {
        crate::app::SETTINGS_LABELS
            .iter()
            .position(|candidate| *candidate == label)
            .unwrap_or_else(|| panic!("no settings row labelled {label:?}"))
    }

    #[test]
    fn set_setting_option_sets_exact_values() {
        let mut state = state_named(&["a"]);
        state.set_setting_option(srow("Grouping"), 0);
        assert!(state.grouping_enabled);
        state.set_setting_option(srow("Grouping"), 1);
        assert!(!state.grouping_enabled);
        state.set_setting_option(srow("Tree view"), 0);
        assert!(state.tree_enabled);
        state.set_setting_option(srow("Tree view"), 1);
        assert!(!state.tree_enabled);
        state.set_setting_option(srow("Hide folder lines"), 0);
        assert!(state.hide_folder_lines);
        state.set_setting_option(srow("Hide folder lines"), 1);
        assert!(!state.hide_folder_lines);
        // Hide zeros toggles with the Unicode set.
        state.set_setting_option(srow("Icons"), 0);
        assert_eq!(state.icon_style, IconStyle::Unicode);
        state.set_setting_option(srow("Hide zeros"), 0);
        assert!(state.hide_zero_counts);
        state.set_setting_option(srow("Hide zeros"), 1);
        assert!(!state.hide_zero_counts);
        state.set_setting_option(srow("Icons"), 1);
        assert_eq!(state.icon_style, IconStyle::Emoji);
        // Under emoji, Hide zeros is inert — a click can't turn it on.
        state.set_setting_option(srow("Hide zeros"), 0);
        assert!(!state.hide_zero_counts);
        state.set_setting_option(srow("Icons"), 0);
        state.set_setting_option(srow("Theme"), 1);
        assert_eq!(state.theme, Theme::Dark);
        state.set_setting_option(srow("Theme"), 2);
        assert_eq!(state.theme, Theme::Light);
        state.set_setting_option(srow("Background"), 1);
        assert_eq!(state.background, Background::Soft);
        state.set_setting_option(srow("Background"), 0);
        assert_eq!(state.background, Background::Normal);
        state.set_setting_option(srow("Contrast"), 1);
        assert_eq!(state.contrast, Contrast::Soft);
        state.set_setting_option(srow("Button hover"), 0);
        assert_eq!(state.button_hover_style, ButtonHoverStyle::Inverted);
        state.set_setting_option(srow("Button hover"), 1);
        assert_eq!(state.button_hover_style, ButtonHoverStyle::Subtle);
        state.set_setting_option(srow("Panel padding"), 1);
        assert!(!state.panel_padding);
        state.set_setting_option(srow("Panel padding"), 0);
        assert!(state.panel_padding);
        state.set_setting_option(srow("Borders"), 1);
        assert!(!state.show_borders);
        state.set_setting_option(srow("Auto branch-check"), 1);
        assert_eq!(state.branch_check, crate::app::BranchCheck::Auto);
        // Out-of-range pairs are a no-op (invalid option, then invalid row).
        let theme = state.theme;
        state.set_setting_option(srow("Theme"), 9);
        state.set_setting_option(99, 0);
        assert_eq!(state.theme, theme);
    }

    #[test]
    fn command_applicable_tracks_context() {
        let state = state_named(&["a"]);
        // Always available regardless of context.
        assert!(state.command_applicable(Command::Settings));
        assert!(state.command_applicable(Command::Help));
        assert!(state.command_applicable(Command::Quit));
        // Folding needs tree or grouping active — both off by default.
        assert!(!state.command_applicable(Command::NavLeft));
        assert!(!state.command_applicable(Command::FoldCollapseAll));
        // View toggles need their data: no groups configured, not a nested tree.
        assert!(!state.command_applicable(Command::GroupingToggle));
        assert!(!state.command_applicable(Command::TreeToggle));
        // Repo-only actions track the selection (a single repo is selected by default).
        assert_eq!(
            state.command_applicable(Command::Info),
            state.selected_repo_index().is_some()
        );
    }

    #[test]
    fn any_modal_open_reflects_modal_state() {
        let mut state = state_named(&["a"]);
        // The "What's New" trigger may open the changelog from the real state.json — reset it so the
        // assertion below starts from no-modals.
        state.show_changelog = false;
        assert!(!state.any_modal_open());
        state.show_settings = true;
        assert!(state.any_modal_open());
        state.show_settings = false;
        state.show_help = true;
        assert!(state.any_modal_open());
    }

    #[test]
    fn opening_a_modal_closes_the_others() {
        let mut state = state_named(&["a"]);
        // Help up, then open settings — single-modal invariant: help must close.
        state.open_help();
        assert!(state.show_help && !state.show_settings);
        state.open_settings();
        assert!(state.show_settings && !state.show_help);
        // Build info likewise replaces settings.
        state.open_build_info();
        assert!(state.show_build_info && !state.show_settings && !state.show_help);
        // The finder/picker overlays also clear any open modal.
        state.show_help = true;
        state.open_finder();
        assert!(state.finder.is_some() && !state.show_help);
        state.open_picker();
        assert!(state.picker.is_some() && state.finder.is_none());
        // close_all_modals leaves nothing open.
        state.close_all_modals();
        assert!(!state.any_modal_open() && state.finder.is_none() && state.picker.is_none());
    }

    #[test]
    fn settings_active_option_tracks_current_values() {
        let mut state = state_named(&["a"]);
        // 2-radio: grouping on → option 0, off → option 1.
        let grouping = srow("Grouping");
        state.set_setting_option(grouping, 0);
        assert_eq!(state.settings_active_option(grouping), 0);
        state.set_setting_option(grouping, 1);
        assert_eq!(state.settings_active_option(grouping), 1);
        // 3-radio: theme auto/dark/light → 0/1/2.
        let theme = srow("Theme");
        state.set_setting_option(theme, 2);
        assert_eq!(state.settings_active_option(theme), 2);
        // 4-radio: auto-pull limit 50/100/250/∞ → 0/1/2/3.
        let limit = srow("Auto-pull limit");
        state.set_setting_option(limit, 3);
        assert_eq!(state.settings_active_option(limit), 3);
        // Button hover: inverted/subtle → 0/1.
        let hover = srow("Button hover");
        state.set_setting_option(hover, 0);
        assert_eq!(state.settings_active_option(hover), 0);
        state.set_setting_option(hover, 1);
        assert_eq!(state.settings_active_option(hover), 1);
        // A click on the active option then cycling lands on the next value.
        state.settings_selected = hover;
        let active = state.settings_active_option(hover);
        state.toggle_selected_setting();
        assert_ne!(state.settings_active_option(hover), active);
    }

    #[test]
    fn tooltip_settings_group_toggles_each_area() {
        let mut state = normalized(state_named(&["a"]));
        let all = srow("All tooltips");
        let tip_rows: Vec<usize> = ["All tooltips", "Footer commands", "Column headers", "Group counts", "Settings rows", "Help links"]
            .iter()
            .map(|label| srow(label))
            .collect();
        // All on by default → option 0 for every Tooltips row.
        for &row in &tip_rows {
            assert_eq!(state.settings_active_option(row), 0, "row {row} defaults on");
        }
        // "All tooltips" off cascades to every area.
        state.set_setting_option(all, 1);
        assert!(state.tooltips.all_off());
        assert_eq!(state.settings_active_option(all), 1);
        // Turning one area back on makes the master mixed (neither radio active → option 2).
        state.set_setting_option(srow("Column headers"), 0);
        assert!(state.tooltips.headers && !state.tooltips.footer);
        assert_eq!(state.settings_active_option(all), 2, "mixed → no radio active");
        // "All tooltips" on cascades every area back on.
        state.set_setting_option(all, 0);
        assert!(state.tooltips.all_on());
        // An individual change (settings rows off) flips just that flag.
        state.set_setting_option(srow("Settings rows"), 1);
        assert!(!state.tooltips.settings && state.tooltips.footer);
        // Defaults: every Tooltips row reads as already-default (no reset entry).
        let mut fresh = normalized(state_named(&["a"]));
        fresh.tooltips = crate::app::TooltipPrefs::default();
        for &row in &tip_rows {
            assert_eq!(
                fresh.settings_active_option(row),
                AppState::settings_default_option(row),
                "row {row} is at its default"
            );
        }
        // A reset restores every area to on.
        state.apply_settings_reset();
        assert_eq!(state.tooltips, crate::app::TooltipPrefs::default());
    }

    #[test]
    fn design_layout_cycles_flat_tabbed() {
        use crate::app::DesignLayout;
        assert_eq!(DesignLayout::Flat.cycle(), DesignLayout::Tabbed);
        assert_eq!(DesignLayout::Tabbed.cycle(), DesignLayout::Flat);
        // The design-section names exist for both the vertical tabs and the key-nav modulo.
        assert_eq!(crate::app::DESIGN_SECTIONS.len(), 6);
        assert_eq!(crate::app::DESIGN_SECTIONS[0], "Theming");
    }

    #[test]
    fn build_info_tree_fold_toggle_and_nav() {
        let mut state = state_named(&["a"]);
        state.build_info_tree =
            crate::treeview::DataNode::parse_json(r#"{"a":1,"obj":{"x":1,"y":2},"arr":[1,2]}"#);
        state.build_info_tree_expanded.clear();
        // Collapsed by default → 3 visible rows (a, obj, arr).
        assert_eq!(state.build_info_tree_rows().len(), 3);
        // Unfold all → obj + arr children appear (3 + 2 + 2 = 7).
        state.build_info_fold_all(true);
        assert_eq!(state.build_info_tree_rows().len(), 7);
        // Fold all → back to 3.
        state.build_info_fold_all(false);
        assert_eq!(state.build_info_tree_rows().len(), 3);
        // Toggle the selected container (row 1 = "obj") expands just it (3 + 2 = 5).
        state.build_info_tree_selected = 1;
        state.build_info_toggle_selected();
        assert_eq!(state.build_info_tree_rows().len(), 5);
        // Selecting a child then ← jumps to its parent ("obj" at row 1).
        state.build_info_tree_selected = 2;
        state.build_info_tree_collapse_or_parent();
        assert_eq!(state.build_info_tree_selected, 1);
        // ← again collapses the parent.
        state.build_info_tree_collapse_or_parent();
        assert_eq!(state.build_info_tree_rows().len(), 3);
    }

    #[test]
    fn ensure_build_info_visible_decoupled_from_selection() {
        let mut state = state_named(&["a"]);
        let json = format!(
            "[{}]",
            (0..20).map(|n| n.to_string()).collect::<Vec<_>>().join(",")
        );
        state.build_info_tree = crate::treeview::DataNode::parse_json(&json);
        assert_eq!(state.build_info_tree_rows().len(), 20);
        let viewport = 5; // max_scroll = 20 - 5 = 15

        // A selection already on screen leaves the scroll untouched (web-app style).
        state.build_info_scroll = 0;
        state.build_info_tree_selected = 2;
        state.ensure_build_info_visible(viewport);
        assert_eq!(state.build_info_scroll, 0);

        // Selecting the last row scrolls just enough to show it — pinned to max_scroll.
        state.build_info_tree_selected = 19;
        state.ensure_build_info_visible(viewport);
        assert_eq!(state.build_info_scroll, 15);

        // Selecting above the viewport snaps the top of the view to the selection.
        state.build_info_tree_selected = 7;
        state.ensure_build_info_visible(viewport);
        assert_eq!(state.build_info_scroll, 7);

        // Scroll set independently of the selection (a wheel bump) is left alone as long as the
        // selection stays on screen — the two are decoupled.
        state.build_info_scroll = 5; // rows 5..9 visible; selection 7 is in view
        state.ensure_build_info_visible(viewport);
        assert_eq!(state.build_info_scroll, 5);

        // A zero viewport (modal too small / pre-first-render) is a no-op.
        state.build_info_scroll = 4;
        state.ensure_build_info_visible(0);
        assert_eq!(state.build_info_scroll, 4);
    }

    #[test]
    fn button_hover_style_cycles() {
        assert_eq!(ButtonHoverStyle::Subtle.cycle(), ButtonHoverStyle::Inverted);
        assert_eq!(ButtonHoverStyle::Inverted.cycle(), ButtonHoverStyle::Subtle);
    }

    #[test]
    fn format_ago_picks_coarse_units() {
        assert_eq!(format_ago(0), "just now");
        assert_eq!(format_ago(59), "just now");
        assert_eq!(format_ago(60), "1m ago");
        assert_eq!(format_ago(3_599), "59m ago");
        assert_eq!(format_ago(3_600), "1h ago");
        assert_eq!(format_ago(86_399), "23h ago");
        assert_eq!(format_ago(86_400), "1d ago");
        assert_eq!(format_ago(700_000), "8d ago");
    }

    #[test]
    fn region_and_rect_hit_testing() {
        assert!(region_hit(Some((5, 10, 13)), 10, 5));
        assert!(region_hit(Some((5, 10, 13)), 12, 5));
        assert!(!region_hit(Some((5, 10, 13)), 13, 5)); // end is exclusive
        assert!(!region_hit(Some((5, 10, 13)), 10, 6));
        assert!(!region_hit(None, 10, 5));
        let rect = Rect { x: 2, y: 3, width: 4, height: 2 };
        assert!(point_in(rect, 2, 3));
        assert!(point_in(rect, 5, 4));
        assert!(!point_in(rect, 6, 4));
        assert!(!point_in(rect, 5, 5));
    }

    #[test]
    fn settings_hit_at_resolves_labels_and_chips() {
        let mut state = state_named(&["a"]);
        state.settings_click = vec![
            (8, 4, 18, 0, None),     // row 0 label
            (8, 18, 22, 0, Some(0)), // row 0 first chip
            (9, 18, 24, 1, Some(1)), // row 1 second chip
        ];
        assert_eq!(state.settings_hit_at(5, 8), Some((0, None)));
        assert_eq!(state.settings_hit_at(19, 8), Some((0, Some(0))));
        assert_eq!(state.settings_hit_at(20, 9), Some((1, Some(1))));
        assert_eq!(state.settings_hit_at(30, 8), None);
        assert_eq!(state.settings_hit_at(19, 10), None);
    }

    #[test]
    fn init_groups_ignores_cache_with_stale_fingerprint() {
        use crate::groups::CacheEntry;
        let mut state = state_named(&["repo-a"]);
        let config: GroupsConfig = serde_json::from_str(
            r#"{"groups": [{"name": "dyn", "command": "echo repo-a"}]}"#,
        )
        .unwrap();
        let mut cache = GroupsCache::default();
        cache.entries.insert(
            "dyn".to_string(),
            CacheEntry {
                resolved_at: 123,
                fingerprint: "command:echo something-else".to_string(),
                members: vec!["repo-a".to_string()],
            },
        );
        state.init_groups(config, &cache);
        // Fingerprint mismatch → cached members ignored, group unresolved.
        assert_eq!(state.groups[0].members, None);
        assert_eq!(state.groups[0].resolved_at, None);
    }
