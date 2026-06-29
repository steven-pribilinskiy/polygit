use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::{
    Background, BranchCheck, ButtonHoverStyle, ChangedRowEffect, ClaudeAgent, ColumnFlags, Contrast,
    DesignLayout, HelpTab, IconStyle, RepoPageColumns, RepoPageStashColumns, RepoTabsMode,
    SelectionStyle, SettingsLayout,
    SortColumn, SortDir, SplitterMode, Theme, TooltipPrefs,
};

/// UI preferences persisted between runs at `~/.config/polygit/state.json`.
/// `#[serde(default)]` keeps older state files (missing newer fields) loadable.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedState {
    pub columns: ColumnFlags,
    /// The info block (`i`) was shown on last exit.
    pub info_pinned: bool,
    /// Left/right splitter position.
    pub split_ratio: f64,
    /// The result/log panel (bottom of the preview) was shown on last exit (defaults on).
    #[serde(default = "default_true")]
    pub show_result_panel: bool,
    /// Info/result split ratio inside the preview (info-panel fraction).
    pub preview_split_ratio: f64,
    /// 1-cell padding inside every bordered panel/modal (default on).
    #[serde(default = "default_true")]
    pub panel_padding: bool,
    /// Glyph set (Unicode vs emoji).
    pub icon_style: IconStyle,
    /// Hide zero-count cells in the list columns (emoji mode always hides them; this extends it to
    /// the Unicode set too).
    #[serde(default)]
    pub hide_zero_counts: bool,
    /// Color theme (auto / dark / light).
    pub theme: Theme,
    /// Contrast level (normal / soft) — text + accent saturation.
    pub contrast: Contrast,
    /// Selected-row highlight style (blue bar vs subtle tint).
    pub selection_style: SelectionStyle,
    /// Button hover style (reverse-video vs soft tint) for hints/tabs/chips/keys.
    #[serde(default)]
    pub button_hover_style: ButtonHoverStyle,
    /// Settings modal layout (tabbed / accordion / flat).
    pub settings_layout: SettingsLayout,
    /// Section names collapsed in the accordion settings layout.
    #[serde(default)]
    pub collapsed_settings: Vec<String>,
    /// Relative paths of repos marked as favorites.
    #[serde(default)]
    pub favorites: Vec<String>,
    /// Pin a "★ Favorites" section to the top of the list (default off).
    #[serde(default)]
    pub favorites_first: bool,
    /// Bookmarked folders (absolute paths) for the folder picker.
    #[serde(default)]
    pub folder_bookmarks: Vec<String>,
    /// Hide the dash-fill leader lines in group / folder headers (default off).
    #[serde(default)]
    pub hide_folder_lines: bool,
    /// Which AI coding-agent CLI the `c` hotkey launches (claude / codex / gemini).
    #[serde(default)]
    pub claude_agent: ClaudeAgent,
    /// Append the agent's "bypass all approval prompts" flag when launching (default off).
    #[serde(default)]
    pub claude_skip_permissions: bool,
    /// Legacy single workspace (absolute paths). Kept for migration only — on load it folds into
    /// `workspaces["default"]` when `workspaces` is empty. No longer written.
    #[serde(default)]
    pub roots: Vec<String>,
    /// Named workspaces: name → folders/roots (absolute paths). Opened with `-w <name>` or the
    /// `ws` picker; the folder picker (`A`) adds/removes roots in the active one. Default launch
    /// (no `-w`) uses the CLI dirs or the cwd, never a workspace.
    #[serde(default)]
    pub workspaces: HashMap<String, Vec<String>>,
    /// Background tone (normal / soft) — surface only. `None` in pre-split state files;
    /// `resolve_background` derives it from `contrast` for backward compatibility.
    pub background: Option<Background>,
    /// Repo-list sort column. Tolerant: the removed `"discovery"` value (and anything
    /// unknown) loads as the default (Name) without discarding the rest of the file.
    #[serde(default, deserialize_with = "sort_column_tolerant")]
    pub sort_column: SortColumn,
    /// Repo-list sort direction.
    pub sort_dir: SortDir,
    /// Last-active help-modal tab.
    pub help_tab: HelpTab,
    /// Grouped list view was on at last exit (default on).
    #[serde(default = "default_true")]
    pub grouping_enabled: bool,
    /// Names (or `folder::name` keys) of collapsed groups.
    pub collapsed_groups: Vec<String>,
    /// Directory-tree view was on at last exit.
    pub tree_enabled: bool,
    /// Relative paths of collapsed folders.
    pub collapsed_folders: Vec<String>,
    /// Repo-page branch columns (all on by default).
    pub repo_page_columns: RepoPageColumns,
    /// Which optional Stashes-tab columns are shown — age / stats (default both on).
    #[serde(default)]
    pub repo_page_stash_columns: RepoPageStashColumns,
    /// Repo-page bottom info panel shown (default on).
    #[serde(default = "default_true")]
    pub repo_page_info: bool,
    /// Per-repo+branch base-branch overrides, keyed `"{repo_abs_path}\u{1f}{branch}"` → base ref.
    /// When set, the repo page diffs that branch's stats against the chosen base instead of the
    /// auto-detected fork parent.
    pub base_overrides: HashMap<String, String>,
    /// Pull every repo automatically on launch (default on). When off, repos load from the
    /// status cache and pulling is a manual action (`e`/`E`).
    #[serde(default = "default_true")]
    pub auto_pull_on_launch: bool,
    /// Skip the launch auto-pull when more than this many repos are discovered. `0` = no limit.
    #[serde(default = "default_auto_pull_max")]
    pub auto_pull_max_repos: u32,
    /// Allow the launch auto-pull while the directory-tree view is active (default off — tree
    /// view suppresses auto-pull).
    pub auto_pull_in_tree: bool,
    /// Highlight actionable elements under the mouse cursor. Off by default: it enables all-motion
    /// mouse tracking, which takes over the terminal's own text selection / URL hover. Default on.
    #[serde(default = "default_true")]
    pub hover_effects: bool,
    /// Draw borders around the two main panes (default on). Off reclaims the border rows/cols.
    #[serde(default = "default_true")]
    pub show_borders: bool,
    /// How the pane splitters are presented: a dedicated 1-cell lane (default) or a thin grip shown
    /// only on hover (default). (Replaces the old `show_splitter` bool; unknown old keys ignored.)
    #[serde(default = "default_splitter_hover")]
    pub splitter_mode: SplitterMode,
    /// Post-change attention indicator on changed cells: off / flash / highlight (default flash).
    /// (Replaces the old `changed_row_flash` / `changed_row_highlight` bools; old keys are ignored.)
    #[serde(default)]
    pub changed_row_effect: ChangedRowEffect,
    /// Split the repo page into branches/worktrees/stashes tabs (off / auto). Default auto.
    #[serde(default = "default_tabs_auto")]
    pub repo_page_tabs: RepoTabsMode,
    /// Repo-page window state: maximized (full-screen) when true, restored (docked panel) when false.
    /// Default restored.
    #[serde(default)]
    pub repo_page_maximized: bool,
    /// In the maximized (full-screen) repo page, show the tabbed view (true) instead of the flat
    /// stacked single view (false). Toggled with `v`. Default flat (stacked).
    #[serde(default)]
    pub repo_page_maximized_tabbed: bool,
    /// Repo-page sections collapsed in the flat (stacked) view — by section name
    /// ("Branches"/"Worktrees"/"Stashes"/"Commits").
    #[serde(default)]
    pub repo_page_collapsed_sections: Vec<String>,
    /// Restored repo-panel height as a fraction of the main area (0 = use the default).
    #[serde(default)]
    pub dock_ratio: f64,
    /// Periodic local branch/status refresh (off / auto). Default off.
    #[serde(default)]
    pub branch_check: BranchCheck,
    /// Per-area tooltip enablement (master + footer/headers/counts/settings/links). All default on.
    #[serde(default)]
    pub tooltips: TooltipPrefs,
    /// Kebab-menu "wrap copied prompt in `cd <repo> && claude '…'`" checkbox (default off).
    #[serde(default)]
    pub kebab_session_prefix: bool,
    /// Help Design System tab layout (flat / tabbed). Default flat.
    #[serde(default)]
    pub design_layout: DesignLayout,
    /// The app version last run — drives the "What's New" modal after an update. Empty on first run.
    #[serde(default)]
    pub last_seen_version: String,
    /// CLI builder: when to show each flag's help text (always / on-hover / never). Default on-hover.
    #[serde(default)]
    pub cli_help_mode: crate::app::CliHelpMode,
    /// Diff modal render style (raw / unified / split). Default raw.
    #[serde(default)]
    pub diff_view: crate::app::DiffView,
    /// Result pane view on last exit: log vs diff (the flat log/raw/unified/split chip row).
    #[serde(default)]
    pub right_view: crate::app::RightView,
    /// Result pane diff render style (raw / unified / split). Default raw.
    #[serde(default)]
    pub pane_diff_view: crate::app::DiffView,
    /// Surface merged & closed PRs (not just open ones) in the PR column + info panel. Default off
    /// — detection always finds all states, this only gates display.
    #[serde(default)]
    pub show_merged_prs: bool,
}

