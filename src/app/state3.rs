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
            || self.copy_menu.is_some()
            || self.base_picker.is_some()
            || self.show_changelog
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
        self.copy_menu = None;
        self.base_picker = None;
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
                "v{version} predates in-app version selection — you won't be able to switch versions from inside the app."
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

    /// Whether a point lands on the repo-page title-bar buttons (maximize/restore or `[esc back]`).
    /// The restored panel's top border doubles as the resize handle, so these columns must be
    /// excluded from the splitter grab or the buttons could never be clicked.
    pub fn title_button_hit(&self, col: u16, row: u16) -> bool {
        let hit = |region: Option<(u16, u16, u16)>| {
            region.is_some_and(|(button_row, start, end)| row == button_row && col >= start && col < end)
        };
        hit(self.repo_page_back_click) || hit(self.repo_page_window_click)
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
                last_commit_rel: String::new(),
                last_commit_secs: 0,
                subject: String::new(),
                stats: stash.stats,
                commit_sha: String::new(),
                author: String::new(),
                merge_base_short: None,
                base: None,
                base_is_override: false,
            });
        }
        // Sort the branch and worktree sections independently by the active column (stashes keep
        // their natural recency order). `None` leaves git's order (HEAD first).
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
        // Tabbed mode: keep only the active tab's rows (so selection / clicks / nav all scope to
        // it). Computed from the locked `page` to avoid re-locking via the public helpers. The
        // Commits tab has no PageRows (it's rendered separately), so it filters to empty.
        let present = u8::from(!page.branches.is_empty())
            + u8::from(!page.worktrees.is_empty())
            + u8::from(!page.stashes.is_empty())
            + u8::from(!page.commits.is_empty());
        if self.repo_page_tabs == RepoTabsMode::Auto && present >= 2 {
            match self.repo_page_tab.row_kind() {
                Some(kind) => rows.retain(|row| row.kind == kind),
                None => rows.clear(),
            }
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
        self.maximized != Some(Pane::RepoPage)
            && self.repo_page_tabs == RepoTabsMode::Auto
            && self.repo_page_present_tabs().len() >= 2
    }

    /// Switch the active repo-page tab, resetting the selection to its first row.
    pub fn repo_page_select_tab(&mut self, tab: RepoTab) {
        self.repo_page_tab = tab;
        self.repo_page_selected = 0;
        self.repo_page_scroll = 0;
    }

    /// Cycle to the next/previous present repo-page tab.
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
        }
    }

    /// Open the diff modal in a loading state for `source`.
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

}
