//! Lifecycle for the smaller modals: changelog, pin picker, help, build info, PR, kebab, branch/base pickers, copy menu, explorer.

use super::*;

impl AppState {
    pub const COPY_MENU_ROWS: usize = 3;

    /// The text to copy for the current `y`-menu selection over `row` (path / branch / both).
    pub fn copy_menu_text(&self, row: &PageRow) -> String {
        match self.copy_menu.unwrap_or(0) {
            1 => row.branch.clone(),
            2 => format!("{} {}", row.path.display(), row.branch),
            _ => row.path.display().to_string(),
        }
    }

    pub fn help_link_at(&self, row: u16) -> Option<String> {
        self.help_links
            .iter()
            .find(|(link_row, _)| *link_row == row)
            .map(|(_, url)| url.clone())
    }

    pub fn help_tab_at(&self, col: u16, row: u16) -> Option<HelpTab> {
        self.help_tab_click
            .iter()
            .find(|(chip_row, start, end, _)| *chip_row == row && col >= *start && col < *end)
            .map(|(_, _, _, tab)| *tab)
    }

    pub fn help_close_at(&self, col: u16, row: u16) -> bool {
        region_hit(self.help_close_click, col, row)
    }

    pub fn copy_menu_option_at(&self, row: u16) -> Option<usize> {
        self.copy_menu_click
            .iter()
            .find(|(click_row, _)| *click_row == row)
            .map(|(_, index)| *index)
    }

    pub fn base_cell_at(&self, col: u16, row: u16) -> Option<usize> {
        self.base_cell_click
            .iter()
            .find(|(click_row, start, end, _)| *click_row == row && col >= *start && col < *end)
            .map(|(_, _, _, index)| *index)
    }

    pub fn base_picker_option_at(&self, row: u16) -> Option<usize> {
        self.base_picker_click
            .iter()
            .find(|(click_row, _)| *click_row == row)
            .map(|(_, index)| *index)
    }

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

    pub fn confirm_base_picker(&mut self) -> Option<(usize, String)> {
        let picker = self.base_picker.take()?;
        let chosen = picker.ref_at(picker.selected);
        self.set_base_override(picker.repo_index, &picker.branch, chosen);
        Some((picker.repo_index, picker.branch))
    }