impl PersistedState {
    /// The named workspaces, folding a legacy single `roots` list into `workspaces["default"]`
    /// when no named workspaces exist yet (so old state files keep their saved folder set).
    pub fn workspaces_migrated(&self) -> HashMap<String, Vec<String>> {
        if self.workspaces.is_empty() && !self.roots.is_empty() {
            HashMap::from([("default".to_string(), self.roots.clone())])
        } else {
            self.workspaces.clone()
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_auto_pull_max() -> u32 {
    100
}

fn default_splitter_hover() -> SplitterMode {
    SplitterMode::Hover
}

fn default_tabs_auto() -> RepoTabsMode {
    RepoTabsMode::Auto
}

/// Deserialize `sort_column` tolerantly: the removed `"discovery"` value and any unknown
/// string fall back to the default (`Name`) instead of failing the whole-file parse.
fn sort_column_tolerant<'de, D>(deserializer: D) -> Result<SortColumn, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Ok(match raw.as_str() {
        "branch" => SortColumn::Branch,
        "status" => SortColumn::Status,
        "ahead-behind" => SortColumn::AheadBehind,
        "dirty" => SortColumn::Dirty,
        "last-commit" => SortColumn::LastCommit,
        "worktrees" => SortColumn::Worktrees,
        "branches" => SortColumn::Branches,
        "stashes" => SortColumn::Stashes,
        // "name", removed "discovery", and anything unknown → default.
        _ => SortColumn::Name,
    })
}

