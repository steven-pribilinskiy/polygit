use super::*;

impl AppState {
    /// Whether the dark palette is active (resolves `Theme::Auto` via the detected background).
    pub fn dark_active(&self) -> bool {
        match self.theme {
            Theme::Auto => self.auto_dark,
            Theme::Dark => true,
            Theme::Light => false,
        }
    }

    /// The active color palette composed from the current theme, background, and contrast.
    pub fn palette(&self) -> crate::theme::Palette {
        crate::theme::palette(self.dark_active(), self.background, self.contrast)
    }

    /// Total items in the list (visible rows + Result item + optional Errors item).
    pub fn list_len(&self) -> usize {
        self.visible_rows().len() + 1 + usize::from(self.has_errors())
    }

    /// Count of repos in each state. Tuple order:
    /// (queued, running, updated, up_to_date, skipped, failed, no_upstream, throttled).
    /// `throttled` is appended last so existing positional accesses (notably `.5` = failed)
    /// keep working.
    pub fn counts(&self) -> (usize, usize, usize, usize, usize, usize, usize, usize) {
        let mut queued = 0;
        let mut running = 0;
        let mut updated = 0;
        let mut up_to_date = 0;
        let mut skipped = 0;
        let mut failed = 0;
        let mut no_upstream = 0;
        let mut throttled = 0;
        for repo in &self.repos {
            let state = repo.lock().unwrap();
            if state.hidden {
                continue;
            }
            match &state.status {
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
        (queued, running, updated, up_to_date, skipped, failed, no_upstream, throttled)
    }

    pub fn done_count(&self) -> usize {
        let (_, _, updated, up_to_date, skipped, failed, no_upstream, throttled) = self.counts();
        updated + up_to_date + skipped + failed + no_upstream + throttled
    }

    /// Any repo ended in `Failed` — gates the dynamic "Errors" list row.
    pub fn has_errors(&self) -> bool {
        self.counts().5 > 0
    }

    /// Repos with an issue (failed or skipped) — the targets of "retry".
    pub fn retryable_repos(&self) -> Vec<usize> {
        self.repos
            .iter()
            .enumerate()
            .filter(|(_, repo)| repo.lock().unwrap().status.is_retryable())
            .map(|(index, _)| index)
            .collect()
    }

    /// Repos not currently in progress — the targets of "refetch" (re-pull regardless of status).
    /// Includes idle/cached repos (Queued) so a suppressed-auto-pull launch can pull them all.
    pub fn refetchable_repos(&self) -> Vec<usize> {
        self.repos
            .iter()
            .enumerate()
            .filter(|(_, repo)| !repo.lock().unwrap().status.is_running())
            .map(|(index, _)| index)
            .collect()
    }

    /// Whether the launch should auto-pull, given the discovered repo count and current view.
    /// Off by master toggle, over the (non-zero) repo limit, or in tree view unless allowed.
    /// Tree suppression keys off `tree_active()` (the toggle is on AND the scan actually has
    /// nested folders), not the raw flag — a flat scan like `…/some-repos` renders no tree,
    /// so a leftover `tree_enabled` from a previous nested scan must not suppress its auto-pull.
    pub fn should_auto_pull(&self, repo_count: usize) -> bool {
        self.auto_pull_on_launch
            && (self.auto_pull_max_repos == 0 || repo_count <= self.auto_pull_max_repos as usize)
            && (self.auto_pull_in_tree || !self.tree_active())
    }

    /// The adaptive auto-branch-check interval in seconds: ~`repo_count / 10`, clamped to 1..60
    /// (10 repos → ~1s, 100 → ~10s, 600+ → 60s).
    pub fn branch_check_interval_secs(repo_count: usize) -> u64 {
        ((repo_count as u64) / 10).clamp(1, 60)
    }

    /// Whether any repo is mid-pull (so the periodic branch-check holds off).
    pub fn any_pull_running(&self) -> bool {
        self.repos.iter().any(|repo| repo.lock().unwrap().status.is_running())
    }

    /// The hint key whose footer click-region contains `(col,row)`, if any.
    pub fn hint_at(&self, col: u16, row: u16) -> Option<HintKey> {
        self.hint_click
            .iter()
            .find(|hint| hint.row == row && col >= hint.col_start && col < hint.col_end)
            .map(|hint| hint.key)
    }

    /// The status-bar command whose click-region contains `(col,row)`, if any (drives the
    /// hover tooltip).
    pub fn command_at(&self, col: u16, row: u16) -> Option<Command> {
        self.clickable
            .iter()
            .find(|region| region.row == row && col >= region.col_start && col < region.col_end)
            .map(|region| region.command)
    }

    /// A footer command's dwell tooltip: line 1 is the static description; some commands add a 2nd
    /// line resolving the concrete target from the selected repo (remote URL, path, PR). The popup
    /// renders `\n`-separated lines. Falls back to the one-liner when nothing dynamic applies.
    pub fn command_tooltip(&self, command: Command) -> String {
        let base = command.tooltip().to_string();
        let info = self.selected_repo_index().map(|idx| {
            let repo = self.repos[idx].lock().unwrap();
            (repo.remote_url.clone(), repo.path.display().to_string(), repo.pr.clone())
        });
        let line2: Option<String> = match command {
            Command::OpenRemote | Command::CopyRemote => Some(
                info.as_ref()
                    .and_then(|(remote, ..)| remote.clone())
                    .unwrap_or_else(|| "(no remote)".to_string()),
            ),
            Command::CopyPath => info.as_ref().map(|(_, path, _)| path.clone()),
            Command::OpenPr => Some(match info.as_ref().and_then(|(.., pr)| pr.clone()) {
                Some(pr) => format!("PR #{} — {}", pr.number, pr.title),
                None => "(no open PR detected)".to_string(),
            }),
            Command::OpenPrWeb => Some(match info.as_ref().and_then(|(.., pr)| pr.clone()) {
                Some(pr) => pr.url,
                None => "(opens the compare page for this branch)".to_string(),
            }),
            _ => None,
        };
        match line2 {
            Some(line2) => format!("{base}\n{line2}"),
            None => base,
        }
    }

    /// The dwell tooltip for a captured region (column header / group-count tail) at a point: its
    /// text, the element to anchor the popup to, and the preferred side.
    pub fn tooltip_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<(String, Rect, tui_pick::Placement, Option<Column>, TooltipArea)> {
        self.hover_tooltips
            .iter()
            .find(|region| region.row == row && col >= region.col_start && col < region.col_end)
            .map(|region| {
                (region.text.clone(), region.anchor, region.placement, region.hide_column, region.area)
            })
    }

    fn selected_status_matches(&self, predicate: impl Fn(&RepoStatus) -> bool) -> bool {
        self.selected_repo_index()
            .is_some_and(|index| predicate(&self.repos[index].lock().unwrap().status))
    }

    /// The selected repo has an issue (failed or skipped) — `r` is meaningful.
    pub fn selected_repo_retryable(&self) -> bool {
        self.selected_status_matches(RepoStatus::is_retryable)
    }

    /// The selected repo is not in progress — `e` (refetch / pull) is meaningful.
    pub fn selected_repo_refetchable(&self) -> bool {
        self.selected_status_matches(|status| !status.is_running())
    }

    /// Repos covered by the selected folder/group header — a folder's whole subtree (recursively)
    /// or a group's visible members — so `e`/`r` can act on just that section. `None` when the
    /// selection isn't a folder/group header (a repo row, Result, etc.).
    pub fn selected_header_repos(&self) -> Option<Vec<usize>> {
        match self.selected_row()? {
            ListRow::FolderHeader { node_idx, .. } => Some(self.tree_subtree_repos(node_idx)),
            ListRow::GroupHeader { group_idx, .. } => Some(self.group_visible_members(group_idx)),
            _ => None,
        }
    }

    /// The selected folder/group header has at least one retryable repo (scoped `r`).
    pub fn selected_header_retryable(&self) -> bool {
        self.selected_header_repos().is_some_and(|repos| {
            repos.iter().any(|&idx| self.repos[idx].lock().unwrap().status.is_retryable())
        })
    }

    /// The selected folder/group header has at least one not-in-progress repo (scoped `e`).
    pub fn selected_header_refetchable(&self) -> bool {
        self.selected_header_repos().is_some_and(|repos| {
            repos.iter().any(|&idx| !self.repos[idx].lock().unwrap().status.is_running())
        })
    }

    /// Any repo has an issue — `R` is meaningful.
    pub fn any_retryable(&self) -> bool {
        self.repos
            .iter()
            .any(|repo| repo.lock().unwrap().status.is_retryable())
    }

    /// Any repo is not in progress — `E` (refetch / pull all) is meaningful.
    pub fn any_refetchable(&self) -> bool {
        self.repos
            .iter()
            .any(|repo| !repo.lock().unwrap().status.is_running())
    }

    /// Whether any overlay modal is open over the main two-pane view (settings / help / keyboard /
    /// build-info / diff / copy / base picker / confirm). The dedicated repo page is excluded — it
    /// has its own footer, not the main status bar.
    pub fn any_modal_open(&self) -> bool {
        self.show_settings
            || self.show_help
            || self.show_keyboard
            || self.show_build_info
            || self.confirm.is_some()
            || self.diff_modal.is_some()
            || self.pr_modal.is_some()
            || self.copy_menu.is_some()
            || self.kebab.is_some()
            || self.base_picker.is_some()
            || self.branch_picker.is_some()
            || self.show_changelog
            || self.explorer.is_some()
    }

    /// Close every overlay modal so a freshly-opened one is the only one on screen (single-modal
    /// invariant — opening Settings while Help is up closes Help instead of stacking). The
    /// repo page is a view, not a modal, so it's left untouched; the keyboard helper is
    /// a child of Help and opens via its own path, which doesn't call this.
    pub fn close_all_modals(&mut self) {
        self.show_help = false;
        self.show_settings = false;
        self.show_keyboard = false;
        self.show_build_info = false;
        self.confirm = None;
        self.diff_modal = None;
        self.pr_modal = None;
        self.copy_menu = None;
        self.kebab = None;
        self.base_picker = None;
        self.branch_picker = None;
        self.dropdown = None;
        self.finder = None;
        self.picker = None;
        self.show_changelog = false;
        self.changelog_pin_mode = false;
        self.pin_show_all = false;
    }

    /// Open the changelog modal. `whats_new` filters to releases newer than the last-seen version
    /// (all expanded); otherwise the full changelog opens with all but the latest two collapsed.
    pub fn open_changelog(&mut self, whats_new: bool) {
        self.close_all_modals();
        self.show_changelog = true;
        self.changelog_whats_new = whats_new;
        self.changelog_scroll = 0;
        self.changelog_selected = 0;
        self.changelog_ensure_visible = true;
        if !whats_new {
            self.changelog_collapsed = crate::changelog::releases()
                .iter()
                .skip(2)
                .map(|release| release.version.to_string())
                .collect();
        }
    }

    /// Open the version picker — the changelog modal in "pin" sub-mode. Lists live releases
    /// (fetched by the caller) and installs the chosen one over the running binary. Defaults to
    /// showing only floor-and-up versions (those with the in-app picker); `a` reveals older ones.
    pub fn open_pin_picker(&mut self) {
        self.close_all_modals();
        self.show_changelog = true;
        self.changelog_pin_mode = true;
        self.pin_show_all = false;
        self.pin_selected = 0;
        self.changelog_scroll = 0;
        self.changelog_ensure_visible = true;
        self.pin_error = None;
        self.pin_status = None;
        self.pin_releases_loading = true;
    }

    /// The picker rows currently visible (all releases unless `pin_show_all` is off, which hides
    /// pre-floor versions). Returns indices into `pin_releases`.
    pub fn pin_visible_indices(&self) -> Vec<usize> {
        self.pin_releases
            .iter()
            .enumerate()
            .filter(|(_, release)| self.pin_show_all || release.is_supported)
            .map(|(idx, _)| idx)
            .collect()
    }

    /// Build the confirm dialog for the currently-selected picker row, or `None` when the selection
    /// is the running version (inert) or the list is empty. Below-floor versions (no in-app picker)
    /// get a danger warning plus the copyable return-to-latest command — the only way back.
    pub fn pin_confirm_for_selected(&self) -> Option<ConfirmDialog> {
        let visible = self.pin_visible_indices();
        let release = visible.get(self.pin_selected).and_then(|&idx| self.pin_releases.get(idx))?;
        if release.is_current {
            return None;
        }
        let version = release.version.clone();
        if release.is_supported {
            return Some(ConfirmDialog::simple(
                format!("Pin v{version} — download & replace the running binary, then reload?"),
                ConfirmAction::PinVersion { version },
                false,
            ));
        }
        let live = self.exe_path.strip_suffix(" (deleted)").unwrap_or(&self.exe_path);
        let exe_dir = std::path::Path::new(live)
            .parent()
            .map(|dir| dir.display().to_string())
            .unwrap_or_else(|| ".".to_string());
        Some(ConfirmDialog {
            message: format!(
                "v{version} is a pre-v3 build: it uses its own legacy state.json, while your v3 \
                 settings live in state-v3.json — both are kept, so nothing is lost (the two builds \
                 just don't share settings). It also can't switch versions from inside the old build. \
                 To come back, run `polygit update` for the latest, or pin a newer version from this \
                 picker again."
            ),
            action: ConfirmAction::PinVersion { version },
            danger: true,
            restore_files: Vec::new(),
            delete_files: Vec::new(),
            detail_lines: Vec::new(),
            detail_title: Some("To return to the latest build later, run:".to_string()),
            copy_line: Some(crate::update::return_to_latest_cmd(&exe_dir)),
        })
    }

    /// Toggle a release's collapsed state in the full changelog accordion.
    pub fn toggle_changelog_release(&mut self, version: &str) {
        if !self.changelog_collapsed.remove(version) {
            self.changelog_collapsed.insert(version.to_string());
        }
    }

    /// Switch the active help tab, remembering it for persistence only when it isn't About (so
    /// reopening help lands on the last *useful* tab, never the credits/links tab).
    pub fn set_help_tab(&mut self, tab: HelpTab) {
        self.help_tab = tab;
        if tab != HelpTab::About {
            self.help_tab_persist = tab;
        }
    }

    /// Open the help modal as the only modal (resets scroll).
    pub fn open_help(&mut self) {
        self.close_all_modals();
        self.show_help = true;
        self.help_scroll = 0;
    }

    /// Open the settings modal as the only modal.
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

    /// Whether settings row `idx`'s label fuzzy-matches the current search query (always true when
    /// the query is empty).
    pub fn settings_row_matches(&self, idx: usize) -> bool {
        self.settings_search.is_empty()
            || SETTINGS_LABELS
                .get(idx)
                .is_some_and(|label| tui_pick::finder::fuzzy_matches(label, &self.settings_search))
    }

    /// The global row indices matching the current search query, in order.
    pub fn settings_filtered_rows(&self) -> Vec<usize> {
        (0..Self::SETTINGS_ROWS).filter(|&idx| self.settings_row_matches(idx)).collect()
    }

    /// Begin / refocus the settings search input.
    pub fn settings_begin_search(&mut self) {
        self.settings_search_focused = true;
    }

    /// Push a char into the settings search query and keep the selection on a matching row.
    pub fn settings_search_push(&mut self, ch: char) {
        self.settings_search.push(ch);
        self.settings_snap_selection();
    }

    /// Backspace the settings search query.
    pub fn settings_search_backspace(&mut self) {
        self.settings_search.pop();
        self.settings_snap_selection();
    }

    /// Clear the settings search (query + focus).
    pub fn settings_clear_search(&mut self) {
        self.settings_search.clear();
        self.settings_search_focused = false;
    }

    /// Keep `settings_selected` on a row that matches the query (the first match if it fell out).
    fn settings_snap_selection(&mut self) {
        let matches = self.settings_filtered_rows();
        if !matches.is_empty() && !matches.contains(&self.settings_selected) {
            self.settings_selected = matches[0];
            self.settings_tab = Self::settings_tab_of_row(self.settings_selected);
        }
    }

    /// Open the build-info modal as the only modal.
    pub fn open_build_info(&mut self) {
        self.close_all_modals();
        self.show_build_info = true;
        self.build_info_scroll = 0;
        // Snapshot the binary size + the config-dir contents for the modal (cheap, on open only).
        self.build_info_binary_size = std::fs::metadata(&self.exe_path).map(|meta| meta.len()).unwrap_or(0);
        let settings = crate::persist::state_path();
        self.build_info_settings_path =
            settings.as_ref().map(|path| path.display().to_string()).unwrap_or_default();
        self.build_info_config_count = crate::persist::config_dir()
            .and_then(|dir| std::fs::read_dir(dir).ok())
            .map(|entries| entries.filter(|entry| entry.is_ok()).count())
            .unwrap_or(0);
        let raw = settings.and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
        // Parse into a collapsible tree (collapsed by default); keep the raw lines as a fallback for
        // when the file isn't valid JSON.
        self.build_info_tree = crate::treeview::DataNode::parse_json(&raw);
        self.build_info_tree_expanded.clear();
        self.build_info_tree_selected = 0;
        self.build_info_settings_preview = raw.lines().map(str::to_string).collect();
    }

    /// Flatten the settings tree to its currently-visible rows (empty when there's no tree).
    pub fn build_info_tree_rows(&self) -> Vec<crate::treeview::TreeRow> {
        self.build_info_tree
            .as_ref()
            .map(|tree| crate::treeview::flatten(tree, &self.build_info_tree_expanded))
            .unwrap_or_default()
    }

    /// Bring the selected settings-tree row into view (keyboard / Alt+wheel nav), web-app style —
    /// a selection already on screen leaves the scroll untouched. Mirrors
    /// `ensure_list_selection_visible`.
    pub fn ensure_build_info_visible(&mut self, viewport: usize) {
        if viewport == 0 {
            return;
        }
        let total = self.build_info_tree_rows().len();
        let max_scroll = total.saturating_sub(viewport);
        let selected = self.build_info_tree_selected;
        if selected < self.build_info_scroll {
            self.build_info_scroll = selected;
        } else if selected >= self.build_info_scroll + viewport {
            self.build_info_scroll = selected + 1 - viewport;
        }
        self.build_info_scroll = self.build_info_scroll.min(max_scroll);
    }

    /// Toggle (fold/unfold) the container at the selected settings-tree row, if it is one.
    pub fn build_info_toggle_selected(&mut self) {
        let rows = self.build_info_tree_rows();
        if let Some(row) = rows.get(self.build_info_tree_selected) {
            if matches!(row.kind, crate::treeview::RowKind::Container { .. })
                && !self.build_info_tree_expanded.remove(&row.path)
            {
                self.build_info_tree_expanded.insert(row.path.clone());
            }
        }
    }

    /// Expand (`expand = true`) or collapse every container in the settings tree.
    pub fn build_info_fold_all(&mut self, expand: bool) {
        if expand {
            if let Some(tree) = &self.build_info_tree {
                self.build_info_tree_expanded =
                    crate::treeview::all_container_paths(tree).into_iter().collect();
            }
        } else {
            self.build_info_tree_expanded.clear();
        }
    }

    /// Move the settings-tree selection by `delta`, clamped to the visible rows.
    pub fn build_info_tree_move(&mut self, delta: isize) {
        let len = self.build_info_tree_rows().len();
        if len == 0 {
            return;
        }
        let next = (self.build_info_tree_selected as isize).saturating_add(delta).clamp(0, len as isize - 1);
        self.build_info_tree_selected = next as usize;
    }

    /// Expand the selected container (→). If it's already expanded or a scalar, no-op.
    pub fn build_info_tree_expand(&mut self) {
        let rows = self.build_info_tree_rows();
        if let Some(row) = rows.get(self.build_info_tree_selected) {
            if let crate::treeview::RowKind::Container { collapsed: true, .. } = row.kind {
                self.build_info_tree_expanded.insert(row.path.clone());
            }
        }
    }

    /// Collapse the selected container (←); if it's a leaf or already collapsed, jump to its parent.
    pub fn build_info_tree_collapse_or_parent(&mut self) {
        let rows = self.build_info_tree_rows();
        let Some(row) = rows.get(self.build_info_tree_selected) else {
            return;
        };
        let is_open_container =
            matches!(row.kind, crate::treeview::RowKind::Container { collapsed: false, .. });
        if is_open_container {
            self.build_info_tree_expanded.remove(&row.path);
            return;
        }
        // Jump to the nearest previous row at a shallower depth (the parent).
        let depth = row.depth;
        if depth > 0 {
            for index in (0..self.build_info_tree_selected).rev() {
                if rows[index].depth < depth {
                    self.build_info_tree_selected = index;
                    break;
                }
            }
        }
    }

    /// Whether a footer `Command` is actionable in the current context (ignoring modal/leader
    /// state, which the footer handles separately). Drives the disabled-look of footer hints.
    pub fn command_applicable(&self, command: Command) -> bool {
        match command {
            // Need a real repo row selected (not Result/Errors or a header).
            Command::Info
            | Command::DiffView
            | Command::OpenPage
            | Command::Claude
            | Command::Lazygit
            | Command::Explore
            | Command::OpenRemote
            | Command::CopyPath
            | Command::CopyRemote => self.selected_repo_index().is_some(),
            // Folding only applies in tree or grouped view.
            Command::NavLeft
            | Command::NavRight
            | Command::FoldCollapseAll
            | Command::FoldExpandAll
            | Command::FoldExpandSubtree => self.tree_active() || self.grouping_active(),
            // View toggles need their data to exist.
            Command::GroupingToggle => !self.groups.is_empty(),
            Command::TreeToggle => !self.tree_nodes.is_empty(),
            Command::FavoritesFirst => self.has_favorites(),
            // Selection moves need a non-empty list.
            Command::NavDown | Command::NavUp => !self.repos.is_empty(),
            // Retry/refetch reuse their existing no-op predicates.
            Command::Retry => self.selected_repo_retryable() || self.selected_header_retryable(),
            Command::RetryAll => self.any_retryable(),
            Command::Refetch => self.selected_repo_refetchable() || self.selected_header_refetchable(),
            Command::RefetchAll => self.any_refetchable(),
            // Everything else is always available (filters, sort, columns, resize, dock, focus,
            // result overlay, settings/help/quit, build info, menu items).
            _ => true,
        }
    }

    /// Navigate selection up, returns true if changed. Skips static group headers. The
    /// right-pane view is intentionally preserved so an open info view (`i`) follows the
    /// selection across repos.
    pub fn nav_up(&mut self) -> bool {
        self.user_navigated = true;
        self.result_overlay = false;
        let rows = self.visible_rows();
        let total = rows.len() + 1 + usize::from(self.has_errors());
        let mut idx = self.selected.min(total.saturating_sub(1));
        while idx > 0 {
            idx -= 1;
            if Self::row_selectable_in(&rows, total, idx) {
                self.selected = idx;
                return true;
            }
        }
        false
    }

    /// Navigate selection down, returns true if changed. Skips static group headers.
    pub fn nav_down(&mut self) -> bool {
        self.user_navigated = true;
        self.result_overlay = false;
        let rows = self.visible_rows();
        let total = rows.len() + 1 + usize::from(self.has_errors());
        let mut idx = self.selected;
        while idx + 1 < total {
            idx += 1;
            if Self::row_selectable_in(&rows, total, idx) {
                self.selected = idx;
                return true;
            }
        }
        false
    }

    pub fn nav_top(&mut self) {
        self.user_navigated = true;
        self.result_overlay = false;
        self.selected = 0;
        self.snap_selection(true);
    }

    pub fn nav_bottom(&mut self) {
        self.user_navigated = true;
        self.result_overlay = false;
        self.selected = self.list_len().saturating_sub(1);
    }

    /// Physical list-item row of the current selection, accounting for the separator rows before
    /// the Result and Errors summary items (mirrors the mapping in `render_list`).
    pub fn selected_item_row(&self) -> usize {
        let rows = self.visible_rows().len();
        if self.selected < rows {
            self.selected
        } else if self.selected == rows {
            rows + 1 // one separator before Result
        } else {
            rows + 3 // separator + Result + separator before Errors
        }
    }

    /// Total physical item rows in the list widget (repo/header rows + separators + Result +
    /// optional Errors + optional empty-state hint) — the basis for clamping the manual scroll.
    pub fn total_item_rows(&self) -> usize {
        let rows = self.visible_rows().len();
        rows + 2 // separator + Result
            + if self.has_errors() { 2 } else { 0 } // separator + Errors
            + if self.discovery_done && self.repos.is_empty() { 2 } else { 0 } // blank + hint
    }

    /// The largest valid `list_scroll` for a given viewport height.
    pub fn max_list_scroll(&self, viewport: usize) -> usize {
        self.total_item_rows().saturating_sub(viewport)
    }

    /// Scroll the list by `delta` rows (plain mouse wheel), clamped to the content. Does NOT move
    /// the selection — the selected row may scroll out of view, web-app style.
    pub fn scroll_list(&mut self, delta: isize, viewport: usize) {
        let max = self.max_list_scroll(viewport) as isize;
        self.list_scroll = (self.list_scroll as isize + delta).clamp(0, max.max(0)) as usize;
    }

    /// Scroll the list only as far as needed to bring the selection into view (keyboard / Alt+wheel
    /// nav). A selection already on screen leaves the scroll untouched — so e.g. moving up off the
    /// bottom row doesn't shift the viewport.
    pub fn ensure_list_selection_visible(&mut self, viewport: usize) {
        if viewport == 0 {
            return;
        }
        let item = self.selected_item_row();
        if item < self.list_scroll {
            self.list_scroll = item;
        } else if item >= self.list_scroll + viewport {
            self.list_scroll = item + 1 - viewport;
        }
        self.list_scroll = self.list_scroll.min(self.max_list_scroll(viewport));
    }

    /// Move the selection up by `step` rows (PageUp), landing on a selectable row.
    pub fn nav_page_up(&mut self, step: usize) {
        self.user_navigated = true;
        self.result_overlay = false;
        self.selected = self.selected.saturating_sub(step.max(1));
        self.snap_selection(false);
    }

    /// Move the selection down by `step` rows (PageDown), clamped to the last row.
    pub fn nav_page_down(&mut self, step: usize) {
        self.user_navigated = true;
        self.result_overlay = false;
        let max = self.list_len().saturating_sub(1);
        self.selected = (self.selected + step.max(1)).min(max);
        self.snap_selection(true);
    }

    /// Returns the repo index for the current selection, or None when a group header or the
    /// Result/Errors row is selected.
    pub fn selected_repo_index(&self) -> Option<usize> {
        match self.visible_rows().get(self.selected) {
            Some(ListRow::Repo { repo_idx, .. }) => Some(*repo_idx),
            _ => None,
        }
    }

    /// Open the dedicated repo page for the selected repo (forces a fresh fetch). The selection
    /// snaps to the current (HEAD) branch once the page loads.
    pub fn open_repo_page(&mut self) {
        if let Some(idx) = self.selected_repo_index() {
            self.repo_page = Some(idx);
            self.repo_page_selected = 0;
            self.repo_page_scroll = 0;
            self.repo_page_message = None;
            self.repo_page_focus_head = true;
            self.repo_page_tab = RepoTab::Branches;
            self.focus = Pane::RepoPage;
            self.repos[idx].lock().unwrap().page = None;
        }
    }

    /// Point the already-open restored panel at a different repo as the list selection moves
    /// (master-detail). Unlike `open_repo_page` it reuses the cached page (no fresh fetch) and
    /// doesn't move focus off the list.
    pub fn retarget_repo_page(&mut self, idx: usize) {
        if self.repo_page == Some(idx) {
            return;
        }
        self.repo_page = Some(idx);
        self.repo_page_selected = 0;
        self.repo_page_scroll = 0;
        self.repo_page_message = None;
        self.repo_page_focus_head = true;
        self.repo_page_tab = RepoTab::Branches;
    }

    /// Whether `pane` can be shown/maximized right now (maximizing an unavailable pane is a no-op):
    /// Info needs a selected repo, the repo page needs to be open; List/Result are always available.
    pub fn is_pane_available(&self, pane: Pane) -> bool {
        match pane {
            Pane::List | Pane::Result => true,
            Pane::Info => self.selected_repo_index().is_some(),
            Pane::RepoPage => self.repo_page.is_some(),
        }
    }

    /// The pane that effectively owns the screen + keyboard right now: the maximized pane when one is
    /// (still) available, otherwise the focused pane. Key routing and the maximized render path both
    /// key off this so there's one definition of "which pane is active".
    pub fn active_pane(&self) -> Pane {
        match self.maximized {
            Some(pane) if self.is_pane_available(pane) => pane,
            _ => self.focus,
        }
    }

    /// Maximize ⇄ restore `pane` (no-op if it isn't available). The pane becomes focused. Only the
    /// repo page's maximize persists (via `save_state` → the legacy `repo_page_maximized` bool);
    /// maximizing List/Info/Result is session-only but we still save so the repo-page bit stays correct.
    pub fn toggle_maximized(&mut self, pane: Pane) {
        if !self.is_pane_available(pane) {
            return;
        }
        self.maximized = if self.maximized == Some(pane) { None } else { Some(pane) };
        self.focus = pane;
        self.save_state();
    }

    /// Direct a `1`/`2`/`3`/`4` (or click) at `pane`: when a pane is maximized, swap which pane is
    /// maximized (you're always looking at exactly one pane); otherwise just move focus. No-op if the
    /// target pane isn't available.
    pub fn focus_or_maximize_pane(&mut self, pane: Pane) {
        if !self.is_pane_available(pane) {
            return;
        }
        if self.maximized.is_some() {
            self.maximized = Some(pane);
            self.focus = pane;
            self.save_state();
        } else {
            self.focus_pane(pane);
        }
    }

    /// The focusable panels in cycle order, only the currently-visible ones. A maximized pane is
    /// full-screen, so it's the sole entry; otherwise List is always present and Info / Result /
    /// RepoPage appear when shown. The number labels stay stable regardless (see `Pane::number`).
    pub fn visible_panes(&self) -> Vec<Pane> {
        if let Some(pane) = self.maximized {
            if self.is_pane_available(pane) {
                return vec![pane];
            }
        }
        let mut panes = vec![Pane::List];
        if self.info_pinned && !self.result_overlay && self.selected_repo_index().is_some() {
            panes.push(Pane::Info);
        }
        if self.show_result_panel {
            panes.push(Pane::Result);
        }
        if self.repo_page.is_some() {
            panes.push(Pane::RepoPage);
        }
        panes
    }

    /// Move focus to the next / previous visible panel, wrapping. A focus that isn't currently
    /// visible snaps to the first panel.
    pub fn cycle_focus(&mut self, forward: bool) {
        let panes = self.visible_panes();
        if panes.is_empty() {
            return;
        }
        let next = match panes.iter().position(|&pane| pane == self.focus) {
            Some(idx) => {
                let len = panes.len();
                if forward { (idx + 1) % len } else { (idx + len - 1) % len }
            }
            None => 0,
        };
        self.focus = panes[next];
    }

    /// Focus a specific panel by identity, ignored if that panel isn't currently visible.
    pub fn focus_pane(&mut self, pane: Pane) {
        if self.visible_panes().contains(&pane) {
            self.focus = pane;
        }
    }

    /// Whether a point lands on any pane's top-border button (the pane maximize/restore `m▢`, the
    /// result/info copy `📋`, or the repo page's `t cols`/`s sort`/maximize/`esc`). Those borders
    /// double as splitter resize handles, so these columns must be excluded from the splitter grab
    /// or the buttons could never be clicked (the drag would steal the press).
    pub fn title_button_hit(&self, col: u16, row: u16) -> bool {
        let hit = |region: Option<(u16, u16, u16)>| {
            region.is_some_and(|(button_row, start, end)| row == button_row && col >= start && col < end)
        };
        hit(self.repo_page_back_click)
            || hit(self.repo_page_window_click)
            || hit(self.page_cols_click)
            || hit(self.page_sort_click)
            || self.max_click.iter().any(|&(r, s, e, _)| row == r && col >= s && col < e)
            || self.info_click.iter().any(|&(r, s, e, _)| row == r && col >= s && col < e)
    }

    /// Once the repo page's rows exist, move the selection to the current (HEAD) branch — done
    /// once per open, and never overriding a manual move.
    pub fn focus_head_branch_if_pending(&mut self) {
        if !self.repo_page_focus_head {
            return;
        }
        let head = self.repo_page_rows().iter().position(|row| row.is_head);
        if let Some(index) = head {
            self.repo_page_selected = index;
            self.repo_page_focus_head = false;
        }
    }

    pub fn close_repo_page(&mut self) {
        self.repo_page = None;
        self.repo_page_message = None;
        if self.focus == Pane::RepoPage {
            self.focus = Pane::List;
        }
    }

    /// The repo page's selectable rows (branches then worktrees), in display order.
    pub fn repo_page_rows(&self) -> Vec<PageRow> {
        let mut rows = Vec::new();
        let Some(idx) = self.repo_page else {
            return rows;
        };
        let state = self.repos[idx].lock().unwrap();
        let Some(page) = &state.page else {
            return rows;
        };
        let repo_path = state.path.clone();
        for branch in &page.branches {
            rows.push(PageRow {
                kind: PageRowKind::Branch,
                branch: branch.name.clone(),
                path: repo_path.clone(),
                deletable: branch.deletable(),
                is_head: branch.is_head,
                dirty: branch.is_head && page.head_dirty_count > 0,
                dirty_count: if branch.is_head { page.head_dirty_count } else { 0 },
                stash_index: None,
                ahead: branch.ahead,
                behind: branch.behind,
                upstream: branch.upstream.clone(),
                last_commit_rel: branch.last_commit_rel.clone(),
                last_commit_secs: branch.last_commit_secs,
                subject: branch.subject.clone(),
                stats: branch.stats,
                commit_sha: branch.commit_sha.clone(),
                author: branch.author.clone(),
                merge_base_short: branch.merge_base_short.clone(),
                base: branch.base.clone(),
                base_is_override: branch.base_is_override,
                parents: Vec::new(),
                upstream_gone: branch.upstream_gone,
            });
        }
        for worktree in &page.worktrees {
            let branch_info = page.branches.iter().find(|info| info.name == worktree.branch);
            let dirty_count = page
                .dirty_worktrees
                .iter()
                .find(|(path, _)| path == &worktree.path)
                .map_or(0, |(_, count)| *count);
            rows.push(PageRow {
                kind: PageRowKind::Worktree,
                branch: worktree.branch.clone(),
                path: worktree.path.clone(),
                deletable: false,
                is_head: false,
                dirty: dirty_count > 0,
                dirty_count,
                stash_index: None,
                ahead: branch_info.and_then(|info| info.ahead),
                behind: branch_info.and_then(|info| info.behind),
                upstream: branch_info.and_then(|info| info.upstream.clone()),
                last_commit_rel: branch_info
                    .map(|info| info.last_commit_rel.clone())
                    .unwrap_or_default(),
                last_commit_secs: branch_info.map(|info| info.last_commit_secs).unwrap_or(0),
                subject: String::new(),
                stats: branch_info.and_then(|info| info.stats),
                commit_sha: branch_info.map(|info| info.commit_sha.clone()).unwrap_or_default(),
                author: branch_info.map(|info| info.author.clone()).unwrap_or_default(),
                merge_base_short: branch_info.and_then(|info| info.merge_base_short.clone()),
                base: branch_info.and_then(|info| info.base.clone()),
                base_is_override: branch_info.is_some_and(|info| info.base_is_override),
                parents: Vec::new(),
                upstream_gone: false,
            });
        }
        for stash in &page.stashes {
            rows.push(PageRow {
                kind: PageRowKind::Stash,
                branch: stash.label.clone(),
                path: repo_path.clone(),
                deletable: false,
                is_head: false,
                dirty: false,
                dirty_count: 0,
                stash_index: Some(stash.index),
                ahead: None,
                behind: None,
                upstream: None,
                // A stash is a commit — reuse the row's commit-date fields for its creation time.
                last_commit_rel: stash.date_rel.clone(),
                last_commit_secs: stash.created_secs,
                subject: String::new(),
                stats: stash.stats,
                commit_sha: String::new(),
                author: String::new(),
                merge_base_short: None,
                base: None,
                base_is_override: false,
                parents: Vec::new(),
                upstream_gone: false,
            });
        }
        for commit in &page.commits {
            rows.push(PageRow {
                kind: PageRowKind::Commit,
                branch: String::new(),
                path: repo_path.clone(),
                deletable: false,
                is_head: false,
                dirty: false,
                dirty_count: 0,
                stash_index: None,
                ahead: None,
                behind: None,
                upstream: None,
                last_commit_rel: commit.rel_date.clone(),
                last_commit_secs: 0,
                subject: commit.subject.clone(),
                stats: None,
                commit_sha: commit.sha.clone(),
                author: commit.author.clone(),
                merge_base_short: None,
                base: None,
                base_is_override: false,
                parents: commit.parents.clone(),
                upstream_gone: false,
            });
        }
        // Sort the branch and worktree sections independently by the active column (stashes +
        // commits keep their natural recency order). `None` leaves git's order (HEAD first).
        if let Some(sort) = self.repo_page_sort {
            let dir = self.repo_page_sort_dir;
            let branch_count = page.branches.len();
            let worktree_count = page.worktrees.len();
            let order = |first: &PageRow, second: &PageRow| {
                let ord = repo_page_row_cmp(sort, first, second);
                if dir == SortDir::Desc { ord.reverse() } else { ord }
            };
            rows[..branch_count].sort_by(order);
            rows[branch_count..branch_count + worktree_count].sort_by(order);
        }
        // Tabbed mode ONLY: keep just the active tab's rows (so selection / clicks / nav scope to
        // it). When maximized the page is a single stacked view of every section, so it must keep
        // ALL rows — mirror `repo_page_tabbed`'s condition (which excludes maximized) inline here,
        // computed from the locked `page` + the lock-free `maximized` field to avoid re-locking.
        let present = u8::from(!page.branches.is_empty())
            + u8::from(!page.worktrees.is_empty())
            + u8::from(!page.stashes.is_empty())
            + u8::from(!page.commits.is_empty());
        // Mirror `repo_page_tabbed` from the locked `page` (avoid re-locking): maximized opts into
        // tabs only via `repo_page_maximized_tabbed`; restored honors the `v` override over auto.
        let tabbed = if self.maximized == Some(Pane::RepoPage) {
            self.repo_page_maximized_tabbed && self.repo_page_tabs == RepoTabsMode::Auto && present >= 2
        } else {
            self.repo_page_tabbed_override
                .unwrap_or(self.repo_page_tabs == RepoTabsMode::Auto && present >= 2)
        };
        if tabbed {
            match self.repo_page_tab.row_kind() {
                Some(kind) => rows.retain(|row| row.kind == kind),
                None => rows.clear(),
            }
        } else {
            // Flat (stacked) view: hide rows in collapsed sections (their header stays, so they can
            // be re-expanded). Headers aren't rows, so nav/selection skip the hidden rows.
            rows.retain(|row| !self.repo_page_collapsed_sections.contains(row.tab().section_name()));
        }
        rows
    }

    /// `(branches, worktrees, stashes, commits)` counts for the open repo page (full, not filtered).
    pub fn repo_page_section_counts(&self) -> (usize, usize, usize, usize) {
        let Some(idx) = self.repo_page else { return (0, 0, 0, 0) };
        let state = self.repos[idx].lock().unwrap();
        state.page.as_ref().map_or((0, 0, 0, 0), |page| {
            (page.branches.len(), page.worktrees.len(), page.stashes.len(), page.commits.len())
        })
    }

    /// The repo-page tabs that have rows, in display order.
    pub fn repo_page_present_tabs(&self) -> Vec<RepoTab> {
        let (branches, worktrees, stashes, commits) = self.repo_page_section_counts();
        let mut tabs = Vec::new();
        if branches > 0 {
            tabs.push(RepoTab::Branches);
        }
        if worktrees > 0 {
            tabs.push(RepoTab::Worktrees);
        }
        if stashes > 0 {
            tabs.push(RepoTab::Stashes);
        }
        if commits > 0 {
            tabs.push(RepoTab::Commits);
        }
        tabs
    }

    /// Whether the repo page is currently rendered as tabs (mode Auto + ≥2 non-empty sections).
    /// Maximized is always a single, full view — every section stacked, no tab bar.
    pub fn repo_page_tabbed(&self) -> bool {
        // Restored: tabbed per `repo_page_tabs`. Maximized: flat stacked by default, tabbed only
        // when the `v` toggle (`repo_page_maximized_tabbed`) is on.
        if self.maximized == Some(Pane::RepoPage) {
            // Maximized stays flat-stacked by default; `v` (repo_page_maximized_tabbed) opts into tabs.
            return self.repo_page_maximized_tabbed
                && self.repo_page_tabs == RepoTabsMode::Auto
                && self.repo_page_present_tabs().len() >= 2;
        }
        // Restored: the auto decision (Auto mode + ≥2 sections), unless `v` set an explicit override.
        let auto = self.repo_page_tabs == RepoTabsMode::Auto && self.repo_page_present_tabs().len() >= 2;
        self.repo_page_tabbed_override.unwrap_or(auto)
    }

    /// Which columns dropdown the repo page's `t cols ▾` opens: the Stashes tab has its own
    /// (StashColumns); every other tab uses the branch-column dropdown (PageColumns).
    pub fn repo_page_cols_dropdown_kind(&self) -> DropdownKind {
        if self.repo_page_tabbed() && self.repo_page_tab == RepoTab::Stashes {
            DropdownKind::StashColumns
        } else {
            DropdownKind::PageColumns
        }
    }

    /// Switch the active repo-page tab, resetting the selection to its first row.
    pub fn repo_page_select_tab(&mut self, tab: RepoTab) {
        self.repo_page_tab = tab;
        self.repo_page_selected = 0;
        self.repo_page_scroll = 0;
    }

    /// Cycle to the next/previous present repo-page tab.
    /// Whether a repo-page section is collapsed in the flat (stacked) view.
    pub fn repo_page_section_collapsed(&self, tab: RepoTab) -> bool {
        self.repo_page_collapsed_sections.contains(tab.section_name())
    }

    /// Collapse/expand a repo-page section (flat view), persisting. Keeps the selection valid.
    pub fn toggle_repo_page_section(&mut self, tab: RepoTab) {
        let name = tab.section_name().to_string();
        if !self.repo_page_collapsed_sections.remove(&name) {
            self.repo_page_collapsed_sections.insert(name);
        }
        let len = self.repo_page_selectable_len();
        if self.repo_page_selected >= len {
            self.repo_page_selected = len.saturating_sub(1);
        }
        self.save_state();
    }

    /// Collapse/expand the section the selected row belongs to (the `z` key).
    pub fn toggle_selected_repo_page_section(&mut self) {
        if let Some(row) = self.repo_page_target() {
            self.toggle_repo_page_section(row.tab());
        }
    }

    /// Expand every section if any is collapsed, else collapse them all (`Z`). The keyboard way to
    /// re-expand a section whose rows are hidden (so `z` can no longer reach it).
    pub fn toggle_all_repo_page_sections(&mut self) {
        if self.repo_page_collapsed_sections.is_empty() {
            for tab in self.repo_page_present_tabs() {
                self.repo_page_collapsed_sections.insert(tab.section_name().to_string());
            }
        } else {
            self.repo_page_collapsed_sections.clear();
        }
        let len = self.repo_page_selectable_len();
        if self.repo_page_selected >= len {
            self.repo_page_selected = len.saturating_sub(1);
        }
        self.save_state();
    }

    pub fn repo_page_cycle_tab(&mut self, forward: bool) {
        let tabs = self.repo_page_present_tabs();
        if tabs.is_empty() {
            return;
        }
        let current = tabs.iter().position(|&tab| tab == self.repo_page_tab).unwrap_or(0);
        let next = if forward {
            (current + 1) % tabs.len()
        } else {
            (current + tabs.len() - 1) % tabs.len()
        };
        self.repo_page_select_tab(tabs[next]);
    }

    /// Set/flip the repo-page branch-table sort, keeping the selection on the same row.
    pub fn set_repo_page_sort(&mut self, sort: RepoPageSort) {
        let prev = self
            .repo_page_rows()
            .get(self.repo_page_selected)
            .map(|row| (row.kind, row.branch.clone(), row.stash_index));
        if self.repo_page_sort == Some(sort) {
            self.repo_page_sort_dir = self.repo_page_sort_dir.flip();
        } else {
            self.repo_page_sort = Some(sort);
            self.repo_page_sort_dir = SortDir::Asc;
        }
        if let Some(prev) = prev {
            let rows = self.repo_page_rows();
            if let Some(index) = rows
                .iter()
                .position(|row| (row.kind, row.branch.clone(), row.stash_index) == prev)
            {
                self.repo_page_selected = index;
            }
        }
    }

    /// The sort column whose clickable header contains `(col,row)`, if any.
    pub fn repo_page_sort_at(&self, col: u16, row: u16) -> Option<RepoPageSort> {
        self.repo_page_sort_click
            .iter()
            .find(|(header_row, start, end, _)| *header_row == row && col >= *start && col < *end)
            .map(|(_, _, _, sort)| *sort)
    }

    /// Toggle a repo-page branch column on/off.
    pub fn toggle_repo_page_column(&mut self, column: RepoPageColumn) {
        let columns = &mut self.repo_page_columns;
        match column {
            RepoPageColumn::AheadBehind => columns.ahead_behind = !columns.ahead_behind,
            RepoPageColumn::Dirty => columns.dirty = !columns.dirty,
            RepoPageColumn::Added => columns.added = !columns.added,
            RepoPageColumn::Modified => columns.modified = !columns.modified,
            RepoPageColumn::Deleted => columns.deleted = !columns.deleted,
            RepoPageColumn::Total => columns.total = !columns.total,
            RepoPageColumn::Upstream => columns.upstream = !columns.upstream,
            RepoPageColumn::Base => columns.base = !columns.base,
            RepoPageColumn::Age => columns.age = !columns.age,
            RepoPageColumn::PullRequest => columns.pull_request = !columns.pull_request,
            RepoPageColumn::Subject => columns.subject = !columns.subject,
        }
    }

    /// Whether a repo-page column has any meaningful value on the open page (or is still loading).
    /// Stats columns count unknown (not-yet-loaded) branches as available, so they don't flicker.
    pub fn repo_page_column_available(&self, column: RepoPageColumn) -> bool {
        let Some(idx) = self.repo_page else {
            return true;
        };
        let state = self.repos[idx].lock().unwrap();
        let Some(page) = state.page.as_ref() else {
            return true;
        };
        match column {
            RepoPageColumn::Age | RepoPageColumn::Subject | RepoPageColumn::Base => true,
            // The PR column only carries data when the repo's current branch has an open PR.
            RepoPageColumn::PullRequest => state.pr.is_some(),
            RepoPageColumn::AheadBehind | RepoPageColumn::Upstream => {
                page.branches.iter().any(|branch| branch.upstream.is_some())
            }
            RepoPageColumn::Dirty => {
                page.head_dirty_count > 0
                    || page.dirty_worktrees.iter().any(|(_, count)| *count > 0)
            }
            RepoPageColumn::Added
            | RepoPageColumn::Modified
            | RepoPageColumn::Deleted
            | RepoPageColumn::Total => page.branches.iter().any(|branch| match branch.stats {
                None => true,
                Some(stats) => match column {
                    RepoPageColumn::Added => stats.added > 0,
                    RepoPageColumn::Modified => stats.modified > 0,
                    RepoPageColumn::Deleted => stats.deleted > 0,
                    _ => stats.total() > 0,
                },
            }),
        }
    }

    /// The repo-page columns actually rendered: enabled flags minus unavailable ones.
    pub fn effective_repo_page_columns(&self) -> RepoPageColumns {
        let columns = self.repo_page_columns;
        let on = |flag: bool, column: RepoPageColumn| flag && self.repo_page_column_available(column);
        RepoPageColumns {
            ahead_behind: on(columns.ahead_behind, RepoPageColumn::AheadBehind),
            dirty: on(columns.dirty, RepoPageColumn::Dirty),
            added: on(columns.added, RepoPageColumn::Added),
            modified: on(columns.modified, RepoPageColumn::Modified),
            deleted: on(columns.deleted, RepoPageColumn::Deleted),
            total: on(columns.total, RepoPageColumn::Total),
            upstream: on(columns.upstream, RepoPageColumn::Upstream),
            base: on(columns.base, RepoPageColumn::Base),
            age: columns.age,
            pull_request: on(columns.pull_request, RepoPageColumn::PullRequest),
            subject: columns.subject,
        }
    }

    /// Build a `DiffSource` for the selected repo-page row if it's diff-able
    /// (a stash, or a dirty branch/worktree); otherwise None.
    pub fn diff_source_for_selected(&self) -> Option<DiffSource> {
        let row = self.repo_page_target()?;
        match row.kind {
            PageRowKind::Stash => Some(DiffSource::Stash {
                path: row.path,
                index: row.stash_index?,
                label: row.branch,
            }),
            // A dirty branch/worktree shows its uncommitted (toggle to base) diff; a clean one
            // shows what the branch added vs its base branch.
            PageRowKind::Branch | PageRowKind::Worktree if row.dirty => Some(DiffSource::Dirty {
                path: row.path,
                name: row.branch,
            }),
            PageRowKind::Branch | PageRowKind::Worktree => Some(DiffSource::Branch {
                path: row.path,
                name: row.branch,
            }),
            PageRowKind::Commit => Some(DiffSource::Commit {
                path: row.path,
                sha: row.commit_sha,
                label: row.subject,
            }),
        }
    }

    /// Open the diff modal in a loading state for `source`.
    /// Open the PR viewer modal for a repo's current PR (if it has one). Returns whether it opened.
    pub fn open_pr_modal_for_repo(&mut self, repo_idx: usize) -> bool {
        let pr = self.repos.get(repo_idx).and_then(|repo| repo.lock().unwrap().pr.clone());
        match pr {
            Some(pr) => {
                self.open_pr_modal(repo_idx, pr.number, pr.url, pr.title);
                true
            }
            None => false,
        }
    }

    /// Open the PR viewer modal in a loading state for a repo's PR (the body loads via `gh pr view`).
    pub fn open_pr_modal(&mut self, repo_idx: usize, number: u32, url: String, title: String) {
        self.pr_modal = Some(crate::app::PrModalState {
            repo_idx,
            number,
            url,
            title,
            view: None,
            scroll: 0,
            collapsed: std::collections::HashSet::new(),
            search: String::new(),
            search_focused: false,
            tab: crate::app::PrModalTab::default(),
            files_diff: None,
            files_diff_loading: false,
            files_view: crate::app::DiffView::Unified,
        });
    }

    /// Switch the PR viewer to `tab`, resetting scroll (each tab starts at the top).
    pub fn pr_modal_select_tab(&mut self, tab: crate::app::PrModalTab) {
        if let Some(modal) = self.pr_modal.as_mut() {
            modal.tab = tab;
            modal.scroll = 0;
            modal.search_focused = false;
        }
    }

    /// Cycle the PR viewer's tab (Tab / Shift+Tab).
    pub fn pr_modal_cycle_tab(&mut self, forward: bool) {
        if let Some(modal) = self.pr_modal.as_ref() {
            let next = modal.tab.cycle(forward);
            self.pr_modal_select_tab(next);
        }
    }

    /// A state-aware cleanup prompt for an AI agent — every repo fact already embedded so the agent
    /// doesn't have to re-run `git`/`gh` to discover the situation. Only includes the sections that
    /// apply (stashes / extra branches / worktrees), and asks for a concrete cleanup pass.
    pub fn kebab_cleanup_prompt(&self, repo_idx: usize) -> String {
        let Some(repo) = self.repos.get(repo_idx) else {
            return String::new();
        };
        let state = repo.lock().unwrap();
        let branch = state.branch.clone().unwrap_or_else(|| "?".to_string());
        let details = state.details.as_ref();
        let ahead = details.and_then(|info| info.ahead).unwrap_or(0);
        let behind = details.and_then(|info| info.behind).unwrap_or(0);
        let dirty = details.map(|info| info.dirty_count).unwrap_or(0);
        let stashes = details.map(|info| info.stash_count).unwrap_or(0);
        let branches = details.map(|info| info.branch_count).unwrap_or(0);
        let worktrees = self.worktrees.iter().filter(|wt| wt.repo == state.name).count();
        let pr = state
            .pr
            .as_ref()
            .map(|pr| format!("#{} \"{}\" ({})", pr.number, pr.title, pr.state.label()));

        let mut prompt = String::new();
        prompt.push_str(&format!(
            "Review and help clean up the git repository at `{}`.\n\n",
            state.path.display()
        ));
        prompt.push_str("Current state (already gathered — don't re-run these to discover it):\n");
        prompt.push_str(&format!("- Branch: `{branch}` (ahead {ahead}, behind {behind} vs upstream)\n"));
        prompt.push_str(&format!(
            "- Working tree: {}\n",
            if dirty == 0 { "clean".to_string() } else { format!("{dirty} uncommitted change(s)") }
        ));
        prompt.push_str(&format!("- Stashes: {stashes}\n"));
        prompt.push_str(&format!("- Local branches (excl. main/dev): {branches}\n"));
        prompt.push_str(&format!("- Worktrees: {worktrees}\n"));
        if let Some(pr) = pr {
            prompt.push_str(&format!("- Open PR for this branch: {pr}\n"));
        }
        prompt.push_str("\nPlease do a cleanup pass and run the git/gh commands yourself (don't ask me to run them):\n");
        let mut step = 1;
        if stashes > 0 {
            prompt.push_str(&format!(
                "{step}. For each stash, check its age and whether its changes are already merged into the current branch or main (`git stash list --date=relative`, `git stash show -p stash@{{i}}`); report which are stale/redundant and drop the safe ones.\n"
            ));
            step += 1;
        }
        if branches > 0 {
            prompt.push_str(&format!(
                "{step}. For each local branch, check if it's merged and whether its upstream is gone; delete the ones that are safely removable.\n"
            ));
            step += 1;
        }
        if worktrees > 0 {
            prompt.push_str(&format!(
                "{step}. For each worktree, check if its branch is merged/stale and prune it if so (`git worktree list`, `git worktree remove`).\n"
            ));
            step += 1;
        }
        prompt.push_str(&format!("{step}. Summarize exactly what you changed and what you left alone, and why.\n"));
        prompt
    }

    /// The text the kebab "Copy cleanup prompt" puts on the clipboard — the bare prompt, or wrapped
    /// as `cd <repo> && claude '<prompt>'` when the session-prefix checkbox is on (single quotes in
    /// the prompt are escaped so the shell command stays valid).
    pub fn kebab_copy_text(&self, repo_idx: usize) -> String {
        let prompt = self.kebab_cleanup_prompt(repo_idx);
        if !self.kebab_session_prefix {
            return prompt;
        }
        let Some(repo) = self.repos.get(repo_idx) else {
            return prompt;
        };
        let path = repo.lock().unwrap().path.display().to_string();
        let escaped = prompt.replace('\'', "'\\''");
        format!("cd {path} && claude '{escaped}'")
    }

    /// Build the kebab menu items for a repo from its current state (dynamic — diff only when dirty,
    /// open-remote only with a remote URL, etc.). The session-prefix checkbox is always present.
    pub fn build_kebab_items(&self, repo_idx: usize) -> Vec<KebabItem> {
        let Some(repo) = self.repos.get(repo_idx) else {
            return Vec::new();
        };
        let state = repo.lock().unwrap();
        let dirty = state.details.as_ref().map(|info| info.dirty_count).unwrap_or(0);
        let has_remote = state.remote_url.is_some();
        let agent = self.claude_agent.binary();
        let checkbox = if self.kebab_session_prefix { "[x]" } else { "[ ]" };
        let favorited = self.favorites.contains(&favorite_key(&state.path));
        vec![
            KebabItem {
                label: if favorited { "★ Unfavorite".to_string() } else { "☆ Favorite".to_string() },
                action: KebabAction::ToggleFavorite,
                enabled: true,
                hint: Some("b".to_string()),
            },
            KebabItem {
                label: "Checkout branch…".to_string(),
                action: KebabAction::Checkout,
                enabled: true,
                hint: None,
            },
            KebabItem {
                label: "Copy cleanup prompt".to_string(),
                action: KebabAction::CopyCleanupPrompt,
                enabled: true,
                hint: None,
            },
            KebabItem {
                label: format!("{checkbox} include `cd … && {agent} '…'`"),
                action: KebabAction::ToggleSessionPrefix,
                enabled: true,
                hint: None,
            },
            KebabItem {
                label: format!("Run {agent}"),
                action: KebabAction::Claude,
                enabled: true,
                hint: Some("c".to_string()),
            },
            KebabItem {
                label: "Explore files…".to_string(),
                action: KebabAction::Explore,
                enabled: true,
                hint: Some("^E".to_string()),
            },
            KebabItem {
                label: "Open lazygit".to_string(),
                action: KebabAction::Lazygit,
                enabled: true,
                hint: Some("l".to_string()),
            },
            KebabItem {
                label: "View diff".to_string(),
                action: KebabAction::Diff,
                enabled: dirty > 0,
                hint: Some("d".to_string()),
            },
            KebabItem {
                label: "Refetch".to_string(),
                action: KebabAction::Refetch,
                enabled: true,
                hint: Some("e".to_string()),
            },
            KebabItem {
                label: "Open remote".to_string(),
                action: KebabAction::OpenRemote,
                enabled: has_remote,
                hint: Some("o".to_string()),
            },
        ]
    }

    /// Open the kebab menu for `repo_idx` (building its state-aware items). The menu anchors to the
    /// repo's `⋮` button (from the last frame's captured regions) so it opens left-aligned under it;
    /// falls back to the list pane's right edge when the row isn't currently captured.
    pub fn open_kebab(&mut self, repo_idx: usize) {
        let items = self.build_kebab_items(repo_idx);
        let anchor = self
            .kebab_open_click
            .iter()
            .find(|(_, _, _, idx)| *idx == repo_idx)
            .map(|&(row, _, end, _)| (row, end))
            .unwrap_or((
                self.list_rows_area.y,
                self.list_rows_area.x + self.list_rows_area.width,
            ));
        self.kebab = Some(KebabMenu {
            repo_idx,
            items,
            selected: 0,
            anchor_row: anchor.0,
            anchor_right: anchor.1,
        });
    }

    pub fn close_kebab(&mut self) {
        self.kebab = None;
    }

    // ── File explorer ───────────────────────────────────────────────────────────────────────────

    /// Open the file explorer rooted at `repo_idx`'s directory (seeded with the persisted columns).
    pub fn open_explorer(&mut self, repo_idx: usize) {
        let Some(repo) = self.repos.get(repo_idx) else {
            return;
        };
        let root = repo.lock().unwrap().path.clone();
        self.close_all_modals();
        self.explorer = Some(crate::explorer::Explorer::open(root, self.explorer_prefs));
    }

    /// Open the explorer for the currently-selected repo (the `Explore` key).
    pub fn open_explorer_selected(&mut self) {
        if let Some(idx) = self.selected_repo_index() {
            self.open_explorer(idx);
        }
    }

    pub fn close_explorer(&mut self) {
        self.explorer = None;
    }

    /// Toggle an explorer column on both the live explorer and the persisted prefs, then save.
    pub fn toggle_explorer_column(&mut self, column: crate::explorer::ExplorerColumn) {
        use crate::explorer::ExplorerColumn;
        let columns = &mut self.explorer_prefs.columns;
        let slot = match column {
            ExplorerColumn::Size => &mut columns.size,
            ExplorerColumn::Permissions => &mut columns.permissions,
            ExplorerColumn::Modified => &mut columns.modified,
            ExplorerColumn::Created => &mut columns.created,
            ExplorerColumn::Kind => &mut columns.kind,
        };
        *slot = !*slot;
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.columns = self.explorer_prefs.columns;
        }
        self.save_state();
    }

    /// Set the explorer's sort key (toggling direction on the same key); persists the result.
    pub fn set_explorer_sort(&mut self, key: crate::explorer::SortKey) {
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.set_sort(key);
            self.explorer_prefs.sort = explorer.sort;
            self.explorer_prefs.sort_ascending = explorer.sort_ascending;
            self.save_state();
        }
    }

