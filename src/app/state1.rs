use super::*;

impl AppState {
    /// Build the group runtimes from the loaded config + cache (static sources resolve inline;
    /// dynamic sources start from their cached membership, fresh or stale). Returns the
    /// validation errors of any dropped/invalid group definitions.
    pub fn init_groups(&mut self, config: GroupsConfig, cache: &GroupsCache) -> Vec<String> {
        self.collapse_threshold = config.collapse_threshold();
        self.group_cache_ttl_minutes = config.cache_ttl_minutes();
        let mut errors = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        self.groups = config
            .groups
            .into_iter()
            .filter_map(|def| {
                if !def.name.trim().is_empty() && !seen.insert(def.name.clone()) {
                    errors.push(format!("group '{}': duplicate name", def.name));
                    return None;
                }
                match def.source() {
                    Ok(source) => {
                        let cached = cache
                            .entries
                            .get(&def.name)
                            .filter(|entry| entry.fingerprint == source.fingerprint());
                        let members = match &source {
                            GroupSource::Pattern(_) => None,
                            GroupSource::Repos(list) => {
                                Some(list.iter().map(|name| name.to_lowercase()).collect())
                            }
                            _ => cached.map(|entry| {
                                entry.members.iter().map(|name| name.to_lowercase()).collect()
                            }),
                        };
                        Some(GroupRuntime {
                            name: def.name,
                            source,
                            members,
                            resolving: false,
                            error: None,
                            resolved_at: cached.map(|entry| entry.resolved_at),
                        })
                    }
                    Err(err) => {
                        errors.push(err);
                        None
                    }
                }
            })
            .collect();
        self.recompute_group_assignments();
        errors
    }

    /// Rebuild the repo → group map (first matching group wins, in config order). Called on
    /// startup and when dynamic membership arrives — never per frame.
    pub fn recompute_group_assignments(&mut self) {
        self.repo_group_map = self
            .repos
            .iter()
            .map(|repo| {
                let (name, rel) = {
                    let state = repo.lock().unwrap();
                    (state.name.to_lowercase(), state.rel_path.to_lowercase())
                };
                self.groups.iter().position(|group| group.contains(&name, &rel))
            })
            .collect();
    }

    /// Rebuild the directory-tree node model from the current repos' relative paths. Called
    /// when the repo set changes (each discovery batch); cheap and pure via `build_tree`.
    /// With multiple roots the tree is a **forest**: each repo's path is prefixed with a unique,
    /// readable label for its root so every root becomes its own top-level node.
    pub fn rebuild_tree(&mut self) {
        let labels = self.root_labels();
        let multi = self.root_dirs.len() > 1;
        let pairs: Vec<(usize, String)> = self
            .repos
            .iter()
            .enumerate()
            .map(|(idx, repo)| {
                let repo = repo.lock().unwrap();
                let path = if multi {
                    let label =
                        labels.get(&repo.root).cloned().unwrap_or_else(|| repo.root.display().to_string());
                    format!("{label}/{}", repo.rel_path)
                } else {
                    repo.rel_path.clone()
                };
                (idx, path)
            })
            .collect();
        self.tree_nodes = build_tree(&pairs);
    }

    /// A unique, readable tree label per root: its basename, or — when basenames collide — its
    /// home-relative path (`~/projects/personal`) so the forest's top-level nodes never merge.
    fn root_labels(&self) -> std::collections::HashMap<PathBuf, String> {
        let mut basename_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for root in &self.root_dirs {
            *basename_counts.entry(root_basename(root)).or_insert(0) += 1;
        }
        self.root_dirs
            .iter()
            .map(|root| {
                let base = root_basename(root);
                let label = if basename_counts.get(&base).copied().unwrap_or(0) > 1 {
                    home_relative(root)
                } else {
                    base
                };
                (root.clone(), label)
            })
            .collect()
    }

    /// Whether the list actually renders as a tree (the toggle is on AND there are folders).
    pub fn tree_active(&self) -> bool {
        self.tree_enabled && !self.tree_nodes.is_empty()
    }

    /// Toggle the directory-tree view, keeping the selection on the same repo. Toasts when the
    /// scan is flat (no nested folders to show).
    pub fn toggle_tree_view(&mut self) {
        if self.tree_nodes.is_empty() {
            self.show_toast("no nested folders — every repo is at the scan root");
            return;
        }
        let prev = self.selected_repo_index();
        self.tree_enabled = !self.tree_enabled;
        self.reselect_repo(prev);
        let toast = if self.tree_enabled { "tree view on" } else { "tree view off" };
        self.show_toast(toast);
    }

    /// All repo indices in a folder's subtree (the node's own repos plus all descendants').
    pub fn tree_subtree_repos(&self, node_idx: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![node_idx];
        while let Some(idx) = stack.pop() {
            let Some(node) = self.tree_nodes.get(idx) else {
                continue;
            };
            out.extend(node.repos.iter().copied());
            stack.extend(node.children.iter().copied());
        }
        out
    }

    /// The current effective concurrency cap (≤ `max_jobs`), reduced by throttle adaptation.
    pub fn effective_jobs(&self) -> usize {
        self.throttle.effective()
    }