/// Resolve the background tone for a loaded state file. Pre-split files carry only `contrast`,
/// where `Soft` meant a soft everything — so a missing background inherits the contrast level.
pub fn resolve_background(background: Option<Background>, contrast: Contrast) -> Background {
    background.unwrap_or(match contrast {
        Contrast::Soft => Background::Soft,
        Contrast::Normal => Background::Normal,
    })
}

/// The app's config directory (`~/.config/polygit`). On first call this migrates a legacy
/// `~/.config/pull-all` directory (renaming it) so existing state/groups/cache carry over the
/// rename. Shared by `persist`, `groups`, and `cache` so the migration happens exactly once.
pub fn config_dir() -> Option<PathBuf> {
    use std::sync::Once;
    static MIGRATE: Once = Once::new();
    let base = dirs::config_dir()?;
    let new_dir = base.join("polygit");
    MIGRATE.call_once(|| {
        let legacy = base.join("pull-all");
        if legacy.is_dir() && !new_dir.exists() {
            let _ = std::fs::rename(&legacy, &new_dir);
        }
    });
    Some(new_dir)
}

pub fn state_path() -> Option<PathBuf> {
    Some(config_dir()?.join("state.json"))
}

/// Load persisted UI state. A missing/corrupt file deserializes from `{}` so every field's
/// serde default applies (notably the `default = "default_true"` ones), unlike the derived
/// `Default` which would zero booleans like `repo_page_info`.
pub fn load() -> PersistedState {
    let contents = state_path().and_then(|path| std::fs::read_to_string(&path).ok());
    let raw = contents.as_deref().unwrap_or("{}");
    serde_json::from_str(raw)
        .or_else(|_| serde_json::from_str("{}"))
        .expect("empty object deserializes with serde defaults")
}

