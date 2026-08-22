//! Persistence, caches, and toasts.

use super::*;

impl AppState {
    pub const TOAST_DURATION: Duration = Duration::from_millis(2500);

    /// Show a transient toast message (reusable anywhere — diff "no changes", view toggles…).
    pub fn show_toast(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast {
            message: message.into(),
            preview: Vec::new(),
            shown_at: Instant::now(),
        });
    }

    pub const COPY_PREVIEW_LINES: usize = 3;

    /// Confirm a clipboard copy: toast with the first few lines of what was copied.
    pub fn show_copy_toast(&mut self, copied: &str) {
        self.toast = Some(Toast {
            message: "copied to clipboard".into(),
            preview: copy_preview(copied),
            shown_at: Instant::now(),
        });
    }

    pub fn active_toast(&self) -> Option<&Toast> {
        self.toast
            .as_ref()
            .filter(|toast| toast.shown_at.elapsed() < Self::TOAST_DURATION)
    }

    #[cfg(test)]
    pub fn save_state(&self) {}

    #[cfg(not(test))]
    pub fn save_state(&self) {
        let mut collapsed_groups: Vec<String> = self.collapsed_groups.iter().cloned().collect();
        collapsed_groups.sort();
        let mut collapsed_folders: Vec<String> = self.collapsed_folders.iter().cloned().collect();
        collapsed_folders.sort();
        use crate::persist as p;
        p::save(&p::PersistedState {
            version: p::SCHEMA_VERSION,
            agent: p::AgentPrefs {
                claude_agent: self.claude_agent,
                claude_skip_permissions: self.claude_skip_permissions,
            },
            coverage: self.coverage_prefs.clone(),
            explorer: self.explorer_prefs,
            perf: p::PerfPrefs {
            placement: self.perf.placement,
            graph: self.perf.graph,
        },
        interaction: p::InteractionPrefs {
                hover_effects: self.hover_effects,
                changed_row_effect: self.changed_row_effect,
            },
            layout: p::LayoutPrefs {
                panel_padding: self.panel_padding,
                show_borders: self.show_borders,
                splitter_mode: self.splitter_mode,
                branch_check: self.branch_check,
                info_layout: self.info_layout,
                // Round the dragged ratios so the file doesn't carry f64 noise.
                split_ratio: p::round4(self.split_ratio),
                preview_split_ratio: p::round4(self.preview_split_ratio),
                dock_ratio: p::round4(self.dock_ratio),
                show_result_panel: self.show_result_panel,
                info_pinned: self.info_pinned,
            },
            lists: p::ListPrefs {
                grouping_enabled: self.grouping_enabled,
                tree_enabled: self.tree_enabled,
                hide_folder_lines: self.hide_folder_lines,
                sort_column: self.sort_column,
                sort_dir: self.sort_dir,
                favorites: {
                    let mut favorites: Vec<String> = self.favorites.iter().cloned().collect();
                    favorites.sort();
                    favorites
                },
                favorites_first: self.favorites_first,
                columns: self.columns,
                hide_zero_counts: self.hide_zero_counts,
            },
            pull_requests: p::PullRequestPrefs { show_merged_prs: self.show_merged_prs },
            repo_page: p::RepoPagePrefs {
                repo_page_columns: self.repo_page_columns,
                repo_page_stash_columns: self.repo_page_stash_columns,
                repo_page_info: self.repo_page_info,
                base_overrides: self.base_overrides.clone(),
                repo_page_tabs: self.repo_page_tabs,
                // Only the repo page's maximize is sticky; other panes' maximize is session-only.
                repo_page_maximized: self.maximized == Some(Pane::RepoPage),
                repo_page_maximized_tabbed: self.repo_page_maximized_tabbed,
                repo_page_collapsed_sections: {
                    let mut sections: Vec<String> =
                        self.repo_page_collapsed_sections.iter().cloned().collect();
                    sections.sort();
                    sections
                },
            },
            session: p::SessionState {
                last_seen_version: env!("CARGO_PKG_VERSION").to_string(),
                help_tab: self.help_tab_persist,
                collapsed_groups,
                collapsed_folders,
                collapsed_settings: {
                    let mut sections: Vec<String> = self.collapsed_settings.iter().cloned().collect();
                    sections.sort();
                    sections
                },
                settings_layout: self.settings_layout,
                design_layout: self.design_layout,
                cli_help_mode: self.cli_builder.help_mode,
                kebab_session_prefix: self.kebab_session_prefix,
            },
            sync: p::SyncPrefs {
                auto_pull_on_launch: self.auto_pull_on_launch,
                auto_pull_max_repos: self.auto_pull_max_repos,
                auto_pull_in_tree: self.auto_pull_in_tree,
                max_pull_mode: self.max_pull_mode,
                max_pull_exact: self.max_pull_exact,
                max_pull_percent: self.max_pull_percent,
            },
            theming: p::ThemingPrefs {
                icon_style: self.icon_style,
                theme: self.theme,
                background: Some(self.background),
                contrast: self.contrast,
                selection_style: self.selection_style,
                button_hover_style: self.button_hover_style,
            },
            tooltips: self.tooltips,
            updates: p::UpdatePrefs {
                auto_update: self.auto_update,
                update_interval: self.update_interval,
                last_update_check: self.last_update_check,
            },
            view: p::ViewPrefs {
                diff_view: self.diff_view,
                right_view: self.right_view,
                pane_diff_view: self.pane_diff_view,
            },
            workspaces: p::WorkspacePrefs {
                // Persist every saved workspace; refresh the active one from the live root set so
                // picker add/remove sticks. Ad-hoc (no active workspace) sessions touch nothing.
                workspaces: {
                    let mut workspaces = self.workspaces.clone();
                    if let Some(name) = &self.active_workspace {
                        workspaces.insert(
                            name.clone(),
                            self.root_dirs.iter().map(|root| root.display().to_string()).collect(),
                        );
                    }
                    workspaces
                },
                roots: Vec::new(), // legacy field — workspaces own the folder sets now
                folder_bookmarks: self.folder_bookmarks.clone(),
            },
        });
    }

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
}
