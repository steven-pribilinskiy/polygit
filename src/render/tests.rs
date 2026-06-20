    use super::*;

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
            diff_modal_footer(source, focus, chips, crate::app::DiffView::Raw)
                .iter()
                .map(|(text, _, _)| text.as_str())
                .collect()
        };
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