    /// Toggle the explorer between the recursive tree view and the flat folder view; persists.
    pub fn toggle_explorer_tree_mode(&mut self) {
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.toggle_tree_mode();
            self.explorer_prefs.tree_mode = explorer.tree_mode;
            self.save_state();
        }
    }

    /// Step the tree expansion one level deeper / shallower (the level stepper buttons/keys).
    pub fn explorer_expand_level(&mut self, deeper: bool) {
        if let Some(explorer) = self.explorer.as_mut() {
            if !explorer.tree_mode {
                explorer.toggle_tree_mode();
                self.explorer_prefs.tree_mode = true;
                self.save_state();
            }
            if let Some(explorer) = self.explorer.as_mut() {
                let next = if deeper { explorer.tree_level + 1 } else { explorer.tree_level.saturating_sub(1) };
                explorer.expand_to_level(next);
            }
        }
    }

    /// Toggle the explorer's time columns between relative ("2d ago") and absolute stamps; persists.
    pub fn toggle_explorer_date_format(&mut self) {
        use crate::explorer::DateFormat;
        let next = match self.explorer_prefs.date_format {
            DateFormat::Relative => DateFormat::Stamp,
            DateFormat::Stamp => DateFormat::Relative,
        };
        self.explorer_prefs.date_format = next;
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.date_format = next;
        }
        self.save_state();
    }

    /// Move the kebab highlight by `delta`, skipping disabled rows, clamped to the menu.
    pub fn kebab_move(&mut self, delta: isize) {
        let Some(menu) = self.kebab.as_mut() else {
            return;
        };
        let len = menu.items.len();
        if len == 0 {
            return;
        }
        let mut idx = menu.selected as isize;
        for _ in 0..len {
            idx = (idx + delta).rem_euclid(len as isize);
            if menu.items[idx as usize].enabled {
                menu.selected = idx as usize;
                return;
            }
        }
    }

    /// Open the branch-checkout picker for `repo_idx` (empty + loading; the worker fills branches).
    pub fn open_branch_picker(&mut self, repo_idx: usize) {
        self.branch_picker =
            Some(BranchPicker { repo_idx, branches: Vec::new(), filter: String::new(), selected: 0, loading: true });
    }

    pub fn close_branch_picker(&mut self) {
        self.branch_picker = None;
    }

    /// Move the branch-picker highlight by `delta`, clamped to the filtered list.
    pub fn branch_picker_move(&mut self, delta: isize) {
        let Some(picker) = self.branch_picker.as_mut() else {
            return;
        };
        let len = picker.filtered().len();
        if len == 0 {
            picker.selected = 0;
            return;
        }
        let last = (len - 1) as isize;
        picker.selected = (picker.selected as isize + delta).clamp(0, last) as usize;
    }

    pub fn open_diff_modal(&mut self, source: DiffSource) {
        self.diff_modal = Some(DiffModal {
            source,
            mode: DiffMode::Uncommitted,
            view: self.diff_view,
            focus: DiffFocus::Files,
            files: Vec::new(),
            selected: 0,
            file_scroll: 0,
            lines: vec!["(loading…)".to_string()],
            scroll: 0,
            loading: true,
            diff_loading: true,
            status_filter: None,
        });
    }

    /// Cycle the diff render style (raw → unified → split), persisting the choice for new modals.
    pub fn diff_modal_cycle_view(&mut self) {
        self.diff_view = self.diff_view.cycle();
        if let Some(modal) = self.diff_modal.as_mut() {
            modal.view = self.diff_view;
            modal.scroll = 0;
        }
        self.save_state();
    }

    /// Toggle which diff-modal panel `j/k/g/G` drive (`Tab`).
    pub fn diff_modal_toggle_focus(&mut self) {
        if let Some(modal) = self.diff_modal.as_mut() {
            modal.focus = match modal.focus {
                DiffFocus::Files => DiffFocus::Diff,
                DiffFocus::Diff => DiffFocus::Files,
            };
        }
    }

    /// Toggle a dirty-row diff between uncommitted and base-branch views, returning true if
    /// a recompute is needed (i.e. the source supports toggling). Stash diffs don't toggle.
    pub fn diff_modal_toggle_mode(&mut self) -> bool {
        let Some(modal) = self.diff_modal.as_mut() else {
            return false;
        };
        if !matches!(modal.source, DiffSource::Dirty { .. }) {
            return false;
        }
        modal.mode = match modal.mode {
            DiffMode::Uncommitted => DiffMode::BaseBranch,
            DiffMode::BaseBranch => DiffMode::Uncommitted,
        };
        modal.files = Vec::new();
        modal.selected = 0;
        modal.file_scroll = 0;
        modal.lines = vec!["(loading…)".to_string()];
        modal.scroll = 0;
        modal.loading = true;
        modal.diff_loading = true;
        modal.status_filter = None;
        true
    }

    /// Move the diff modal's file selection by `delta` positions in the *visible* (filtered,
    /// grouped) list, clamped. `selected` itself stays an absolute index into `files`. Returns
    /// true if it changed (so the caller can refetch the newly-selected file's diff).
    pub fn diff_modal_select(&mut self, delta: isize) -> bool {
        let viewport = self.diff_files_viewport;
        let Some(modal) = self.diff_modal.as_mut() else {
            return false;
        };
        let visible = modal.visible_file_indices();
        if visible.is_empty() {
            return false;
        }
        let pos = visible.iter().position(|&index| index == modal.selected).unwrap_or(0);
        let next_pos = (pos as isize + delta).clamp(0, visible.len() as isize - 1) as usize;
        let next = visible[next_pos];
        if next == modal.selected {
            return false;
        }
        modal.selected = next;
        modal.scroll = 0;
        modal.lines = vec!["(loading…)".to_string()];
        modal.diff_loading = true;
        Self::keep_file_selected_visible(modal, viewport);
        true
    }

    /// Select a diff-modal file by its position in the *visible* list (mouse click / g/G).
    /// Returns true if the absolute selection changed.
    pub fn diff_modal_select_index(&mut self, pos: usize) -> bool {
        let viewport = self.diff_files_viewport;
        let Some(modal) = self.diff_modal.as_mut() else {
            return false;
        };
        let visible = modal.visible_file_indices();
        let Some(&next) = visible.get(pos) else {
            return false;
        };
        if next == modal.selected {
            return false;
        }
        modal.selected = next;
        modal.scroll = 0;
        modal.lines = vec!["(loading…)".to_string()];
        modal.diff_loading = true;
        Self::keep_file_selected_visible(modal, viewport);
        true
    }

    /// Apply a status filter (`None` = all). Returns true if the selection moved to the first
    /// visible file (because the previous selection was filtered out) and needs a diff refetch.
    pub fn diff_modal_set_filter(&mut self, status: Option<char>) -> bool {
        let viewport = self.diff_files_viewport;
        let Some(modal) = self.diff_modal.as_mut() else {
            return false;
        };
        modal.status_filter = status;
        modal.file_scroll = 0;
        let visible = modal.visible_file_indices();
        if visible.contains(&modal.selected) {
            Self::keep_file_selected_visible(modal, viewport);
            return false;
        }
        let Some(&first) = visible.first() else {
            return false;
        };
        modal.selected = first;
        modal.scroll = 0;
        modal.lines = vec!["(loading…)".to_string()];
        modal.diff_loading = true;
        Self::keep_file_selected_visible(modal, viewport);
        true
    }

    /// Cycle the status filter: all → each present chip in order → all. No-op without chips.
    pub fn diff_modal_cycle_filter(&mut self) -> bool {
        let Some(modal) = self.diff_modal.as_ref() else {
            return false;
        };
        if !modal.chips_active() {
            return false;
        }
        let chips: Vec<char> = modal.status_chips().into_iter().map(|(bucket, _)| bucket).collect();
        let next = match modal.status_filter {
            None => chips.first().copied(),
            Some(current) => {
                let pos = chips.iter().position(|&bucket| bucket == current);
                match pos {
                    Some(index) => chips.get(index + 1).copied(),
                    None => chips.first().copied(),
                }
            }
        };
        self.diff_modal_set_filter(next)
    }

    /// Nudge the file-list scroll so the selected file's *visible position* stays in view (used
    /// after keyboard moves; scrollbar/wheel scrolling leaves the selection alone).
    fn keep_file_selected_visible(modal: &mut DiffModal, viewport: usize) {
        if viewport == 0 {
            return;
        }
        let visible = modal.visible_file_indices();
        let Some(pos) = visible.iter().position(|&index| index == modal.selected) else {
            return;
        };
        if pos < modal.file_scroll {
            modal.file_scroll = pos;
        } else if pos >= modal.file_scroll + viewport {
            modal.file_scroll = pos + 1 - viewport;
        }
    }

    /// Scroll the diff modal's file-list view by `delta` rows (Shift+wheel), selection unchanged.
    pub fn diff_files_scroll(&mut self, delta: isize) {
        let viewport = self.diff_files_viewport;
        if let Some(modal) = self.diff_modal.as_mut() {
            let max = modal.visible_file_indices().len().saturating_sub(viewport);
            let next = (modal.file_scroll as isize + delta).clamp(0, max as isize);
            modal.file_scroll = next as usize;
        }
    }

    /// The status-filter chip at `(col,row)`, if any. The outer `Option` distinguishes "no chip
    /// here" from the inner `Option<char>` (the "all" chip carries `None`).
    pub fn diff_chip_at(&self, col: u16, row: u16) -> Option<Option<char>> {
        self.diff_chips_click
            .iter()
            .find(|(chip_row, start, end, _)| *chip_row == row && col >= *start && col < *end)
            .map(|(_, _, _, bucket)| *bucket)
    }

    /// The *visible-list position* at a screen row inside the file-list panel (mouse hit-test).
    pub fn diff_modal_file_at(&self, row: u16) -> Option<usize> {
        let modal = self.diff_modal.as_ref()?;
        let area = self.diff_files_area;
        if row < area.y || row >= area.y + area.height {
            return None;
        }
        let pos = (row - area.y) as usize + modal.file_scroll;
        (pos < modal.visible_file_indices().len()).then_some(pos)
    }

    pub fn repo_page_selectable_len(&self) -> usize {
        self.repo_page_rows().len()
    }

    /// The currently selected repo-page row, if any.
    pub fn repo_page_target(&self) -> Option<PageRow> {
        self.repo_page_rows().into_iter().nth(self.repo_page_selected)
    }

    /// The selectable repo-page row at a screen row, if any (mouse hit-test).
    pub fn repo_page_row_at(&self, row: u16) -> Option<usize> {
        self.repo_page_click
            .iter()
            .find(|(click_row, _)| *click_row == row)
            .map(|(_, index)| *index)
    }

    pub fn toggle_column(&mut self, column: Column) {
        match column {
            Column::Status => self.columns.status = !self.columns.status,
            Column::AheadBehind => self.columns.ahead_behind = !self.columns.ahead_behind,
            Column::Dirty => self.columns.dirty = !self.columns.dirty,
            Column::LastCommit => self.columns.last_commit = !self.columns.last_commit,
            Column::Worktrees => self.columns.worktrees = !self.columns.worktrees,
            Column::Branches => self.columns.branches = !self.columns.branches,
            Column::Stashes => self.columns.stashes = !self.columns.stashes,
            Column::PulledCommits => self.columns.pulled_commits = !self.columns.pulled_commits,
            Column::PulledFiles => self.columns.pulled_files = !self.columns.pulled_files,
            Column::PullRequest => {
                self.columns.pull_request = !self.columns.pull_request;
                // Re-arm the all-repos PR pass so re-enabling re-resolves stale entries.
                if !self.columns.pull_request {
                    self.pr_pass_spawned = false;
                }
            }
            Column::Favorite => self.columns.favorite = !self.columns.favorite,
        }
    }

    /// Whether any repo recorded a pull delta this session (drives the pulled-column auto-hide).
    fn any_pull_result(&self) -> bool {
        self.repos.iter().any(|repo| repo.lock().unwrap().pull_result.is_some())
    }

    /// Latch the pulled/chg columns on once a pull delta appears. Called each frame; cheap once set.
    pub fn refresh_pulled_seen(&mut self) {
        if !self.pulled_seen && self.any_pull_result() {
            self.pulled_seen = true;
        }
    }

    /// Whether a column could show a meaningful value, or is still loading. Hidden only once
    /// everything it depends on has loaded AND every repo's value is trivial; pending data
    /// counts as available so columns don't flicker mid-scan. The always-on columns
    /// (ahead/behind, dirty, last-commit) are never hidden.
    pub fn column_available(&self, column: Column) -> bool {
        match column {
            Column::Status | Column::AheadBehind | Column::Dirty | Column::LastCommit => true,
            Column::Worktrees => {
                if !self.worktrees_done {
                    return true;
                }
                self.repos.iter().any(|repo| {
                    let name = repo.lock().unwrap().name.clone();
                    self.worktrees.iter().any(|entry| entry.repo == name)
                })
            }
            Column::Branches => {
                if !self.discovery_done {
                    return true;
                }
                self.repos.iter().any(|repo| {
                    match repo.lock().unwrap().details.as_ref() {
                        None => true,
                        Some(details) => details.branch_count > 1,
                    }
                })
            }
            Column::Stashes => {
                if !self.discovery_done {
                    return true;
                }
                self.repos.iter().any(|repo| {
                    match repo.lock().unwrap().details.as_ref() {
                        None => true,
                        Some(details) => details.stash_count > 0,
                    }
                })
            }
            // The pulled columns come from the pulls themselves. Once any pull has landed a delta
            // this session the columns latch on (`pulled_seen`) and stay — so a retry/refetch, which
            // briefly clears every `pull_result`, no longer flickers them out and back in.
            Column::PulledCommits | Column::PulledFiles => self.pulled_seen,
            // Self-fills via `gh` in the background; always available when enabled (cells are
            // blank for repos without a PR or not yet resolved).
            Column::PullRequest => true,
            // The star is always meaningful (it's how you favorite a repo).
            Column::Favorite => true,
        }
    }

    /// The columns actually rendered: enabled flags minus any that are currently unavailable.
    pub fn effective_columns(&self) -> ColumnFlags {
        ColumnFlags {
            status: self.columns.status,
            ahead_behind: self.columns.ahead_behind,
            dirty: self.columns.dirty,
            last_commit: self.columns.last_commit,
            worktrees: self.columns.worktrees && self.column_available(Column::Worktrees),
            branches: self.columns.branches && self.column_available(Column::Branches),
            stashes: self.columns.stashes && self.column_available(Column::Stashes),
            pulled_commits: self.columns.pulled_commits
                && self.column_available(Column::PulledCommits),
            pulled_files: self.columns.pulled_files && self.column_available(Column::PulledFiles),
            pull_request: self.columns.pull_request && self.column_available(Column::PullRequest),
            favorite: self.columns.favorite,
        }
    }

    // ── Keybindings editor ──────────────────────────────────────────────────────────────────────

    /// Open the keybindings editor (remap shortcuts), clearing any transient sub-state.
    pub fn open_keybindings(&mut self) {
        self.show_keybindings = true;
        self.keybindings_capture = None;
        self.keybindings_conflict = None;
        self.keybindings_reset_confirm = false;
        self.keybindings_status = None;
        let last = crate::keybindings::action_defs().len().saturating_sub(1);
        self.keybindings_selected = self.keybindings_selected.min(last);
    }

    /// Close the editor and drop any capture/conflict/confirm sub-state.
    pub fn close_keybindings(&mut self) {
        self.show_keybindings = false;
        self.keybindings_capture = None;
        self.keybindings_conflict = None;
        self.keybindings_reset_confirm = false;
    }

    /// The action currently selected in the editor.
    pub fn keybindings_selected_action(&self) -> crate::keybindings::KeyAction {
        crate::keybindings::action_defs()[self.keybindings_selected].action
    }

    /// Move the editor selection by `delta` rows (action-index space; clamps to range).
    pub fn keybindings_move(&mut self, delta: isize) {
        let len = crate::keybindings::action_defs().len() as isize;
        let next = (self.keybindings_selected as isize).saturating_add(delta).clamp(0, (len - 1).max(0));
        self.keybindings_selected = next as usize;
        self.keybindings_status = None;
    }

    /// Select an action by its index (clicked row).
    pub fn keybindings_select(&mut self, index: usize) {
        let last = crate::keybindings::action_defs().len().saturating_sub(1);
        self.keybindings_selected = index.min(last);
        self.keybindings_status = None;
    }

    /// Enter capture mode for an action: the next keypress becomes its binding.
    pub fn keybindings_start_capture(&mut self, action: crate::keybindings::KeyAction) {
        self.keybindings_capture = Some(action);
        self.keybindings_conflict = None;
        self.keybindings_reset_confirm = false;
        self.keybindings_status = None;
    }

    /// Apply a captured chord to the action being bound, routing through conflict detection.
    pub fn keybindings_apply_capture(&mut self, chord: crate::keybindings::KeyChord) {
        let Some(action) = self.keybindings_capture.take() else {
            return;
        };
        let label = crate::keybindings::def_for(action).label;
        if let Some(other) = self.keybindings.conflict(action, chord) {
            self.keybindings_conflict = Some((action, chord, other));
            return;
        }
        self.keybindings.set_only(action, chord);
        self.keybindings.save();
        self.keybindings_status = Some(format!("bound {} → {label}", chord.display()));
    }

    /// Resolve a pending conflict: `accept` reassigns the chord (taking it from the other action),
    /// otherwise the bind is discarded.
    pub fn keybindings_resolve_conflict(&mut self, accept: bool) {
        let Some((action, chord, _)) = self.keybindings_conflict.take() else {
            return;
        };
        if accept {
            self.keybindings.unbind(chord);
            self.keybindings.set_only(action, chord);
            self.keybindings.save();
            let label = crate::keybindings::def_for(action).label;
            self.keybindings_status = Some(format!("reassigned {} → {label}", chord.display()));
        }
    }

    /// Clear the selected action's binding (the key does nothing until rebound).
    pub fn keybindings_clear_selected(&mut self) {
        let action = self.keybindings_selected_action();
        self.keybindings.clear(action);
        self.keybindings.save();
        let label = crate::keybindings::def_for(action).label;
        self.keybindings_status = Some(format!("cleared {label}"));
    }

    /// Reset the selected action to its default chords.
    pub fn keybindings_reset_selected(&mut self) {
        let action = self.keybindings_selected_action();
        self.keybindings.reset(action);
        self.keybindings.save();
        let label = crate::keybindings::def_for(action).label;
        self.keybindings_status = Some(format!("reset {label}"));
    }

    /// Reset every shortcut to its default.
    pub fn keybindings_reset_all(&mut self) {
        self.keybindings.reset_all();
        self.keybindings.save();
        self.keybindings_reset_confirm = false;
        self.keybindings_status = Some("all shortcuts reset to defaults".to_string());
    }
}