    pub fn move_base_picker(&mut self, delta: isize) {
        if let Some(picker) = self.base_picker.as_mut() {
            let last = picker.row_count().saturating_sub(1);
            let next = (picker.selected as isize).saturating_add(delta).clamp(0, last as isize);
            picker.selected = next as usize;
        }
    }

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
            || self.branch_filter_modal.is_some()
            || self.show_changelog
            || self.explorer.is_some()
    }

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
        self.branch_filter_modal = None;
        self.dropdown = None;
        self.finder = None;
        self.picker = None;
        self.show_changelog = false;
        self.changelog_pin_mode = false;
        self.pin_show_all = false;
    }

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

    pub fn pin_visible_indices(&self) -> Vec<usize> {
        self.pin_releases
            .iter()
            .enumerate()
            .filter(|(_, release)| self.pin_show_all || release.is_supported)
            .map(|(idx, _)| idx)
            .collect()
    }

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

    pub fn toggle_changelog_release(&mut self, version: &str) {
        if !self.changelog_collapsed.remove(version) {
            self.changelog_collapsed.insert(version.to_string());
        }
    }

    pub fn set_help_tab(&mut self, tab: HelpTab) {
        self.help_tab = tab;
        if tab != HelpTab::About {
            self.help_tab_persist = tab;
        }
    }

    pub fn open_help(&mut self) {
        self.close_all_modals();
        self.show_help = true;
        self.help_scroll = 0;
    }

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

    pub fn build_info_tree_rows(&self) -> Vec<crate::treeview::TreeRow> {
        self.build_info_tree
            .as_ref()
            .map(|tree| crate::treeview::flatten(tree, &self.build_info_tree_expanded))
            .unwrap_or_default()
    }

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

    pub fn build_info_tree_move(&mut self, delta: isize) {
        let len = self.build_info_tree_rows().len();
        if len == 0 {
            return;
        }
        let next = (self.build_info_tree_selected as isize).saturating_add(delta).clamp(0, len as isize - 1);
        self.build_info_tree_selected = next as usize;
    }

    pub fn build_info_tree_expand(&mut self) {
        let rows = self.build_info_tree_rows();
        if let Some(row) = rows.get(self.build_info_tree_selected) {
            if let crate::treeview::RowKind::Container { collapsed: true, .. } = row.kind {
                self.build_info_tree_expanded.insert(row.path.clone());
            }
        }
    }

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

    pub fn pr_modal_select_tab(&mut self, tab: crate::app::PrModalTab) {
        if let Some(modal) = self.pr_modal.as_mut() {
            modal.tab = tab;
            modal.scroll = 0;
            modal.search_focused = false;
        }
    }

    pub fn pr_modal_cycle_tab(&mut self, forward: bool) {
        if let Some(modal) = self.pr_modal.as_ref() {
            let next = modal.tab.cycle(forward);
            self.pr_modal_select_tab(next);
        }
    }

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
        prompt.push_str(&format!("- Local branches beyond the mainline (excl. main/master/dev/…): {branches}\n"));
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
        let mut items = Vec::new();
        // Merged/gone-upstream repos lead with the actionable suggestion: one run-item per
        // deduped candidate branch (top-most first), then a copy-the-command item. `S` runs the
        // top candidate, so hint it on the first row.
        let switch_targets = state.switch_targets();
        for (rank, base) in switch_targets.iter().enumerate() {
            // Fully-merged branches get a "& delete <branch>" in the label + command.
            let delete = state.switch_delete_branch(base);
            items.push(KebabItem {
                label: format!("⎇ {}", crate::app::switch_title(base, delete.as_deref())),
                action: KebabAction::SwitchBase,
                enabled: true,
                hint: if rank == 0 { Some("S".to_string()) } else { None },
                data: Some(base.clone()),
            });
        }
        if let Some(top) = switch_targets.first() {
            let delete = state.switch_delete_branch(top);
            items.push(KebabItem {
                label: "⧉ Copy switch command".to_string(),
                action: KebabAction::CopySwitchCommand,
                enabled: true,
                hint: None,
                data: Some(crate::app::switch_command(top, delete.as_deref())),
            });
        }
        items.extend([
            KebabItem {
                label: if favorited { "★ Unfavorite".to_string() } else { "☆ Favorite".to_string() },
                action: KebabAction::ToggleFavorite,
                enabled: true,
                hint: Some("b".to_string()),
                data: None,
            },
            KebabItem {
                label: "Checkout branch…".to_string(),
                action: KebabAction::Checkout,
                enabled: true,
                hint: None,
                data: None,
            },
            KebabItem {
                label: "Copy cleanup prompt".to_string(),
                action: KebabAction::CopyCleanupPrompt,
                enabled: true,
                hint: None,
                data: None,
            },
            KebabItem {
                label: format!("{checkbox} include `cd … && {agent} '…'`"),
                action: KebabAction::ToggleSessionPrefix,
                enabled: true,
                hint: None,
                data: None,
            },
            KebabItem {
                label: format!("Run {agent}"),
                action: KebabAction::Claude,
                enabled: true,
                hint: Some("c".to_string()),
                data: None,
            },
            KebabItem {
                label: "Explore files…".to_string(),
                action: KebabAction::Explore,
                enabled: true,
                hint: Some("^E".to_string()),
                data: None,
            },
            KebabItem {
                label: "Open lazygit".to_string(),
                action: KebabAction::Lazygit,
                enabled: true,
                hint: Some("l".to_string()),
                data: None,
            },
            KebabItem {
                label: "View diff".to_string(),
                action: KebabAction::Diff,
                enabled: dirty > 0,
                hint: Some("d".to_string()),
                data: None,
            },
            KebabItem {
                label: "Refetch".to_string(),
                action: KebabAction::Refetch,
                enabled: true,
                hint: Some("e".to_string()),
                data: None,
            },
            KebabItem {
                label: "Open remote".to_string(),
                action: KebabAction::OpenRemote,
                enabled: has_remote,
                hint: Some("o".to_string()),
                data: None,
            },
        ]);
        items
    }

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

    pub fn open_explorer(&mut self, repo_idx: usize) {
        let Some(repo) = self.repos.get(repo_idx) else {
            return;
        };
        let root = repo.lock().unwrap().path.clone();
        self.close_all_modals();
        self.explorer = Some(crate::explorer::Explorer::open(root, self.explorer_prefs));
    }

    pub fn open_explorer_selected(&mut self) {
        if let Some(idx) = self.selected_repo_index() {
            self.open_explorer(idx);
        }
    }

    pub fn close_explorer(&mut self) {
        self.explorer = None;
    }

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

    pub fn set_explorer_sort(&mut self, key: crate::explorer::SortKey) {
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.set_sort(key);
            self.explorer_prefs.sort = explorer.sort;
            self.explorer_prefs.sort_ascending = explorer.sort_ascending;
            self.save_state();
        }
    }

    pub fn toggle_explorer_pin(&mut self) {
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.toggle_pin();
            self.explorer_prefs.mode = explorer.mode;
            self.save_state();
        }
    }

    pub fn toggle_explorer_tree_mode(&mut self) {
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.toggle_tree_mode();
            self.explorer_prefs.tree_mode = explorer.tree_mode;
            self.save_state();
        }
    }

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

    pub fn toggle_explorer_gitignored(&mut self) {
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.toggle_show_gitignored();
            self.explorer_prefs.show_gitignored = explorer.show_gitignored;
            self.save_state();
        }
    }

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

    pub fn open_branch_picker(&mut self, repo_idx: usize) {
        self.branch_picker =
            Some(BranchPicker { repo_idx, branches: Vec::new(), filter: String::new(), selected: 0, loading: true });
    }

    pub fn close_branch_picker(&mut self) {
        self.branch_picker = None;
    }

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

    pub fn open_branch_filter(&mut self) {
        self.branch_filter_modal = Some(BranchFilterModal {
            mode: self.branch_filter_mode,
            query: String::new(),
            selected: 0,
            scroll: 0,
        });
    }

    pub fn close_branch_filter(&mut self) {
        self.branch_filter_modal = None;
    }

    /// Row count for the open branch-filter modal: the synthetic "clear filter" row (only when a
    /// filter is currently applied) plus one row per matching branch name.
    fn branch_filter_row_count(&self) -> usize {
        usize::from(self.branch_filter.is_some()) + self.branch_filter_modal_rows().len()
    }

    pub fn branch_filter_move(&mut self, delta: isize) {
        let len = self.branch_filter_row_count();
        if len == 0 {
            if let Some(modal) = self.branch_filter_modal.as_mut() {
                modal.selected = 0;
            }
            return;
        }
        let last = (len - 1) as isize;
        if let Some(modal) = self.branch_filter_modal.as_mut() {
            modal.selected = (modal.selected as isize + delta).clamp(0, last) as usize;
        }
    }

    /// Cycle the modal's active/local/remote/any mode (`Tab`/`Shift-Tab`); resets the selection
    /// since the aggregate list can reorder/resize when the mode changes.
    pub fn branch_filter_cycle_mode(&mut self, forward: bool) {
        let Some(modal) = self.branch_filter_modal.as_mut() else {
            return;
        };
        modal.mode = if forward { modal.mode.next() } else { modal.mode.prev() };
        modal.selected = 0;
        modal.scroll = 0;
    }

    /// Apply the modal's highlighted row: row 0 (when present) clears the filter; otherwise picks
    /// that branch name with the modal's current mode. Closes the modal either way.
    pub fn branch_filter_apply_selected(&mut self) {
        let Some(modal) = self.branch_filter_modal.clone() else {
            return;
        };
        let clear_row = self.branch_filter.is_some();
        if clear_row && modal.selected == 0 {
            self.branch_filter = None;
        } else {
            let index = if clear_row { modal.selected - 1 } else { modal.selected };
            if let Some((name, _count)) = self.branch_filter_modal_rows().get(index) {
                self.branch_filter = Some(name.clone());
                self.branch_filter_mode = modal.mode;
            }
        }
        self.close_branch_filter();
    }
}
