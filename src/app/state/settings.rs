//! The settings dialog: tabs, sections, search, and per-row option state.

use super::*;

impl AppState {
    pub fn settings_hit_at(&self, col: u16, row: u16) -> Option<(usize, Option<usize>)> {
        self.settings_click
            .iter()
            .find(|(region_row, start, end, _, _)| {
                *region_row == row && col >= *start && col < *end
            })
            .map(|(_, _, _, row_idx, option)| (*row_idx, *option))
    }

    pub fn set_setting_option(&mut self, row_idx: usize, option_idx: usize) {
        // Rows are in alphabetical-section order (see SETTINGS_LABELS): Agent · Interaction · Layout
        // · Lists · Pull requests · Sync · Theming · Tooltips.
        let Some(&label) = crate::app::SETTINGS_LABELS.get(row_idx) else {
            return;
        };
        match (label, option_idx) {
            // Agent
            ("AI agent", 0) => self.claude_agent = ClaudeAgent::Claude,
            ("AI agent", 1) => self.claude_agent = ClaudeAgent::Codex,
            ("AI agent", 2) => self.claude_agent = ClaudeAgent::Gemini,
            ("Skip permissions", 0) => self.claude_skip_permissions = true,
            ("Skip permissions", 1) => self.claude_skip_permissions = false,
            // Cloning
            ("Clone history", 0) => self.coverage_prefs.blobless = false,
            ("Clone history", 1) => self.coverage_prefs.blobless = true,
            ("Clone size cap", index) => {
                self.coverage_prefs.max_size_kb = Self::CLONE_SIZE_CAPS_KB
                    .get(index)
                    .copied()
                    .unwrap_or(0);
            }
            ("Fork placement", 0) => self.coverage_prefs.forks_subdir = "forks".to_string(),
            ("Fork placement", 1) => self.coverage_prefs.forks_subdir = String::new(),
            ("Layout", index) => {
                self.coverage_prefs.layout =
                    Self::CLONE_LAYOUTS.get(index).copied().unwrap_or("{repo}").to_string();
            }
            ("Prefix depth", index) => self.coverage_prefs.prefix_depth = index + 1,
            // Interaction
            ("Hover effects", 0) => self.hover_effects = true,
            ("Hover effects", 1) => self.hover_effects = false,
            ("Changed-row effect", 0) => self.changed_row_effect = ChangedRowEffect::Off,
            ("Changed-row effect", 1) => self.changed_row_effect = ChangedRowEffect::Flash,
            ("Changed-row effect", 2) => self.changed_row_effect = ChangedRowEffect::Highlight,
            // Layout
            ("Panel padding", 0) => self.panel_padding = true,
            ("Panel padding", 1) => self.panel_padding = false,
            ("Borders", 0) => self.show_borders = true,
            ("Borders", 1) => self.show_borders = false,
            ("Pane splitter", 0) => self.splitter_mode = SplitterMode::Dedicated,
            ("Pane splitter", 1) => self.splitter_mode = SplitterMode::Hover,
            ("Repo page tabs", 0) => {
                self.repo_page_tabs = RepoTabsMode::Off;
                self.repo_page_tabbed_override = None; // changing the preference clears any `v` flip
            }
            ("Repo page tabs", 1) => {
                self.repo_page_tabs = RepoTabsMode::Auto;
                self.repo_page_tabbed_override = None;
            }
            ("Auto branch-check", 0) => self.branch_check = BranchCheck::Off,
            ("Auto branch-check", 1) => self.branch_check = BranchCheck::Auto,
            ("Info layout", 0) => self.info_layout = crate::app::InfoLayout::Sections,
            ("Info layout", 1) => self.info_layout = crate::app::InfoLayout::Groups,
            ("Info layout", 2) => self.info_layout = crate::app::InfoLayout::Flat,
            // Layout density preset (derived, not stored — see `layout_density`). Compact/Spacious
            // apply their bundle to the three fields above; Custom has no bundle of its own, so
            // clicking it is a no-op.
            ("Layout density", 0) => {
                self.panel_padding = false;
                self.show_borders = false;
                self.splitter_mode = SplitterMode::Hover;
            }
            ("Layout density", 1) => {
                self.panel_padding = true;
                self.show_borders = true;
                self.splitter_mode = SplitterMode::Dedicated;
            }
            ("Layout density", 2) => {}
            // Lists
            ("Grouping", 0) | ("Grouping", 1) => {
                let enable = option_idx == 0;
                if self.grouping_enabled != enable {
                    let prev = self.selected_repo_index();
                    self.grouping_enabled = enable;
                    self.reselect_repo(prev);
                }
            }
            ("Tree view", 0) | ("Tree view", 1) => {
                let enable = option_idx == 0;
                if self.tree_enabled != enable {
                    let prev = self.selected_repo_index();
                    self.tree_enabled = enable;
                    self.reselect_repo(prev);
                }
            }
            ("Hide folder lines", 0) => self.hide_folder_lines = true,
            ("Hide folder lines", 1) => self.hide_folder_lines = false,
            // Pull requests
            ("Merged PRs", 0) => self.show_merged_prs = true,
            ("Merged PRs", 1) => self.show_merged_prs = false,
            // Sync
            ("Auto-pull on launch", 0) => self.auto_pull_on_launch = true,
            ("Auto-pull on launch", 1) => self.auto_pull_on_launch = false,
            ("Auto-pull limit", 0) => self.auto_pull_max_repos = 50,
            ("Auto-pull limit", 1) => self.auto_pull_max_repos = 100,
            ("Auto-pull limit", 2) => self.auto_pull_max_repos = 250,
            ("Auto-pull limit", 3) => self.auto_pull_max_repos = 0,
            ("Auto-pull in tree", 0) => self.auto_pull_in_tree = true,
            ("Auto-pull in tree", 1) => self.auto_pull_in_tree = false,
            // Theming
            ("Icons", 0) => self.icon_style = IconStyle::Unicode,
            ("Icons", 1) => self.icon_style = IconStyle::Emoji,
            // Hide zeros is forced on (and inert) in emoji mode — ignore clicks then.
            ("Hide zeros", 0) if self.icon_style != IconStyle::Emoji => self.hide_zero_counts = true,
            ("Hide zeros", 1) if self.icon_style != IconStyle::Emoji => self.hide_zero_counts = false,
            ("Theme", 0) => self.theme = Theme::Auto,
            ("Theme", 1) => self.theme = Theme::Dark,
            ("Theme", 2) => self.theme = Theme::Light,
            ("Background", 0) => self.background = Background::Normal,
            ("Background", 1) => self.background = Background::Soft,
            ("Background", 2) => self.background = Background::Terminal,
            ("Contrast", 0) => self.contrast = Contrast::Normal,
            ("Contrast", 1) => self.contrast = Contrast::Soft,
            ("List selection", 0) => self.selection_style = SelectionStyle::Blue,
            ("List selection", 1) => self.selection_style = SelectionStyle::Subtle,
            ("Button hover", 0) => self.button_hover_style = ButtonHoverStyle::Inverted,
            ("Button hover", 1) => self.button_hover_style = ButtonHoverStyle::Subtle,
            // Tooltips
            ("All tooltips", 0) => self.tooltips.set_all(true),
            ("All tooltips", 1) => self.tooltips.set_all(false),
            ("Footer commands", 0) => self.tooltips.footer = true,
            ("Footer commands", 1) => self.tooltips.footer = false,
            ("Column headers", 0) => self.tooltips.headers = true,
            ("Column headers", 1) => self.tooltips.headers = false,
            ("Group counts", 0) => self.tooltips.counts = true,
            ("Group counts", 1) => self.tooltips.counts = false,
            ("Settings rows", 0) => self.tooltips.settings = true,
            ("Settings rows", 1) => self.tooltips.settings = false,
            ("Help links", 0) => self.tooltips.links = true,
            ("Help links", 1) => self.tooltips.links = false,
            // Updates
            ("Auto-update", 0) => self.auto_update = AutoUpdate::Off,
            ("Auto-update", 1) => self.auto_update = AutoUpdate::Notify,
            ("Auto-update", 2) => self.auto_update = AutoUpdate::Install,
            ("Update check", 0) => self.update_interval = UpdateInterval::Daily,
            ("Update check", 1) => self.update_interval = UpdateInterval::Weekly,
            // Workers
            ("Parallel pulls", 0) => self.set_max_pull_mode(crate::app::MaxPullMode::Exact),
            ("Parallel pulls", 1) => self.set_max_pull_mode(crate::app::MaxPullMode::Percent),
            // The value is picked from a dropdown, not a radio — a no-op here keeps the round-trip.
            ("Parallel value", _) => {}
            _ => return,
        }
        self.save_state();
    }