    /// The distinct parent directories of all discovered repos. Worktrees live as
    /// `<repo>.worktrees/` siblings, so scanning each parent finds every repo's worktrees —
    /// for a single-level scan this is just the scan root.
    pub fn repo_parent_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for repo in &self.repos {
            if let Some(parent) = repo.lock().unwrap().path.parent() {
                let parent = parent.to_path_buf();
                if !dirs.contains(&parent) {
                    dirs.push(parent);
                }
            }
        }
        dirs
    }

    /// Whether the list actually renders grouped (the toggle is on AND groups are configured).
    pub fn grouping_active(&self) -> bool {
        self.grouping_enabled && !self.groups.is_empty()
    }

    /// The group's display name (`groups.len()` = the implicit ungrouped section).
    pub fn group_name(&self, group_idx: usize) -> &str {
        self.groups.get(group_idx).map_or("ungrouped", |group| group.name.as_str())
    }

    /// Any group has a dynamic (command/url) source — `Z` refresh is meaningful.
    pub fn any_dynamic_groups(&self) -> bool {
        self.groups.iter().any(|group| group.source.is_dynamic())
    }

    /// Toggle the per-repo diff view in the preview pane (shared by the `d` key and the
    /// status-bar hint).
    pub fn toggle_diff_view(&mut self) {
        if self.right_view == RightView::Diff {
            // Toggling off: drop the cached diff so it refreshes next time.
            if let Some(repo_idx) = self.selected_repo_index() {
                self.repos[repo_idx].lock().unwrap().diff = None;
            }
            self.right_view = RightView::Log;
        } else {
            // Entering Diff: start at the top, not the log's scroll position.
            if let Some(repo_idx) = self.selected_repo_index() {
                let mut state = self.repos[repo_idx].lock().unwrap();
                state.preview_scroll = 0;
                state.auto_scroll = false;
            }
            self.right_view = RightView::Diff;
        }
    }

    /// Toggle the grouped list view, keeping the selection on the same repo (shared by the
    /// `z` key and the status-bar hint). Toasts a pointer at the config when no groups exist.
    pub fn toggle_grouping_view(&mut self) {
        if self.groups.is_empty() {
            self.show_toast("no groups configured — see ~/.config/polygit/groups.json");
            return;
        }
        let prev = self.selected_repo_index();
        self.grouping_enabled = !self.grouping_enabled;
        self.reselect_repo(prev);
        let toast = if self.grouping_enabled { "grouping on" } else { "grouping off" };
        self.show_toast(toast);
    }

    /// How long a toast stays on screen before auto-dismissing.
    pub const TOAST_DURATION: Duration = Duration::from_millis(2500);

    /// Show a transient toast message (reusable anywhere — diff "no changes", view toggles…).
    pub fn show_toast(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast {
            message: message.into(),
            preview: Vec::new(),
            shown_at: Instant::now(),
        });
    }

    /// Max preview lines in a copy-confirmation toast.
    pub const COPY_PREVIEW_LINES: usize = 3;

    /// Confirm a clipboard copy: toast with the first few lines of what was copied.
    pub fn show_copy_toast(&mut self, copied: &str) {
        self.toast = Some(Toast {
            message: "copied to clipboard".into(),
            preview: copy_preview(copied),
            shown_at: Instant::now(),
        });
    }

    /// The toast if one is currently visible (un-expired), else None.
    pub fn active_toast(&self) -> Option<&Toast> {
        self.toast
            .as_ref()
            .filter(|toast| toast.shown_at.elapsed() < Self::TOAST_DURATION)
    }

    /// Number of rows in the `y` copy menu.
    pub const COPY_MENU_ROWS: usize = 3;

    /// The text to copy for the current `y`-menu selection over `row` (path / branch / both).
    pub fn copy_menu_text(&self, row: &PageRow) -> String {
        match self.copy_menu.unwrap_or(0) {
            1 => row.branch.clone(),
            2 => format!("{} {}", row.path.display(), row.branch),
            _ => row.path.display().to_string(),
        }
    }

    /// The active glyph set for the current icon-style setting.
    pub fn icons(&self) -> &'static IconSet {
        self.icon_style.icons()
    }

    /// No-op under test so unit tests can never clobber the user's real state.json.
    #[cfg(test)]
    pub fn save_state(&self) {}

    /// Persist the current UI preferences (columns, info state, splitter, settings).
    #[cfg(not(test))]
    pub fn save_state(&self) {
        let mut collapsed_groups: Vec<String> = self.collapsed_groups.iter().cloned().collect();
        collapsed_groups.sort();
        let mut collapsed_folders: Vec<String> = self.collapsed_folders.iter().cloned().collect();
        collapsed_folders.sort();
        crate::persist::save(&crate::persist::PersistedState {
            columns: self.columns,
            favorites: {
                let mut favorites: Vec<String> = self.favorites.iter().cloned().collect();
                favorites.sort();
                favorites
            },
            favorites_first: self.favorites_first,
            folder_bookmarks: self.folder_bookmarks.clone(),
            info_pinned: self.info_pinned,
            split_ratio: self.split_ratio,
            show_result_panel: self.show_result_panel,
            preview_split_ratio: self.preview_split_ratio,
            dock_ratio: self.dock_ratio,
            panel_padding: self.panel_padding,
            icon_style: self.icon_style,
            hide_zero_counts: self.hide_zero_counts,
            hide_folder_lines: self.hide_folder_lines,
            claude_agent: self.claude_agent,
            claude_skip_permissions: self.claude_skip_permissions,
            roots: Vec::new(), // legacy field — workspaces own the folder sets now
            workspaces: {
                // Persist every saved workspace; refresh the active one from the live root set so
                // picker add/remove sticks. Ad-hoc (no active workspace) sessions touch nothing.
                let mut workspaces = self.workspaces.clone();
                if let Some(name) = &self.active_workspace {
                    workspaces.insert(
                        name.clone(),
                        self.root_dirs.iter().map(|root| root.display().to_string()).collect(),
                    );
                }
                workspaces
            },
            theme: self.theme,
            contrast: self.contrast,
            selection_style: self.selection_style,
            button_hover_style: self.button_hover_style,
            settings_layout: self.settings_layout,
            collapsed_settings: {
                let mut sections: Vec<String> = self.collapsed_settings.iter().cloned().collect();
                sections.sort();
                sections
            },
            background: Some(self.background),
            sort_column: self.sort_column,
            sort_dir: self.sort_dir,
            help_tab: self.help_tab_persist,
            grouping_enabled: self.grouping_enabled,
            collapsed_groups,
            tree_enabled: self.tree_enabled,
            collapsed_folders,
            repo_page_tabs: self.repo_page_tabs,
            // Only the repo page's maximize is sticky; other panes' maximize is session-only.
            repo_page_maximized: self.maximized == Some(Pane::RepoPage),
            repo_page_maximized_tabbed: self.repo_page_maximized_tabbed,
            repo_page_collapsed_sections: {
                let mut sections: Vec<String> = self.repo_page_collapsed_sections.iter().cloned().collect();
                sections.sort();
                sections
            },
            branch_check: self.branch_check,
            repo_page_columns: self.repo_page_columns,
            repo_page_info: self.repo_page_info,
            base_overrides: self.base_overrides.clone(),
            auto_pull_on_launch: self.auto_pull_on_launch,
            auto_pull_max_repos: self.auto_pull_max_repos,
            auto_pull_in_tree: self.auto_pull_in_tree,
            hover_effects: self.hover_effects,
            show_borders: self.show_borders,
            splitter_mode: self.splitter_mode,
            changed_row_flash: self.changed_row_flash,
            changed_row_highlight: self.changed_row_highlight,
            tooltips: self.tooltips,
            design_layout: self.design_layout,
            last_seen_version: env!("CARGO_PKG_VERSION").to_string(),
            cli_help_mode: self.cli_builder.help_mode,
            diff_view: self.diff_view,
            show_merged_prs: self.show_merged_prs,
        });
    }

    /// Rebuild the status cache from the live repos and persist it. Repos pulled/refreshed this
    /// session (terminal + not `stale`) get a fresh entry stamped `now`; repos still showing
    /// cached data keep their existing entry (untouched age); transient (queued/running) repos
    /// are left as whatever was previously cached. `now` is Unix seconds (passed in — not read
    /// in pure code). No-op under test.
    #[cfg_attr(test, allow(unused_variables))]
    pub fn flush_cache(&mut self, now: i64) {
        for repo in &self.repos {
            let state = repo.lock().unwrap();
            let Some(status) = crate::cache::CacheStatus::from_status(&state.status) else {
                continue; // queued/running — keep any prior entry
            };
            if state.stale {
                continue; // not touched this session — preserve its cached age + data
            }
            self.status_cache.insert(
                state.path.clone(),
                crate::cache::CachedRepo {
                    status,
                    branch: state.branch.clone(),
                    details: state.details.clone(),
                    pull_result: state.pull_result.clone(),
                    updated_at: now,
                },
            );
        }
        #[cfg(not(test))]
        crate::cache::save(&self.status_cache);
    }

    /// Upsert resolved PRs (repos with a `pr_checked_at`) into the PR cache + persist. Mirrors
    /// `flush_cache`; called alongside it on settle/quit so the network results survive a relaunch.
    pub fn flush_pr_cache(&mut self) {
        for repo in &self.repos {
            let state = repo.lock().unwrap();
            let (Some(checked_at), Some(branch)) = (state.pr_checked_at, state.branch.as_deref())
            else {
                continue; // not resolved this session — preserve any prior cached entry
            };
            self.pr_cache.insert(
                crate::pr_cache::key(&state.path, branch),
                crate::pr_cache::PrCacheEntry { pr: state.pr.clone(), checked_at },
            );
        }
        #[cfg(not(test))]
        crate::pr_cache::save(&self.pr_cache);
    }

    /// Decide whether the repo at `idx` needs a `gh` PR lookup. Consults the in-memory PR cache: if
    /// a fresh entry exists (or the repo's in-memory `pr` is still within the TTL), seeds it and
    /// returns `None`; otherwise marks `pr_loading` and returns the repo handle for the caller to
    /// spawn `run_pull_request` on. `now` is Unix seconds.
    pub fn maybe_resolve_pr(&self, idx: usize, now: i64) -> Option<SharedRepoState> {
        let repo = &self.repos[idx];
        let mut state = repo.lock().unwrap();
        if state.pr_loading {
            return None;
        }
        if state.pr_checked_at.is_some_and(|at| crate::pr_cache::is_fresh(at, now)) {
            return None; // fresh in memory
        }
        // When the branch is known, a fresh cache entry seeds without a network call. (When it
        // isn't loaded yet, fall through to spawn — the worker resolves the branch via `gh`.)
        if let Some(branch) = state.branch.clone() {
            if let Some(entry) = self.pr_cache.get(&crate::pr_cache::key(&state.path, &branch)) {
                if crate::pr_cache::is_fresh(entry.checked_at, now) {
                    state.pr = entry.pr.clone();
                    state.pr_checked_at = Some(entry.checked_at);
                    return None; // seeded from cache — no network call
                }
            }
        }
        state.pr_loading = true;
        Some(std::sync::Arc::clone(repo))
    }

    /// The info-block action at `(col,row)`, if any (mouse hit-test).
    pub fn info_action_at(&self, col: u16, row: u16) -> Option<InfoAction> {
        self.info_click
            .iter()
            .find(|(click_row, start, end, _)| *click_row == row && col >= *start && col < *end)
            .map(|(_, _, _, action)| action.clone())
    }

    /// Expand or collapse a truncated info-block field by its label.
    pub fn toggle_info_expanded(&mut self, field: &str) {
        if !self.info_expanded.remove(field) {
            self.info_expanded.insert(field.to_string());
        }
    }

    /// The URL of a clickable help-modal link at the given screen row, if any.
    pub fn help_link_at(&self, row: u16) -> Option<String> {
        self.help_links
            .iter()
            .find(|(link_row, _)| *link_row == row)
            .map(|(_, url)| url.clone())
    }

    /// The help-modal tab whose chip is at `(col,row)`, if any (mouse click-to-switch).
    pub fn help_tab_at(&self, col: u16, row: u16) -> Option<HelpTab> {
        self.help_tab_click
            .iter()
            .find(|(chip_row, start, end, _)| *chip_row == row && col >= *start && col < *end)
            .map(|(_, _, _, tab)| *tab)
    }

    /// Whether `(col,row)` lands on the help-modal `[esc]` close button.
    pub fn help_close_at(&self, col: u16, row: u16) -> bool {
        region_hit(self.help_close_click, col, row)
    }

    /// The settings row (and option chip, if any) at `(col,row)` — None = the row label.
    pub fn settings_hit_at(&self, col: u16, row: u16) -> Option<(usize, Option<usize>)> {
        self.settings_click
            .iter()
            .find(|(region_row, start, end, _, _)| {
                *region_row == row && col >= *start && col < *end
            })
            .map(|(_, _, _, row_idx, option)| (*row_idx, *option))
    }

    /// The copy-menu option at a screen row, if any (mouse hit-test).
    pub fn copy_menu_option_at(&self, row: u16) -> Option<usize> {
        self.copy_menu_click
            .iter()
            .find(|(click_row, _)| *click_row == row)
            .map(|(_, index)| *index)
    }

    /// The selectable repo-page row whose `base` cell is at `(col,row)`, if any (mouse hit-test).
    pub fn base_cell_at(&self, col: u16, row: u16) -> Option<usize> {
        self.base_cell_click
            .iter()
            .find(|(click_row, start, end, _)| *click_row == row && col >= *start && col < *end)
            .map(|(_, _, _, index)| *index)
    }

    /// The base-picker option (0 = detected, then candidate index + 1) at a screen row, if any.
    pub fn base_picker_option_at(&self, row: u16) -> Option<usize> {
        self.base_picker_click
            .iter()
            .find(|(click_row, _)| *click_row == row)
            .map(|(_, index)| *index)
    }

    /// Open the base-branch picker for the branch on selectable row `index` of the open page.
    /// No-op unless that row is a branch with a resolved base. Candidates are gathered from the
    /// page (local branches, their upstreams, and every detected base) so the menu is synchronous.
    pub fn open_base_picker(&mut self, index: usize) {
        let Some(repo_index) = self.repo_page else {
            return;
        };
        let rows = self.repo_page_rows();
        let Some(row) = rows.get(index) else {
            return;
        };
        if row.kind != PageRowKind::Branch {
            return;
        }
        let branch = row.branch.clone();
        let (repo_path, mut candidates) = {
            let state = self.repos[repo_index].lock().unwrap();
            let path = state.path.clone();
            let mut refs: Vec<String> = Vec::new();
            if let Some(page) = state.page.as_ref() {
                for info in &page.branches {
                    if info.name != branch {
                        refs.push(info.name.clone());
                    }
                    if let Some(upstream) = &info.upstream {
                        refs.push(upstream.clone());
                    }
                    if let Some(base) = &info.base {
                        refs.push(base.clone());
                    }
                }
            }
            (path, refs)
        };
        candidates.sort();
        candidates.dedup();
        let current = self.base_overrides.get(&base_override_key(&repo_path, &branch)).cloned();
        // The displayed base is the detected one only when no override is in effect.
        let detected = if row.base_is_override { None } else { row.base.clone() };
        // Start the highlight on the current override (if any), else the detected entry (row 0).
        let selected = current
            .as_ref()
            .and_then(|over| candidates.iter().position(|cand| cand == over))
            .map_or(0, |pos| pos + 1);
        self.base_picker = Some(BasePicker {
            repo_index,
            branch,
            detected,
            current,
            candidates,
            selected,
        });
    }

    /// Apply the base-picker's highlighted option as the override and close the picker. Returns
    /// `(repo_index, branch)` so the caller can respawn the stats worker, or `None` if not open.
    pub fn confirm_base_picker(&mut self) -> Option<(usize, String)> {
        let picker = self.base_picker.take()?;
        let chosen = picker.ref_at(picker.selected);
        self.set_base_override(picker.repo_index, &picker.branch, chosen);
        Some((picker.repo_index, picker.branch))
    }

    /// Move the base-picker highlight by `delta`, clamped to its rows. `isize::MIN`/`MAX` jump to
    /// the first/last row (saturating, so they can't overflow).
    pub fn move_base_picker(&mut self, delta: isize) {
        if let Some(picker) = self.base_picker.as_mut() {
            let last = picker.row_count().saturating_sub(1);
            let next = (picker.selected as isize).saturating_add(delta).clamp(0, last as isize);
            picker.selected = next as usize;
        }
    }

    /// Set (or clear, with `None`) the base override for a repo+branch, persist it, and reset that
    /// branch's stats so the worker recomputes against the new base. Mirrors the override into the
    /// repo's own map (the stats worker reads it without the global `AppState`).
    pub fn set_base_override(&mut self, repo_index: usize, branch: &str, base_ref: Option<String>) {
        let mut state = self.repos[repo_index].lock().unwrap();
        let key = base_override_key(&state.path, branch);
        match &base_ref {
            Some(value) if !value.is_empty() => {
                self.base_overrides.insert(key, value.clone());
                state.base_overrides.insert(branch.to_string(), value.clone());
            }
            _ => {
                self.base_overrides.remove(&key);
                state.base_overrides.remove(branch);
            }
        }
        // Reset the branch's resolved base + stats so the worker re-resolves and re-diffs it.
        if let Some(page) = state.page.as_mut() {
            if let Some(info) = page.branches.iter_mut().find(|info| info.name == branch) {
                info.stats = None;
                info.merge_base_short = None;
                info.base = None;
                info.base_is_override = false;
            }
        }
        drop(state);
        self.save_state();
    }

    /// Seed a repo's per-branch override map from the persisted global map (call before opening the
    /// page so the stats worker sees the user's prior choices).
    pub fn seed_repo_base_overrides(&self, repo_index: usize) {
        let mut state = self.repos[repo_index].lock().unwrap();
        let path = state.path.clone();
        let prefix = format!("{}\u{1f}", path.display());
        state.base_overrides = self
            .base_overrides
            .iter()
            .filter_map(|(key, value)| {
                key.strip_prefix(&prefix).map(|branch| (branch.to_string(), value.clone()))
            })
            .collect();
    }

    /// Set a settings row to a specific option (mouse click on a radio chip) — unlike
    /// `toggle_selected_setting`, which cycles. Same row order; out-of-range pairs are a no-op.
    pub fn set_setting_option(&mut self, row_idx: usize, option_idx: usize) {
        // Rows are in alphabetical-section order (see SETTINGS_LABELS): Agent · Interaction · Layout
        // · Lists · Pull requests · Sync · Theming · Tooltips.
        match (row_idx, option_idx) {
            // Agent
            (0, 0) => self.claude_agent = ClaudeAgent::Claude,
            (0, 1) => self.claude_agent = ClaudeAgent::Codex,
            (0, 2) => self.claude_agent = ClaudeAgent::Gemini,
            (1, 0) => self.claude_skip_permissions = true,
            (1, 1) => self.claude_skip_permissions = false,
            // Interaction
            (2, 0) => self.hover_effects = true,
            (2, 1) => self.hover_effects = false,
            (3, 0) => self.changed_row_flash = true,
            (3, 1) => self.changed_row_flash = false,
            (4, 0) => self.changed_row_highlight = true,
            (4, 1) => self.changed_row_highlight = false,
            // Layout
            (5, 0) => self.panel_padding = true,
            (5, 1) => self.panel_padding = false,
            (6, 0) => self.show_borders = true,
            (6, 1) => self.show_borders = false,
            (7, 0) => self.splitter_mode = SplitterMode::Dedicated,
            (7, 1) => self.splitter_mode = SplitterMode::Hover,
            (8, 0) => self.repo_page_tabs = RepoTabsMode::Off,
            (8, 1) => self.repo_page_tabs = RepoTabsMode::Auto,
            (9, 0) => {
                if self.maximized == Some(Pane::RepoPage) {
                    self.maximized = None;
                }
            }
            (9, 1) => self.maximized = Some(Pane::RepoPage),
            (10, 0) => self.branch_check = BranchCheck::Off,
            (10, 1) => self.branch_check = BranchCheck::Auto,
            // Lists
            (11, 0) | (11, 1) => {
                let enable = option_idx == 0;
                if self.grouping_enabled != enable {
                    let prev = self.selected_repo_index();
                    self.grouping_enabled = enable;
                    self.reselect_repo(prev);
                }
            }
            (12, 0) | (12, 1) => {
                let enable = option_idx == 0;
                if self.tree_enabled != enable {
                    let prev = self.selected_repo_index();
                    self.tree_enabled = enable;
                    self.reselect_repo(prev);
                }
            }
            (13, 0) => self.hide_folder_lines = true,
            (13, 1) => self.hide_folder_lines = false,
            // Pull requests
            (14, 0) => self.show_merged_prs = true,
            (14, 1) => self.show_merged_prs = false,
            // Sync
            (15, 0) => self.auto_pull_on_launch = true,
            (15, 1) => self.auto_pull_on_launch = false,
            (16, 0) => self.auto_pull_max_repos = 50,
            (16, 1) => self.auto_pull_max_repos = 100,
            (16, 2) => self.auto_pull_max_repos = 250,
            (16, 3) => self.auto_pull_max_repos = 0,
            (17, 0) => self.auto_pull_in_tree = true,
            (17, 1) => self.auto_pull_in_tree = false,
            // Theming
            (18, 0) => self.icon_style = IconStyle::Unicode,
            (18, 1) => self.icon_style = IconStyle::Emoji,
            // Hide zeros is forced on (and inert) in emoji mode — ignore clicks then.
            (19, 0) if self.icon_style != IconStyle::Emoji => self.hide_zero_counts = true,
            (19, 1) if self.icon_style != IconStyle::Emoji => self.hide_zero_counts = false,
            (20, 0) => self.theme = Theme::Auto,
            (20, 1) => self.theme = Theme::Dark,
            (20, 2) => self.theme = Theme::Light,
            (21, 0) => self.background = Background::Normal,
            (21, 1) => self.background = Background::Soft,
            (21, 2) => self.background = Background::Terminal,
            (22, 0) => self.contrast = Contrast::Normal,
            (22, 1) => self.contrast = Contrast::Soft,
            (23, 0) => self.selection_style = SelectionStyle::Blue,
            (23, 1) => self.selection_style = SelectionStyle::Subtle,
            (24, 0) => self.button_hover_style = ButtonHoverStyle::Inverted,
            (24, 1) => self.button_hover_style = ButtonHoverStyle::Subtle,
            // Tooltips
            (25, 0) => self.tooltips.set_all(true),
            (25, 1) => self.tooltips.set_all(false),
            (26, 0) => self.tooltips.footer = true,
            (26, 1) => self.tooltips.footer = false,
            (27, 0) => self.tooltips.headers = true,
            (27, 1) => self.tooltips.headers = false,
            (28, 0) => self.tooltips.counts = true,
            (28, 1) => self.tooltips.counts = false,
            (29, 0) => self.tooltips.settings = true,
            (29, 1) => self.tooltips.settings = false,
            (30, 0) => self.tooltips.links = true,
            (30, 1) => self.tooltips.links = false,
            _ => return,
        }
        self.save_state();
    }

    /// Index of the currently-active option for settings row `row_idx` (mirrors the render row
    /// data + `set_setting_option`). Lets a click on the already-active chip cycle to the next
    /// value instead of being a no-op. Out-of-range rows return 0.
    pub fn settings_active_option(&self, row_idx: usize) -> usize {
        match row_idx {
            // Agent
            0 => match self.claude_agent {
                ClaudeAgent::Claude => 0,
                ClaudeAgent::Codex => 1,
                ClaudeAgent::Gemini => 2,
            },
            1 => usize::from(!self.claude_skip_permissions),
            // Interaction
            2 => usize::from(!self.hover_effects),
            3 => usize::from(!self.changed_row_flash),
            4 => usize::from(!self.changed_row_highlight),
            // Layout
            5 => usize::from(!self.panel_padding),
            6 => usize::from(!self.show_borders),
            7 => match self.splitter_mode {
                SplitterMode::Dedicated => 0,
                SplitterMode::Hover => 1,
            },
            8 => match self.repo_page_tabs {
                RepoTabsMode::Off => 0,
                RepoTabsMode::Auto => 1,
            },
            9 => usize::from(self.maximized == Some(Pane::RepoPage)),
            10 => match self.branch_check {
                BranchCheck::Off => 0,
                BranchCheck::Auto => 1,
            },
            // Lists
            11 => usize::from(!self.grouping_enabled),
            12 => usize::from(!self.tree_enabled),
            13 => usize::from(!self.hide_folder_lines),
            // Pull requests
            14 => usize::from(!self.show_merged_prs),
            // Sync
            15 => usize::from(!self.auto_pull_on_launch),
            16 => match self.auto_pull_max_repos {
                50 => 0,
                100 => 1,
                250 => 2,
                _ => 3,
            },
            17 => usize::from(!self.auto_pull_in_tree),
            // Theming
            18 => match self.icon_style {
                IconStyle::Unicode => 0,
                IconStyle::Emoji => 1,
            },
            // Emoji always hides zeros → force-selected "on" regardless of the stored flag.
            19 => usize::from(!(self.hide_zero_counts || self.icon_style == IconStyle::Emoji)),
            20 => match self.theme {
                Theme::Auto => 0,
                Theme::Dark => 1,
                Theme::Light => 2,
            },
            21 => match self.background {
                Background::Normal => 0,
                Background::Soft => 1,
                Background::Terminal => 2,
            },
            22 => match self.contrast {
                Contrast::Normal => 0,
                Contrast::Soft => 1,
            },
            23 => match self.selection_style {
                SelectionStyle::Blue => 0,
                SelectionStyle::Subtle => 1,
            },
            24 => match self.button_hover_style {
                ButtonHoverStyle::Inverted => 0,
                ButtonHoverStyle::Subtle => 1,
            },
            // Tooltips — All tooltips: 0 = all on, 1 = all off, 2 = mixed (neither radio active).
            25 => {
                if self.tooltips.all_on() {
                    0
                } else if self.tooltips.all_off() {
                    1
                } else {
                    2
                }
            }
            26 => usize::from(!self.tooltips.footer),
            27 => usize::from(!self.tooltips.headers),
            28 => usize::from(!self.tooltips.counts),
            29 => usize::from(!self.tooltips.settings),
            30 => usize::from(!self.tooltips.links),
            _ => 0,
        }
    }

    /// The option labels for settings row `row` (the single source the modal renders and the
    /// reset-plan formats). Boolean rows are `on`/`off`; the rest list their choices in order.
    pub fn settings_option_labels(row: usize) -> &'static [&'static str] {
        match row {
            0 => &["claude", "codex", "gemini"],
            8 => &["off", "auto"],
            9 => &["restored", "maximized"],
            10 => &["off", "auto"],
            16 => &["50", "100", "250", "\u{221e}"],
            18 => &["unicode", "emoji"],
            20 => &["auto", "dark", "light"],
            21 => &["normal", "soft", "terminal"],
            22 => &["normal", "soft"],
            23 => &["blue", "subtle"],
            24 => &["inverted", "subtle"],
            _ => &["on", "off"],
        }
    }

    /// The default option index for settings row `row` (mirrors the field defaults in
    /// `persist.rs` / the enum `#[default]`s). Used to detect what a reset would change.
    pub fn settings_default_option(row: usize) -> usize {
        match row {
            // Defaults whose active option is the first (on / auto / unicode / restored / claude / …).
            // 0 AI agent → claude; the Tooltips group (25–30) all default on. Layout: borders(6),
            // pane-splitter dedicated(7), repo-page-tabs off(8), repo-page restored(9), branch-check
            // off(10). Sync: auto-pull-on-launch(15). Interaction: changed-row flash(3). Theming:
            // icons unicode(18), theme(20), background(21), contrast(22), selection(23).
            0 | 3 | 6 | 7 | 8 | 9 | 10 | 15 | 18 | 20 | 21 | 22 | 23 | 25 | 26 | 27 | 28 | 29 | 30 => 0,
            // 24 button-hover defaults to `subtle` (index 1), 16 auto-pull-limit to `100` (index 1),
            // and every remaining boolean defaults off (index 1).
            _ => 1,
        }
    }

    /// The settings that differ from their defaults, formatted `Label: current → default`.
    /// Empty when everything is already at defaults.
    pub fn settings_reset_plan(&self) -> Vec<String> {
        (0..Self::SETTINGS_ROWS)
            .filter(|&row| row != 25) // "All tooltips" is derived from the per-area rows below it.
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

    /// Reset every settings-modal preference to its default (data — favorites, workspaces, caches,
    /// collapsed sets — is left untouched). Rebuilds the derived list state and persists.
    pub fn apply_settings_reset(&mut self) {
        self.grouping_enabled = false;
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
        self.hover_effects = false;
        self.changed_row_flash = true;
        self.changed_row_highlight = false;
        self.panel_padding = false;
        self.show_borders = true;
        self.splitter_mode = SplitterMode::Dedicated;
        self.repo_page_tabs = RepoTabsMode::Off;
        self.maximized = None;
        self.branch_check = BranchCheck::Off;
        self.tooltips = TooltipPrefs::default();
        self.claude_agent = ClaudeAgent::default();
        self.claude_skip_permissions = false;
        self.show_merged_prs = false;
        self.recompute_group_assignments();
        self.rebuild_tree();
        self.save_state();
    }

    /// Open the reset-to-defaults confirmation (listing the settings that will change), or toast
    /// when nothing differs from the defaults.
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

    pub const DEFAULT_SPLIT: f64 = 0.4;
    pub const MIN_SPLIT: f64 = 0.2;
    pub const MAX_SPLIT: f64 = 0.7;

    /// Nudge the split ratio by `delta`, clamped to the allowed range.
    pub fn adjust_split(&mut self, delta: f64) {
        self.split_ratio = (self.split_ratio + delta).clamp(Self::MIN_SPLIT, Self::MAX_SPLIT);
    }

    /// Set the split ratio from an absolute divider column (mouse drag).
    pub fn set_split_from_col(&mut self, col: u16) {
        if self.main_area.width == 0 {
            return;
        }
        let rel = f64::from(col.saturating_sub(self.main_area.x)) / f64::from(self.main_area.width);
        self.split_ratio = rel.clamp(Self::MIN_SPLIT, Self::MAX_SPLIT);
    }

    pub const DOCK_DEFAULT: f64 = 0.45;
    pub const DOCK_MIN: f64 = 0.2;
    pub const DOCK_MAX: f64 = 0.7;

    /// Set the docked-panel height ratio from an absolute screen row (mouse drag on the dock's
    /// top boundary): rows *below* the boundary become the dock.
    pub fn set_dock_from_row(&mut self, row: u16) {
        let area = self.dock_full_area;
        if area.height == 0 {
            return;
        }
        let below = (area.y + area.height).saturating_sub(row);
        let rel = f64::from(below) / f64::from(area.height);
        self.dock_ratio = rel.clamp(Self::DOCK_MIN, Self::DOCK_MAX);
    }

    pub const PREVIEW_SPLIT_DEFAULT: f64 = 0.4;
    pub const PREVIEW_SPLIT_MIN: f64 = 0.2;
    pub const PREVIEW_SPLIT_MAX: f64 = 0.8;

    /// Toggle the result/log panel (the bottom of the preview). Hidden, the info panel fills the
    /// pane (so it reads like the repo list). Persisted.
    pub fn toggle_result_panel(&mut self) {
        self.show_result_panel = !self.show_result_panel;
        self.show_toast(if self.show_result_panel {
            "result panel: shown"
        } else {
            "result panel: hidden"
        });
        self.save_state();
    }

    /// Set the info/result split ratio from an absolute screen row (drag on the boundary inside the
    /// preview): rows *above* the boundary become the info panel.
    pub fn set_preview_split_from_row(&mut self, row: u16) {
        let area = self.preview_split_area;
        if area.height == 0 {
            return;
        }
        let above = row.saturating_sub(area.y);
        let rel = f64::from(above) / f64::from(area.height);
        self.preview_split_ratio = rel.clamp(Self::PREVIEW_SPLIT_MIN, Self::PREVIEW_SPLIT_MAX);
    }

    /// Map mouse coordinates to a list selection index, or None for the separator row / header /
    /// outside the list. Result maps to `visible_len`. Uses the exact rows rect captured at
    /// render, so it's correct regardless of border, padding, and the column header.
    pub fn list_selection_at(&self, col: u16, row: u16) -> Option<usize> {
        let area = self.list_rows_area;
        if area.width == 0 || area.height == 0 {
            return None;
        }
        if col < area.x || col >= area.x + area.width || row < area.y || row >= area.y + area.height
        {
            return None;
        }
        let row_idx = (row - area.y) as usize + self.list_offset;
        let rows = self.visible_rows();
        // Physical rows: [rows…][sep][Result]([sep][Errors]). Group headers and spacers are
        // real list rows, so physical == logical for the rows region.
        if row_idx < rows.len() {
            match rows[row_idx] {
                // Static (small-group) headers, the favorites header, and spacers are inert.
                ListRow::GroupHeader { collapsible: false, .. }
                | ListRow::FavoritesHeader
                | ListRow::Spacer => None,
                _ => Some(row_idx),
            }
        } else if row_idx == rows.len() + 1 {
            Some(rows.len())
        } else if self.has_errors() && row_idx == rows.len() + 3 {
            Some(rows.len() + 1)
        } else {
            None
        }
    }

    /// The scrollbar whose track is at `(col,row)`, if it's actually scrollable (mouse grab).
    pub fn scrollbar_at(&self, col: u16, row: u16) -> Option<ScrollKind> {
        self.scroll_hits
            .iter()
            .find(|hit| {
                hit.total > hit.viewport
                    && hit.track.width > 0
                    && col == hit.track.x + hit.track.width - 1
                    && row >= hit.track.y
                    && row < hit.track.y + hit.track.height
            })
            .map(|hit| hit.kind)
    }

    /// The scroll offset mapping track row `row` to an absolute position for `kind` (clamped).
    pub fn scroll_value_for(&self, kind: ScrollKind, row: u16) -> Option<usize> {
        let hit = self.scroll_hits.iter().find(|hit| hit.kind == kind)?;
        let track_height = f64::from(hit.track.height.max(1));
        let rel = f64::from(row.saturating_sub(hit.track.y));
        let fraction = (rel / track_height).clamp(0.0, 1.0);
        let max_scroll = hit.total.saturating_sub(hit.viewport);
        Some(((fraction * hit.total as f64) as usize).min(max_scroll))
    }

    /// Apply a scroll offset to whatever `kind` controls. Returns true when the diff-modal file
    /// selection changed (so the caller can reload that file's diff).
    pub fn apply_scroll(&mut self, kind: ScrollKind, value: usize) -> bool {
        match kind {
            ScrollKind::List => {
                // Scroll the list view independently of the selection (render clamps to range).
                self.list_scroll = value;
                false
            }
            ScrollKind::Info => {
                if let Some(idx) = self.selected_repo_index() {
                    self.repos[idx].lock().unwrap().info_scroll = value;
                }
                false
            }
            ScrollKind::Preview => {
                if let Some(idx) = self.selected_repo_index() {
                    let mut state = self.repos[idx].lock().unwrap();
                    state.auto_scroll = false;
                    state.preview_scroll = value;
                }
                false
            }
            ScrollKind::DiffBody => {
                if let Some(modal) = self.diff_modal.as_mut() {
                    modal.scroll = value;
                }
                false
            }
            ScrollKind::DiffFiles => {
                // Scroll the file-list view independently of the selection (no diff reload).
                if let Some(modal) = self.diff_modal.as_mut() {
                    modal.file_scroll = value;
                }
                false
            }
            ScrollKind::Help => {
                self.help_scroll = value;
                false
            }
            ScrollKind::RepoPage => {
                self.repo_page_scroll = value;
                false
            }
            ScrollKind::Keyboard => {
                self.keyboard_scroll = value;
                false
            }
            ScrollKind::Settings => {
                self.settings_scroll = value;
                false
            }
            ScrollKind::Changelog => {
                self.changelog_scroll = value;
                false
            }
            ScrollKind::BuildInfo => {
                self.build_info_scroll = value;
                false
            }
            ScrollKind::PrModal => {
                if let Some(modal) = self.pr_modal.as_mut() {
                    modal.scroll = value;
                }
                false
            }
        }
    }

    /// Returns indices of repos visible given the current filter, in the active sort order.
    /// Whether a repo matches an `@token` filter — the token (lowercase, `@` stripped) tested
    /// against the repo's status keyword and a few attributes (dirty/clean/ahead/behind). An
    /// empty token (just `@`, still typing) matches everything.
    pub fn status_token_matches(state: &RepoState, token: &str) -> bool {
        if token.is_empty() {
            return true;
        }
        let status_key = match state.status {
            RepoStatus::Queued => "queued",
            RepoStatus::Running { .. } => "running",
            RepoStatus::UpToDate => "up-to-date",
            RepoStatus::Updated => "updated",
            RepoStatus::NoUpstream => "no-upstream",
            RepoStatus::Skipped => "skipped",
            RepoStatus::Throttled => "throttled",
            RepoStatus::Failed => "failed",
        };
        let mut keys: Vec<&str> = vec![status_key];
        if let Some(details) = &state.details {
            keys.push(if details.dirty_count > 0 { "dirty" } else { "clean" });
            if details.ahead.unwrap_or(0) > 0 {
                keys.push("ahead");
            }
            if details.behind.unwrap_or(0) > 0 {
                keys.push("behind");
            }
        }
        keys.iter().any(|key| key.contains(token))
    }
}