/// Persist UI state, best-effort (errors are ignored).
/// (Unused in the test profile — `AppState::save_state` is stubbed out there.)
#[cfg_attr(test, allow(dead_code))]
pub fn save(state: &PersistedState) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, contents);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_inherits_soft_contrast_when_absent() {
        assert_eq!(resolve_background(None, Contrast::Soft), Background::Soft);
        assert_eq!(resolve_background(None, Contrast::Normal), Background::Normal);
    }

    #[test]
    fn explicit_background_wins_over_contrast() {
        assert_eq!(resolve_background(Some(Background::Normal), Contrast::Soft), Background::Normal);
        assert_eq!(resolve_background(Some(Background::Soft), Contrast::Normal), Background::Soft);
    }

    #[test]
    fn legacy_roots_migrate_into_default_workspace() {
        // An old file with a single `roots` list and no `workspaces` → workspaces["default"].
        let json = r#"{"roots":["/a","/b"]}"#;
        let state: PersistedState = serde_json::from_str(json).unwrap();
        let workspaces = state.workspaces_migrated();
        assert_eq!(workspaces.get("default"), Some(&vec!["/a".to_string(), "/b".to_string()]));
        // Named workspaces present → legacy roots are ignored (no migration).
        let json = r#"{"roots":["/legacy"],"workspaces":{"work":["/x"]}}"#;
        let state: PersistedState = serde_json::from_str(json).unwrap();
        let workspaces = state.workspaces_migrated();
        assert_eq!(workspaces.get("work"), Some(&vec!["/x".to_string()]));
        assert!(!workspaces.contains_key("default"));
        // Neither present → empty.
        assert!(PersistedState::default().workspaces_migrated().is_empty());
    }

    #[test]
    fn old_state_without_background_loads() {
        // A pre-split state file has no `background` key; serde(default) → None.
        let json = r#"{"contrast":"soft","theme":"dark"}"#;
        let state: PersistedState = serde_json::from_str(json).unwrap();
        assert_eq!(state.background, None);
        assert_eq!(resolve_background(state.background, state.contrast), Background::Soft);
    }

    #[test]
    fn removed_discovery_sort_loads_as_name_without_losing_other_fields() {
        // An old file with the removed "discovery" sort must not reset the whole file.
        let json = r#"{"sort_column":"discovery","panel_padding":true,"grouping_enabled":true}"#;
        let state: PersistedState = serde_json::from_str(json).unwrap();
        assert_eq!(state.sort_column, SortColumn::Name);
        assert!(state.panel_padding);
        assert!(state.grouping_enabled);
    }

    #[test]
    fn base_overrides_default_empty_and_round_trip() {
        // An old file without the key loads with an empty override map (no panic, no reset).
        let old: PersistedState = serde_json::from_str(r#"{"panel_padding":true}"#).unwrap();
        assert!(old.base_overrides.is_empty());
        assert!(old.panel_padding);
        // A set override round-trips through serialize → deserialize.
        let mut state = PersistedState::default();
        state.base_overrides.insert("/repo\u{1f}feature".to_string(), "origin/stage".to_string());
        let json = serde_json::to_string(&state).unwrap();
        let back: PersistedState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.base_overrides.get("/repo\u{1f}feature").map(String::as_str), Some("origin/stage"));
    }

    #[test]
    fn sort_column_tolerant_maps_known_and_unknown() {
        let cases = [
            (r#"{"sort_column":"branch"}"#, SortColumn::Branch),
            (r#"{"sort_column":"ahead-behind"}"#, SortColumn::AheadBehind),
            (r#"{"sort_column":"stashes"}"#, SortColumn::Stashes),
            (r#"{"sort_column":"garbage"}"#, SortColumn::Name),
        ];
        for (json, expected) in cases {
            let state: PersistedState = serde_json::from_str(json).unwrap();
            assert_eq!(state.sort_column, expected, "for {json}");
        }
    }
}