    pub fn layout_density(&self) -> usize {
        if !self.panel_padding && !self.show_borders && self.splitter_mode == SplitterMode::Hover {
            0
        } else if self.panel_padding
            && self.show_borders
            && self.splitter_mode == SplitterMode::Dedicated
        {
            1
        } else {
            2
        }
    }

    pub fn settings_active_option(&self, row_idx: usize) -> usize {
        let Some(&label) = crate::app::SETTINGS_LABELS.get(row_idx) else {
            return 0;
        };
        match label {
            // Agent
            "AI agent" => match self.claude_agent {
                ClaudeAgent::Claude => 0,
                ClaudeAgent::Codex => 1,
                ClaudeAgent::Gemini => 2,
            },
            "Skip permissions" => usize::from(!self.claude_skip_permissions),
            // Cloning
            "Clone history" => usize::from(self.coverage_prefs.blobless),
            "Clone size cap" => Self::CLONE_SIZE_CAPS_KB
                .iter()
                .position(|cap| *cap == self.coverage_prefs.max_size_kb)
                .unwrap_or(0),
            "Fork placement" => usize::from(self.coverage_prefs.forks_subdir.is_empty()),
            "Layout" => Self::CLONE_LAYOUTS
                .iter()
                .position(|template| *template == self.coverage_prefs.layout)
                .unwrap_or(0),
            "Prefix depth" => self.coverage_prefs.prefix_depth.saturating_sub(1).min(2),
            // Interaction
            "Hover effects" => usize::from(!self.hover_effects),
            "Changed-row effect" => match self.changed_row_effect {
                ChangedRowEffect::Off => 0,
                ChangedRowEffect::Flash => 1,
                ChangedRowEffect::Highlight => 2,
            },
            // Layout
            "Panel padding" => usize::from(!self.panel_padding),
            "Borders" => usize::from(!self.show_borders),
            "Pane splitter" => match self.splitter_mode {
                SplitterMode::Dedicated => 0,
                SplitterMode::Hover => 1,
            },
            "Repo page tabs" => match self.repo_page_tabs {
                RepoTabsMode::Off => 0,
                RepoTabsMode::Auto => 1,
            },
            "Auto branch-check" => match self.branch_check {
                BranchCheck::Off => 0,
                BranchCheck::Auto => 1,
            },
            "Info layout" => match self.info_layout {
                crate::app::InfoLayout::Sections => 0,
                crate::app::InfoLayout::Groups => 1,
                crate::app::InfoLayout::Flat => 2,
            },
            "Layout density" => self.layout_density(),
            // Lists
            "Grouping" => usize::from(!self.grouping_enabled),
            "Tree view" => usize::from(!self.tree_enabled),
            "Hide folder lines" => usize::from(!self.hide_folder_lines),
            // Pull requests
            "Merged PRs" => usize::from(!self.show_merged_prs),
            // Sync
            "Auto-pull on launch" => usize::from(!self.auto_pull_on_launch),
            "Auto-pull limit" => match self.auto_pull_max_repos {
                50 => 0,
                100 => 1,
                250 => 2,
                _ => 3,
            },
            "Auto-pull in tree" => usize::from(!self.auto_pull_in_tree),
            // Theming
            "Icons" => match self.icon_style {
                IconStyle::Unicode => 0,
                IconStyle::Emoji => 1,
            },
            // Emoji always hides zeros → force-selected "on" regardless of the stored flag.
            "Hide zeros" => usize::from(!(self.hide_zero_counts || self.icon_style == IconStyle::Emoji)),
            "Theme" => match self.theme {
                Theme::Auto => 0,
                Theme::Dark => 1,
                Theme::Light => 2,
            },
            "Background" => match self.background {
                Background::Normal => 0,
                Background::Soft => 1,
                Background::Terminal => 2,
            },
            "Contrast" => match self.contrast {
                Contrast::Normal => 0,
                Contrast::Soft => 1,
            },
            "List selection" => match self.selection_style {
                SelectionStyle::Blue => 0,
                SelectionStyle::Subtle => 1,
            },
            "Button hover" => match self.button_hover_style {
                ButtonHoverStyle::Inverted => 0,
                ButtonHoverStyle::Subtle => 1,
            },
            // Tooltips — All tooltips: 0 = all on, 1 = all off, 2 = mixed (neither radio active).
            "All tooltips" => {
                if self.tooltips.all_on() {
                    0
                } else if self.tooltips.all_off() {
                    1
                } else {
                    2
                }
            }
            "Footer commands" => usize::from(!self.tooltips.footer),
            "Column headers" => usize::from(!self.tooltips.headers),
            "Group counts" => usize::from(!self.tooltips.counts),
            "Settings rows" => usize::from(!self.tooltips.settings),
            "Help links" => usize::from(!self.tooltips.links),
            // Updates
            "Auto-update" => match self.auto_update {
                AutoUpdate::Off => 0,
                AutoUpdate::Notify => 1,
                AutoUpdate::Install => 2,
            },
            "Update check" => match self.update_interval {
                UpdateInterval::Daily => 0,
                UpdateInterval::Weekly => 1,
            },
            // Workers — row 33 mode (exact/percent); row 34 is the dropdown value (single chip).
            "Parallel pulls" => match self.max_pull_mode {
                crate::app::MaxPullMode::Exact => 0,
                crate::app::MaxPullMode::Percent => 1,
            },
            "Parallel value" => 0,
            _ => 0,
        }
    }

    pub fn settings_option_labels(row: usize) -> &'static [&'static str] {
        let Some(&label) = crate::app::SETTINGS_LABELS.get(row) else {
            return Default::default();
        };
        match label {
            "AI agent" => &["claude", "codex", "gemini"],
            "Clone history" => &["full", "blobless"],
            "Clone size cap" => &["off", "1 GB", "2 GB", "5 GB"],
            "Fork placement" => &["subdir", "alongside"],
            "Layout" => &["flat", "by project", "by family", "by owner"],
            "Prefix depth" => &["1", "2", "3"],
            "Changed-row effect" => &["off", "flash", "highlight"],
            "Pane splitter" => &["dedicated", "on hover"],
            "Repo page tabs" => &["off", "auto"],
            "Auto branch-check" => &["off", "auto"],
            "Info layout" => &["titled", "spaced", "flat"],
            "Layout density" => &["compact", "spacious", "custom"],
            "Auto-pull limit" => &["50", "100", "250", "\u{221e}"],
            "Icons" => &["unicode", "emoji"],
            "Theme" => &["auto", "dark", "light"],
            "Background" => &["normal", "soft", "terminal"],
            "Contrast" => &["normal", "soft"],
            "List selection" => &["blue", "subtle"],
            "Button hover" => &["inverted", "subtle"],
            "Auto-update" => &["off", "notify", "install"],
            "Update check" => &["daily", "weekly"],
            "Parallel pulls" => &["exact", "percent"],
            // The value row is a dropdown, not radio chips; this static placeholder just satisfies
            // the "one option, round-trips" invariant — the real chip label is built at render time.
            "Parallel value" => &["value"],
            _ => &["on", "off"],
        }
    }

    pub fn settings_default_option(row: usize) -> usize {
        let Some(&label) = crate::app::SETTINGS_LABELS.get(row) else {
            return Default::default();
        };
        match label {
            // Rows whose DEFAULT is the first option (index 0). Agent: AI agent→claude(0). Interaction:
            // hover on(2). Layout: panel padding on(4), borders on(5), branch-check off(8), info layout
            // titled(9). Lists: grouping on(11). Sync: auto-pull-on-launch(15). Theming: icons unicode(18),
            // theme auto(20), background normal(21), contrast normal(22), selection blue(23). Tooltips
            // (25–30) all on.
            // Updates: update-check daily defaults to option 0; auto-update defaults to option 1
            // (notify) — handled by the `_ => 1` arm below.
            // Cloning: every row's default is its first option — full history, no size cap, forks in
            // a subdir, flat layout, prefix depth 1.
            "Clone history" | "Clone size cap" | "Fork placement" | "Layout" | "Prefix depth"
            | "AI agent" | "Hover effects" | "Panel padding" | "Borders" | "Auto branch-check" | "Info layout" | "Grouping" | "Auto-pull on launch" | "Icons" | "Theme" | "Background" | "Contrast" | "List selection" | "All tooltips" | "Footer commands" | "Column headers" | "Group counts" | "Settings rows" | "Help links" | "Update check" | "Parallel value" => 0,
            // Layout density(10) — derived; the shipped/reset field values (padding+borders on,
            // splitter on-hover) don't exactly match either named bundle, so its "default" is
            // custom(2), not compact/spacious. See `layout_density`.
            "Layout density" => 2,
            // Index-1 defaults: changed-row effect flash(3), pane splitter on-hover(6), repo-page-tabs
            // auto(7), auto-pull-limit 100(16), button-hover subtle(24), parallel-pulls mode percent(33),
            // and every remaining boolean off.
            _ => 1,
        }
    }

    pub fn settings_reset_plan(&self) -> Vec<String> {
        (0..Self::SETTINGS_ROWS)
            // "All tooltips" is derived from the per-area rows below it — by label, not a hardcoded
            // index that drifts when sections are reordered.
            .filter(|&row| row != crate::app::settings_row("All tooltips"))
            .filter_map(|row| {
                let current = self.settings_active_option(row);
                let default = Self::settings_default_option(row);
                if current == default {
                    return None;
                }
                let labels = Self::settings_option_labels(row);
                Some(format!(
                    "{}: {} \u{2192} {}",
                    SETTINGS_LABELS[row],
                    labels.get(current).copied().unwrap_or("?"),
                    labels.get(default).copied().unwrap_or("?"),
                ))
            })
            .collect()
    }

    pub fn apply_settings_reset(&mut self) {
        // These MUST mirror the field defaults in persist.rs (and the indices in
        // `settings_default_option`) — a divergence makes "reset" leave a field off-default or the
        // confirmation say "already at defaults" when it isn't.
        self.grouping_enabled = true;
        self.tree_enabled = false;
        self.hide_folder_lines = false;
        self.icon_style = IconStyle::Unicode;
        self.hide_zero_counts = false;
        self.theme = Theme::Auto;
        self.background = Background::Normal;
        self.contrast = Contrast::Normal;
        self.selection_style = SelectionStyle::Blue;
        self.button_hover_style = ButtonHoverStyle::Subtle;
        self.auto_pull_on_launch = true;
        self.auto_pull_max_repos = 100;
        self.auto_pull_in_tree = false;
        self.hover_effects = true;
        self.changed_row_effect = ChangedRowEffect::default();
        self.panel_padding = true;
        self.show_borders = true;
        self.splitter_mode = SplitterMode::Hover;
        self.repo_page_tabs = RepoTabsMode::Auto;
        self.repo_page_tabbed_override = None;
        self.maximized = None;
        self.branch_check = BranchCheck::Off;
        self.info_layout = crate::app::InfoLayout::default();
        self.tooltips = TooltipPrefs::default();
        self.claude_agent = ClaudeAgent::default();
        self.claude_skip_permissions = false;
        self.show_merged_prs = false;
        // Max parallel pulls → default (Percent 100% = all cores); apply it live.
        self.max_pull_mode = crate::app::MaxPullMode::Percent;
        self.max_pull_exact = 0;
        self.max_pull_percent = 100;
        self.apply_max_pull();
        self.recompute_group_assignments();
        self.rebuild_tree();
        self.save_state();
    }

    pub fn open_settings_reset_confirm(&mut self) {
        let plan = self.settings_reset_plan();
        if plan.is_empty() {
            self.show_toast("settings already at defaults".to_string());
            return;
        }
        // Single-modal invariant: replace the settings modal with the confirm (which renders + takes
        // input on top of the main view). Settings stays closed after; reopen with `,` if needed.
        self.show_settings = false;
        self.settings_clear_search();
        let count = plan.len();
        let plural = if count == 1 { "" } else { "s" };
        self.confirm = Some(ConfirmDialog {
            message: format!("Reset {count} setting{plural} to defaults?"),
            action: ConfirmAction::ResetSettings,
            danger: false,
            restore_files: Vec::new(),
            delete_files: Vec::new(),
            detail_lines: plan,
            detail_title: Some("Will reset:".to_string()),
            copy_line: None,
        });
    }

    pub const SETTINGS_ROWS: usize = 40;

    /// One-line tooltip for a settings row (or a specific option, where it adds something) —
    /// shown after ~1s of hovering, like the footer command tooltips. Keyed by the global row
    /// index (see `SETTINGS_TABS`) and the hovered option, if any.
    pub fn settings_tip(row: usize, option: Option<usize>) -> Option<&'static str> {
        // Derived from the single-source `SETTINGS` table (co-located label + tip), so a tooltip
        // can never drift to the wrong row on a reorder/insert. An option-specific tip (e.g. the
        // Icons unicode/emoji rows) wins when present; otherwise the row's general tip.
        let info = crate::app::SETTINGS.get(row)?;
        if let Some(opt) = option {
            if let Some(tip) = info.option_tips.get(opt) {
                return Some(tip);
            }
        }
        Some(info.tip)
    }

    pub fn settings_tab_range(tab: usize) -> (usize, usize) {
        let start: usize = SETTINGS_TABS.iter().take(tab).map(|(_, count)| count).sum();
        let len = SETTINGS_TABS.get(tab).map_or(0, |(_, count)| *count);
        (start, len)
    }

    pub fn settings_tabbed_blank_before(row: usize) -> bool {
        // Blank before Theme (5) — separates the Icons group (Icons + Hide zeros) from the palette
        // group — and before List selection (8) — groups List selection + Button hover.
        row == 5 || row == 8
    }

    pub fn settings_tab_of_row(row: usize) -> usize {
        let mut acc = 0;
        for (tab, (_, count)) in SETTINGS_TABS.iter().enumerate() {
            acc += count;
            if row < acc {
                return tab;
            }
        }
        SETTINGS_TABS.len().saturating_sub(1)
    }

    pub fn settings_select_tab(&mut self, tab: usize) {
        if tab >= SETTINGS_TABS.len() {
            return;
        }
        self.settings_tab = tab;
        self.settings_selected = Self::settings_tab_range(tab).0;
    }

    pub fn settings_cycle_tab(&mut self, forward: bool) {
        let count = SETTINGS_TABS.len();
        let next = if forward {
            (self.settings_tab + 1) % count
        } else {
            (self.settings_tab + count - 1) % count
        };
        self.settings_select_tab(next);
    }

    pub fn settings_section_collapsed(&self, tab_idx: usize) -> bool {
        SETTINGS_TABS
            .get(tab_idx)
            .is_some_and(|(name, _)| self.collapsed_settings.contains(*name))
    }

    pub fn toggle_settings_section(&mut self, tab_idx: usize) {
        let Some((name, _)) = SETTINGS_TABS.get(tab_idx) else {
            return;
        };
        if self.collapsed_settings.contains(*name) {
            self.collapsed_settings.remove(*name);
        } else {
            self.collapsed_settings.insert((*name).to_string());
        }
        self.save_state();
    }

    pub fn set_selected_settings_section(&mut self, collapse: bool) {
        if self.settings_layout != SettingsLayout::Accordion {
            return;
        }
        let tab = self
            .settings_on_header
            .unwrap_or_else(|| Self::settings_tab_of_row(self.settings_selected));
        if self.settings_section_collapsed(tab) != collapse {
            self.toggle_settings_section(tab);
        }
        // Collapsing hides the section's rows. If focus was on one of those rows, move it to the
        // section header — otherwise the selection would point at a now-hidden row and nothing would
        // read as focused (you couldn't tell what just happened). The header then shows its
        // highlight, so a left-press always lands somewhere visible.
        if collapse {
            self.settings_on_header = Some(tab);
        }
    }

    pub fn settings_all_collapsed(&self) -> bool {
        SETTINGS_TABS.iter().all(|(name, _)| self.collapsed_settings.contains(*name))
    }

    pub fn toggle_all_settings_sections(&mut self) {
        if self.settings_all_collapsed() {
            self.collapsed_settings.clear();
        } else {
            for (name, _) in SETTINGS_TABS {
                self.collapsed_settings.insert((*name).to_string());
            }
        }
        self.save_state();
    }

    pub fn accordion_positions(&self) -> Vec<AccPos> {
        let mut positions = Vec::new();
        let mut row = 0usize;
        for (section, (_, count)) in SETTINGS_TABS.iter().enumerate() {
            positions.push(AccPos::Header(section));
            let collapsed = self.settings_section_collapsed(section);
            for _ in 0..*count {
                if !collapsed {
                    positions.push(AccPos::Row(row));
                }
                row += 1;
            }
        }
        positions
    }

    pub fn accordion_selection(&self) -> AccPos {
        match self.settings_on_header {
            Some(section) => AccPos::Header(section),
            None => AccPos::Row(self.settings_selected),
        }
    }

    fn set_accordion_selection(&mut self, position: AccPos) {
        match position {
            AccPos::Header(section) => self.settings_on_header = Some(section),
            AccPos::Row(row) => {
                self.settings_on_header = None;
                self.settings_selected = row;
                self.settings_tab = Self::settings_tab_of_row(row);
            }
        }
    }

    pub fn toggle_focused_accordion_section(&mut self) {
        let section = self
            .settings_on_header
            .unwrap_or_else(|| Self::settings_tab_of_row(self.settings_selected));
        self.toggle_settings_section(section);
    }

    pub fn settings_move(&mut self, delta: isize) {
        // While searching, navigate the flat filtered list regardless of layout.
        if !self.settings_search.is_empty() {
            let matches = self.settings_filtered_rows();
            if matches.is_empty() {
                return;
            }
            let current = matches.iter().position(|&row| row == self.settings_selected).unwrap_or(0);
            let next = (current as isize + delta).clamp(0, matches.len() as isize - 1) as usize;
            self.settings_selected = matches[next];
            self.settings_tab = Self::settings_tab_of_row(self.settings_selected);
            return;
        }
        if self.settings_layout == SettingsLayout::Accordion {
            // Navigate the interleaved header/row sequence (headers are selectable; rows in
            // collapsed sections are skipped because they aren't in `accordion_positions`).
            let positions = self.accordion_positions();
            if positions.is_empty() {
                return;
            }
            let current =
                positions.iter().position(|pos| *pos == self.accordion_selection()).unwrap_or(0);
            let next = (current as isize + delta).clamp(0, positions.len() as isize - 1) as usize;
            self.set_accordion_selection(positions[next]);
            return;
        }
        let (lo, hi) = if self.settings_layout == SettingsLayout::Tabbed {
            let (start, len) = Self::settings_tab_range(self.settings_tab);
            (start as isize, (start + len).saturating_sub(1) as isize)
        } else {
            (0, Self::SETTINGS_ROWS.saturating_sub(1) as isize)
        };
        let current = self.settings_selected as isize;
        self.settings_selected = (current + delta).clamp(lo, hi) as usize;
        self.settings_tab = Self::settings_tab_of_row(self.settings_selected);
    }

    pub fn apply_max_pull(&mut self) {
        let cores = num_cpus::get().max(1);
        let jobs = crate::app::resolve_max_pull(
            self.max_pull_mode,
            self.max_pull_exact,
            self.max_pull_percent,
            cores,
        );
        self.max_jobs = jobs;
        self.throttle.resize(jobs);
        self.save_state();
    }

    pub fn set_max_pull_mode(&mut self, mode: crate::app::MaxPullMode) {
        self.max_pull_mode = mode;
        self.apply_max_pull();
    }

    pub fn toggle_max_pull_mode(&mut self) {
        let next = match self.max_pull_mode {
            crate::app::MaxPullMode::Percent => crate::app::MaxPullMode::Exact,
            crate::app::MaxPullMode::Exact => crate::app::MaxPullMode::Percent,
        };
        self.set_max_pull_mode(next);
    }

    pub fn set_max_pull_value(&mut self, value: u32) {
        match self.max_pull_mode {
            crate::app::MaxPullMode::Exact => self.max_pull_exact = value,
            crate::app::MaxPullMode::Percent => self.max_pull_percent = value,
        }
        self.apply_max_pull();
    }

    pub fn max_pull_value_choices(&self) -> Vec<(u32, String, bool)> {
        let cores = num_cpus::get().max(1);
        match self.max_pull_mode {
            crate::app::MaxPullMode::Exact => {
                let current = if self.max_pull_exact == 0 {
                    cores as u32
                } else {
                    self.max_pull_exact
                };
                crate::app::core_value_steps(cores)
                    .into_iter()
                    .map(|value| (value as u32, value.to_string(), value as u32 == current))
                    .collect()
            }
            crate::app::MaxPullMode::Percent => {
                let current = if self.max_pull_percent == 0 { 100 } else { self.max_pull_percent };
                crate::app::MAX_PULL_PERCENTS
                    .into_iter()
                    .map(|pct| {
                        let jobs = crate::app::resolve_max_pull(
                            crate::app::MaxPullMode::Percent,
                            0,
                            pct,
                            cores,
                        );
                        (pct, format!("{pct}% ({jobs})"), pct == current)
                    })
                    .collect()
            }
        }
    }

    pub fn open_parallel_value_dropdown(&mut self) {
        let value_row = crate::app::settings_row("Parallel value");
        let anchor = self.settings_click.iter().find_map(|&(row, _, end, row_idx, option)| {
            (row_idx == value_row && option == Some(0)).then_some((row, end))
        });
        if let Some((row, end)) = anchor {
            self.open_dropdown(crate::app::DropdownKind::ParallelValue, end, row);
        }
    }

    pub fn max_pull_value_label(&self) -> String {
        match self.max_pull_mode {
            crate::app::MaxPullMode::Exact => self.max_jobs.to_string(),
            crate::app::MaxPullMode::Percent => {
                let pct = if self.max_pull_percent == 0 { 100 } else { self.max_pull_percent };
                format!("{pct}% ({})", self.max_jobs)
            }
        }
    }

    /// Size caps offered by the **Clone size cap** row, in kilobytes. Index 0 is "no limit", which
    /// is the default: skipping large repos silently is how the shell script this replaces lost
    /// them, so the cap is opt-in and always reported.
    pub const CLONE_SIZE_CAPS_KB: [u64; 4] = [0, 1024 * 1024, 2 * 1024 * 1024, 5 * 1024 * 1024];

    /// Templates offered by the **Layout** row. Flat is first because it is the default — most
    /// repos in a real org cluster with nothing, so an eager grouping scatters them.
    pub const CLONE_LAYOUTS: [&str; 4] =
        ["{repo}", "{project}/{repo}", "{group}/{repo}", "{owner}/{repo}"];

    pub fn toggle_selected_setting(&mut self) {
        let Some(&label) = crate::app::SETTINGS_LABELS.get(self.settings_selected) else {
            return;
        };
        match label {
            // Agent
            "AI agent" => self.claude_agent = self.claude_agent.cycle(),
            "Skip permissions" => self.claude_skip_permissions = !self.claude_skip_permissions,
            // Cloning
            "Clone history" => self.coverage_prefs.blobless = !self.coverage_prefs.blobless,
            "Clone size cap" => {
                let next = Self::CLONE_SIZE_CAPS_KB
                    .iter()
                    .position(|cap| *cap == self.coverage_prefs.max_size_kb)
                    .map(|index| (index + 1) % Self::CLONE_SIZE_CAPS_KB.len())
                    .unwrap_or(0);
                self.coverage_prefs.max_size_kb = Self::CLONE_SIZE_CAPS_KB[next];
            }
            "Fork placement" => {
                self.coverage_prefs.forks_subdir = if self.coverage_prefs.forks_subdir.is_empty() {
                    "forks".to_string()
                } else {
                    String::new()
                };
            }
            "Layout" => {
                let next = Self::CLONE_LAYOUTS
                    .iter()
                    .position(|template| *template == self.coverage_prefs.layout)
                    .map(|index| (index + 1) % Self::CLONE_LAYOUTS.len())
                    .unwrap_or(0);
                self.coverage_prefs.layout = Self::CLONE_LAYOUTS[next].to_string();
            }
            "Prefix depth" => {
                self.coverage_prefs.prefix_depth =
                    if self.coverage_prefs.prefix_depth >= 3 { 1 } else { self.coverage_prefs.prefix_depth + 1 };
            }
            // Interaction
            "Hover effects" => self.hover_effects = !self.hover_effects,
            "Changed-row effect" => self.changed_row_effect = self.changed_row_effect.cycle(),
            // Layout
            "Panel padding" => self.panel_padding = !self.panel_padding,
            "Borders" => self.show_borders = !self.show_borders,
            "Pane splitter" => self.splitter_mode = self.splitter_mode.cycle(),
            "Repo page tabs" => {
                self.repo_page_tabs = self.repo_page_tabs.cycle();
                self.repo_page_tabbed_override = None; // changing the preference clears any `v` flip
            }
            "Auto branch-check" => self.branch_check = self.branch_check.cycle(),
            "Info layout" => self.info_layout = self.info_layout.cycle(),
            // Layout density preset (derived) — cycles Compact ⇄ Spacious; Custom isn't a cycle
            // target since it has no bundle of its own (see `layout_density`). Mirrors
            // `set_setting_option`'s (10, 0)/(10, 1) bundles directly (no double save_state).
            "Layout density" => {
                if self.layout_density() == 1 {
                    self.panel_padding = false;
                    self.show_borders = false;
                    self.splitter_mode = SplitterMode::Hover;
                } else {
                    self.panel_padding = true;
                    self.show_borders = true;
                    self.splitter_mode = SplitterMode::Dedicated;
                }
            }
            // Lists
            "Grouping" => {
                let prev = self.selected_repo_index();
                self.grouping_enabled = !self.grouping_enabled;
                self.reselect_repo(prev);
            }
            "Tree view" => {
                let prev = self.selected_repo_index();
                self.tree_enabled = !self.tree_enabled;
                self.reselect_repo(prev);
            }
            "Hide folder lines" => self.hide_folder_lines = !self.hide_folder_lines,
            // Pull requests
            "Merged PRs" => self.show_merged_prs = !self.show_merged_prs,
            // Sync
            "Auto-pull on launch" => self.auto_pull_on_launch = !self.auto_pull_on_launch,
            "Auto-pull limit" => self.auto_pull_max_repos = next_auto_pull_limit(self.auto_pull_max_repos),
            "Auto-pull in tree" => self.auto_pull_in_tree = !self.auto_pull_in_tree,
            // Theming
            "Icons" => {
                self.icon_style = match self.icon_style {
                    IconStyle::Unicode => IconStyle::Emoji,
                    IconStyle::Emoji => IconStyle::Unicode,
                };
            }
            // Inert in emoji mode (always hides zeros); only togglable with the Unicode set.
            "Hide zeros" if self.icon_style != IconStyle::Emoji => {
                self.hide_zero_counts = !self.hide_zero_counts;
            }
            "Theme" => self.theme = self.theme.cycle(),
            "Background" => self.background = self.background.cycle(),
            "Contrast" => self.contrast = self.contrast.cycle(),
            "List selection" => self.selection_style = self.selection_style.cycle(),
            "Button hover" => self.button_hover_style = self.button_hover_style.cycle(),
            // Tooltips
            "All tooltips" => self.tooltips.set_all(!self.tooltips.all_on()),
            "Footer commands" => self.tooltips.footer = !self.tooltips.footer,
            "Column headers" => self.tooltips.headers = !self.tooltips.headers,
            "Group counts" => self.tooltips.counts = !self.tooltips.counts,
            "Settings rows" => self.tooltips.settings = !self.tooltips.settings,
            "Help links" => self.tooltips.links = !self.tooltips.links,
            // Updates
            "Auto-update" => self.auto_update = self.auto_update.cycle(),
            "Update check" => self.update_interval = self.update_interval.cycle(),
            // Workers — mode row cycles Exact↔Percent; value row opens its dropdown.
            "Parallel pulls" => self.toggle_max_pull_mode(),
            "Parallel value" => self.open_parallel_value_dropdown(),
            _ => {}
        }
        self.save_state();
    }

    pub fn cycle_selected_setting(&mut self, forward: bool) {
        let row = self.settings_selected;
        let count = Self::settings_option_labels(row).len();
        if count == 0 {
            return;
        }
        let active = self.settings_active_option(row).min(count - 1);
        let next = if forward { (active + 1) % count } else { (active + count - 1) % count };
        self.set_setting_option(row, next);
    }

    pub fn open_settings(&mut self) {
        self.close_all_modals();
        self.show_settings = true;
        self.settings_selected = 0;
        // Accordion opens focused on the first section header; other layouts on the first row.
        self.settings_on_header =
            (self.settings_layout == crate::app::SettingsLayout::Accordion).then_some(0);
        self.settings_scroll = 0;
        self.settings_ensure_visible = true; // open scrolled to the selection
        self.settings_search.clear();
        self.settings_search_focused = false;
    }

    pub fn settings_row_matches(&self, idx: usize) -> bool {
        self.settings_search.is_empty()
            || SETTINGS_LABELS
                .get(idx)
                .is_some_and(|label| tui_pick::finder::fuzzy_matches(label, &self.settings_search))
    }

    pub fn settings_filtered_rows(&self) -> Vec<usize> {
        (0..Self::SETTINGS_ROWS).filter(|&idx| self.settings_row_matches(idx)).collect()
    }

    pub fn settings_begin_search(&mut self) {
        self.settings_search_focused = true;
    }

    pub fn settings_search_push(&mut self, ch: char) {
        self.settings_search.push(ch);
        self.settings_snap_selection();
    }

    pub fn settings_search_backspace(&mut self) {
        self.settings_search.pop();
        self.settings_snap_selection();
    }

    pub fn settings_clear_search(&mut self) {
        self.settings_search.clear();
        self.settings_search_focused = false;
    }

    fn settings_snap_selection(&mut self) {
        let matches = self.settings_filtered_rows();
        if !matches.is_empty() && !matches.contains(&self.settings_selected) {
            self.settings_selected = matches[0];
            self.settings_tab = Self::settings_tab_of_row(self.settings_selected);
        }
    }
}
