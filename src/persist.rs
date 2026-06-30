use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::{
    AutoUpdate, Background, BranchCheck, ButtonHoverStyle, ChangedRowEffect, ClaudeAgent,
    CliHelpMode, ColumnFlags, Contrast, DesignLayout, DiffView, HelpTab, IconStyle, InfoLayout,
    RepoPageColumns, RepoPageStashColumns, RepoTabsMode, RightView, SelectionStyle, SettingsLayout,
    SortColumn, SortDir, SplitterMode, Theme, TooltipPrefs, UpdateInterval,
};

/// `state.json` schema version. Files carrying this key load via the nested path; files without it
/// (or `0`) are the legacy FLAT schema and migrate through `LegacyFlatState` → `From`. Bump only
/// when the nested *shape* changes again.
pub const SCHEMA_VERSION: u32 = 1;

/// UI preferences persisted between runs at `~/.config/polygit/state.json`, grouped into logical
/// sections (mirroring the Settings UI plus a few non-settings groups). `#[serde(default)]` on every
/// section + sub-struct keeps partial / future files loadable; a whole missing section defaults from
/// the sub-struct's `Default`. Old FLAT files (no `version`) migrate once via `LegacyFlatState`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedState {
    /// Schema version (see `SCHEMA_VERSION`). Present ⇒ nested; absent/0 ⇒ legacy flat.
    pub version: u32,
    pub agent: AgentPrefs,
    /// File-explorer preferences (columns, sort, date format).
    pub explorer: crate::explorer::ExplorerPrefs,
    pub interaction: InteractionPrefs,
    pub layout: LayoutPrefs,
    pub lists: ListPrefs,
    pub pull_requests: PullRequestPrefs,
    pub repo_page: RepoPagePrefs,
    pub session: SessionState,
    pub sync: SyncPrefs,
    pub theming: ThemingPrefs,
    pub tooltips: TooltipPrefs,
    pub updates: UpdatePrefs,
    pub view: ViewPrefs,
    pub workspaces: WorkspacePrefs,
}

/// AI coding-agent launch (the `c` hotkey).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentPrefs {
    /// Which agent CLI `c` launches (claude / codex / gemini).
    pub claude_agent: ClaudeAgent,
    /// Append the agent's "bypass all approval prompts" flag when launching.
    pub claude_skip_permissions: bool,
}

/// Mouse / attention interaction.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct InteractionPrefs {
    /// Highlight actionable elements under the mouse (enables all-motion tracking). Default on.
    pub hover_effects: bool,
    /// Post-change attention indicator on changed cells: off / flash / highlight.
    pub changed_row_effect: ChangedRowEffect,
}

impl Default for InteractionPrefs {
    fn default() -> Self {
        InteractionPrefs { hover_effects: true, changed_row_effect: ChangedRowEffect::default() }
    }
}

/// Pane geometry + layout chrome (split ratios, borders, padding, splitter, info/result panels).
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutPrefs {
    /// 1-cell padding inside every bordered panel/modal. Default on.
    pub panel_padding: bool,
    /// Draw borders around the two main panes. Default on.
    pub show_borders: bool,
    /// How the pane splitters are presented (dedicated lane vs hover grip). Default hover.
    pub splitter_mode: SplitterMode,
    /// Periodic local branch/status refresh (off / auto). Default off.
    pub branch_check: BranchCheck,
    /// Info panel grouping layout (titled / spaced / flat). Default titled.
    pub info_layout: InfoLayout,
    /// Left/right splitter position (0 ⇒ use the default; clamped on load).
    pub split_ratio: f64,
    /// Info/result split ratio inside the preview (info-panel fraction).
    pub preview_split_ratio: f64,
    /// Restored repo-panel height as a fraction of the main area (0 ⇒ default).
    pub dock_ratio: f64,
    /// The result/log panel (bottom of the preview) was shown on last exit. Default on.
    pub show_result_panel: bool,
    /// The info block (`i`) was shown on last exit.
    pub info_pinned: bool,
}

impl Default for LayoutPrefs {
    fn default() -> Self {
        LayoutPrefs {
            panel_padding: true,
            show_borders: true,
            splitter_mode: SplitterMode::Hover,
            branch_check: BranchCheck::default(),
            info_layout: InfoLayout::default(),
            split_ratio: 0.0,
            preview_split_ratio: 0.0,
            dock_ratio: 0.0,
            show_result_panel: true,
            info_pinned: false,
        }
    }
}

/// Repo-list display: grouping/tree, columns, sort, favorites.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ListPrefs {
    /// Grouped list view was on at last exit. Default on.
    pub grouping_enabled: bool,
    /// Directory-tree view was on at last exit.
    pub tree_enabled: bool,
    /// Hide the dash-fill leader lines in group / folder headers.
    pub hide_folder_lines: bool,
    /// Repo-list sort column. Tolerant: removed `"discovery"` (and unknown) → `Name`.
    #[serde(deserialize_with = "sort_column_tolerant")]
    pub sort_column: SortColumn,
    /// Repo-list sort direction.
    pub sort_dir: SortDir,
    /// Relative paths of repos marked as favorites.
    pub favorites: Vec<String>,
    /// Pin a "★ Favorites" section to the top of the list.
    pub favorites_first: bool,
    /// Which list columns are shown.
    pub columns: ColumnFlags,
    /// Hide zero-count cells in the list columns (emoji mode always hides them).
    pub hide_zero_counts: bool,
}

impl Default for ListPrefs {
    fn default() -> Self {
        ListPrefs {
            grouping_enabled: true,
            tree_enabled: false,
            hide_folder_lines: false,
            sort_column: SortColumn::default(),
            sort_dir: SortDir::default(),
            favorites: Vec::new(),
            favorites_first: false,
            columns: ColumnFlags::default(),
            hide_zero_counts: false,
        }
    }
}

/// Pull-request display.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PullRequestPrefs {
    /// Surface merged & closed PRs (not just open) in the PR column + info panel.
    pub show_merged_prs: bool,
}

/// Auto-pull-on-launch policy.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncPrefs {
    /// Pull every repo automatically on launch. Default on.
    pub auto_pull_on_launch: bool,
    /// Skip the launch auto-pull above this many repos (`0` = no limit). Default 100.
    pub auto_pull_max_repos: u32,
    /// Allow the launch auto-pull while the directory-tree view is active.
    pub auto_pull_in_tree: bool,
}

impl Default for SyncPrefs {
    fn default() -> Self {
        SyncPrefs { auto_pull_on_launch: true, auto_pull_max_repos: 100, auto_pull_in_tree: false }
    }
}

/// Colors + glyphs.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemingPrefs {
    /// Glyph set (Unicode vs emoji).
    pub icon_style: IconStyle,
    /// Color theme (auto / dark / light).
    pub theme: Theme,
    /// Background tone (normal / soft). `None` in pre-split files; `resolve_background` derives it.
    pub background: Option<Background>,
    /// Contrast level (normal / soft).
    pub contrast: Contrast,
    /// Selected-row highlight style (blue bar vs subtle tint).
    pub selection_style: SelectionStyle,
    /// Button hover style (reverse-video vs soft tint).
    pub button_hover_style: ButtonHoverStyle,
}

/// Self-update from published GitHub releases (distinct from the local new-build watcher).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatePrefs {
    /// Update policy for published releases (off / notify / install).
    pub auto_update: AutoUpdate,
    /// How often the self-update check polls GitHub (daily / weekly).
    pub update_interval: UpdateInterval,
    /// Unix seconds of the last release check (cadence spans launches).
    pub last_update_check: i64,
}

/// Saved workspaces + folder bookmarks.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePrefs {
    /// Named workspaces: name → folders/roots (absolute paths).
    pub workspaces: HashMap<String, Vec<String>>,
    /// Legacy single workspace — folded into `workspaces["default"]` by `migrated()` when
    /// `workspaces` is empty. No longer written.
    pub roots: Vec<String>,
    /// Bookmarked folders (absolute paths) for the folder picker.
    pub folder_bookmarks: Vec<String>,
}

impl WorkspacePrefs {
    /// The named workspaces, folding a legacy single `roots` list into `workspaces["default"]`
    /// when no named workspaces exist yet (so old state files keep their saved folder set).
    pub fn migrated(&self) -> HashMap<String, Vec<String>> {
        if self.workspaces.is_empty() && !self.roots.is_empty() {
            HashMap::from([("default".to_string(), self.roots.clone())])
        } else {
            self.workspaces.clone()
        }
    }
}

/// Repo-page columns + view state.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoPagePrefs {
    /// Branch columns (all on by default).
    pub repo_page_columns: RepoPageColumns,
    /// Optional Stashes-tab columns — age / stats (default both on).
    pub repo_page_stash_columns: RepoPageStashColumns,
    /// Bottom info panel shown. Default on.
    pub repo_page_info: bool,
    /// Per-repo+branch base-branch overrides, keyed `"{repo_abs_path}\u{1f}{branch}"` → base ref.
    pub base_overrides: HashMap<String, String>,
    /// Split into branches/worktrees/stashes tabs (off / auto). Default auto.
    pub repo_page_tabs: RepoTabsMode,
    /// Window state: maximized (full-screen) vs restored (docked panel). Default restored.
    pub repo_page_maximized: bool,
    /// In the maximized page, show the tabbed view instead of the flat stacked single view.
    pub repo_page_maximized_tabbed: bool,
    /// Sections collapsed in the flat (stacked) view, by section name.
    pub repo_page_collapsed_sections: Vec<String>,
}

impl Default for RepoPagePrefs {
    fn default() -> Self {
        RepoPagePrefs {
            repo_page_columns: RepoPageColumns::default(),
            repo_page_stash_columns: RepoPageStashColumns::default(),
            repo_page_info: true,
            base_overrides: HashMap::new(),
            repo_page_tabs: RepoTabsMode::Auto,
            repo_page_maximized: false,
            repo_page_maximized_tabbed: false,
            repo_page_collapsed_sections: Vec::new(),
        }
    }
}

/// Result-pane / diff view state.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewPrefs {
    /// Diff modal render style (raw / unified / split). Default raw.
    pub diff_view: DiffView,
    /// Result pane view on last exit: log vs diff.
    pub right_view: RightView,
    /// Result pane diff render style (raw / unified / split). Default raw.
    pub pane_diff_view: DiffView,
}

/// Transient / chrome state that isn't a Settings toggle (last-seen version, active tabs, collapsed
/// sets, UI-layout prefs).
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    /// The app version last run — drives the "What's New" modal after an update.
    pub last_seen_version: String,
    /// Last-active help-modal tab.
    pub help_tab: HelpTab,
    /// Names (or `folder::name` keys) of collapsed groups.
    pub collapsed_groups: Vec<String>,
    /// Relative paths of collapsed folders.
    pub collapsed_folders: Vec<String>,
    /// Section names collapsed in the accordion settings layout.
    pub collapsed_settings: Vec<String>,
    /// Settings modal layout (tabbed / accordion / flat).
    pub settings_layout: SettingsLayout,
    /// Help Design System tab layout (flat / tabbed).
    pub design_layout: DesignLayout,
    /// CLI builder: when to show each flag's help text (always / on-hover / never).
    pub cli_help_mode: CliHelpMode,
    /// Kebab-menu "wrap copied prompt in `cd <repo> && claude '…'`" checkbox.
    pub kebab_session_prefix: bool,
}

/// The legacy FLAT `state.json` schema (every release before the nested v1 schema). Read-only:
/// deserialized from an old file, then remapped into the nested `PersistedState` via `From`. Keeps
/// all the original per-field defaults + tolerant deserializers so old files load identically.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct LegacyFlatState {
    pub columns: ColumnFlags,
    pub info_pinned: bool,
    pub split_ratio: f64,
    #[serde(default = "default_true")]
    pub show_result_panel: bool,
    pub preview_split_ratio: f64,
    #[serde(default = "default_true")]
    pub panel_padding: bool,
    pub icon_style: IconStyle,
    #[serde(default)]
    pub hide_zero_counts: bool,
    pub theme: Theme,
    pub contrast: Contrast,
    pub selection_style: SelectionStyle,
    #[serde(default)]
    pub button_hover_style: ButtonHoverStyle,
    pub settings_layout: SettingsLayout,
    #[serde(default)]
    pub collapsed_settings: Vec<String>,
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub favorites_first: bool,
    #[serde(default)]
    pub folder_bookmarks: Vec<String>,
    #[serde(default)]
    pub hide_folder_lines: bool,
    #[serde(default)]
    pub claude_agent: ClaudeAgent,
    #[serde(default)]
    pub claude_skip_permissions: bool,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub workspaces: HashMap<String, Vec<String>>,
    pub background: Option<Background>,
    #[serde(default, deserialize_with = "sort_column_tolerant")]
    pub sort_column: SortColumn,
    pub sort_dir: SortDir,
    pub help_tab: HelpTab,
    #[serde(default = "default_true")]
    pub grouping_enabled: bool,
    pub collapsed_groups: Vec<String>,
    pub tree_enabled: bool,
    pub collapsed_folders: Vec<String>,
    pub repo_page_columns: RepoPageColumns,
    #[serde(default)]
    pub repo_page_stash_columns: RepoPageStashColumns,
    #[serde(default = "default_true")]
    pub repo_page_info: bool,
    pub base_overrides: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub auto_pull_on_launch: bool,
    #[serde(default = "default_auto_pull_max")]
    pub auto_pull_max_repos: u32,
    pub auto_pull_in_tree: bool,
    #[serde(default = "default_true")]
    pub hover_effects: bool,
    #[serde(default = "default_true")]
    pub show_borders: bool,
    #[serde(default = "default_splitter_hover")]
    pub splitter_mode: SplitterMode,
    #[serde(default)]
    pub changed_row_effect: ChangedRowEffect,
    #[serde(default = "default_tabs_auto")]
    pub repo_page_tabs: RepoTabsMode,
    #[serde(default)]
    pub repo_page_maximized: bool,
    #[serde(default)]
    pub repo_page_maximized_tabbed: bool,
    #[serde(default)]
    pub repo_page_collapsed_sections: Vec<String>,
    #[serde(default)]
    pub dock_ratio: f64,
    #[serde(default)]
    pub branch_check: BranchCheck,
    #[serde(default)]
    pub tooltips: TooltipPrefs,
    #[serde(default)]
    pub kebab_session_prefix: bool,
    #[serde(default)]
    pub design_layout: DesignLayout,
    #[serde(default)]
    pub last_seen_version: String,
    #[serde(default)]
    pub cli_help_mode: CliHelpMode,
    #[serde(default)]
    pub diff_view: DiffView,
    #[serde(default)]
    pub right_view: RightView,
    #[serde(default)]
    pub pane_diff_view: DiffView,
    #[serde(default)]
    pub info_layout: InfoLayout,
    #[serde(default)]
    pub show_merged_prs: bool,
    #[serde(default)]
    pub auto_update: AutoUpdate,
    #[serde(default)]
    pub update_interval: UpdateInterval,
    #[serde(default)]
    pub last_update_check: i64,
}

impl From<LegacyFlatState> for PersistedState {
    /// Remap a deserialized legacy flat file into the nested schema. Reads the legacy struct's
    /// already-defaulted values (so the `default_true` fields keep their true defaults), and
    /// preserves BOTH `roots` and `workspaces` so the read-time `migrated()` fold is unchanged.
    fn from(legacy: LegacyFlatState) -> Self {
        PersistedState {
            version: SCHEMA_VERSION,
            agent: AgentPrefs {
                claude_agent: legacy.claude_agent,
                claude_skip_permissions: legacy.claude_skip_permissions,
            },
            // New in v3 — no legacy equivalent (columns off, name-ascending, relative dates).
            explorer: crate::explorer::ExplorerPrefs::default(),
            interaction: InteractionPrefs {
                hover_effects: legacy.hover_effects,
                changed_row_effect: legacy.changed_row_effect,
            },
            layout: LayoutPrefs {
                panel_padding: legacy.panel_padding,
                show_borders: legacy.show_borders,
                splitter_mode: legacy.splitter_mode,
                branch_check: legacy.branch_check,
                info_layout: legacy.info_layout,
                split_ratio: legacy.split_ratio,
                preview_split_ratio: legacy.preview_split_ratio,
                dock_ratio: legacy.dock_ratio,
                show_result_panel: legacy.show_result_panel,
                info_pinned: legacy.info_pinned,
            },
            lists: ListPrefs {
                grouping_enabled: legacy.grouping_enabled,
                tree_enabled: legacy.tree_enabled,
                hide_folder_lines: legacy.hide_folder_lines,
                sort_column: legacy.sort_column,
                sort_dir: legacy.sort_dir,
                favorites: legacy.favorites,
                favorites_first: legacy.favorites_first,
                columns: legacy.columns,
                hide_zero_counts: legacy.hide_zero_counts,
            },
            pull_requests: PullRequestPrefs { show_merged_prs: legacy.show_merged_prs },
            repo_page: RepoPagePrefs {
                repo_page_columns: legacy.repo_page_columns,
                repo_page_stash_columns: legacy.repo_page_stash_columns,
                repo_page_info: legacy.repo_page_info,
                base_overrides: legacy.base_overrides,
                repo_page_tabs: legacy.repo_page_tabs,
                repo_page_maximized: legacy.repo_page_maximized,
                repo_page_maximized_tabbed: legacy.repo_page_maximized_tabbed,
                repo_page_collapsed_sections: legacy.repo_page_collapsed_sections,
            },
            session: SessionState {
                last_seen_version: legacy.last_seen_version,
                help_tab: legacy.help_tab,
                collapsed_groups: legacy.collapsed_groups,
                collapsed_folders: legacy.collapsed_folders,
                collapsed_settings: legacy.collapsed_settings,
                settings_layout: legacy.settings_layout,
                design_layout: legacy.design_layout,
                cli_help_mode: legacy.cli_help_mode,
                kebab_session_prefix: legacy.kebab_session_prefix,
            },
            sync: SyncPrefs {
                auto_pull_on_launch: legacy.auto_pull_on_launch,
                auto_pull_max_repos: legacy.auto_pull_max_repos,
                auto_pull_in_tree: legacy.auto_pull_in_tree,
            },
            theming: ThemingPrefs {
                icon_style: legacy.icon_style,
                theme: legacy.theme,
                background: legacy.background,
                contrast: legacy.contrast,
                selection_style: legacy.selection_style,
                button_hover_style: legacy.button_hover_style,
            },
            tooltips: legacy.tooltips,
            updates: UpdatePrefs {
                auto_update: legacy.auto_update,
                update_interval: legacy.update_interval,
                last_update_check: legacy.last_update_check,
            },
            view: ViewPrefs {
                diff_view: legacy.diff_view,
                right_view: legacy.right_view,
                pane_diff_view: legacy.pane_diff_view,
            },
            workspaces: WorkspacePrefs {
                workspaces: legacy.workspaces,
                roots: legacy.roots,
                folder_bookmarks: legacy.folder_bookmarks,
            },
        }
    }
}

/// Round a ratio to 4 decimals so persisted geometry doesn't carry f64 noise like
/// `0.49333333333333335`. Applied on write only — the load-side clamps stay authoritative.
pub(crate) fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
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

/// v3's nested state file. Kept SEPARATE from the legacy flat `state.json` so the two schemas never
/// collide: v3 reads/writes `state-v3.json`, while pre-v3 builds keep using their own `state.json`.
/// Pinning an older build is therefore non-destructive — each version owns its config file.
pub fn state_path() -> Option<PathBuf> {
    Some(config_dir()?.join("state-v3.json"))
}

/// The legacy flat `state.json` (still owned by pre-v3 builds). v3 reads it ONCE to seed
/// `state-v3.json` on first run, and never writes it.
pub fn legacy_state_path() -> Option<PathBuf> {
    Some(config_dir()?.join("state.json"))
}

/// Whether the raw JSON carries a `version >= 1` key — i.e. it's the NESTED schema. Absent / `0` /
/// unparseable ⇒ treat as the legacy flat schema. Tolerant: a parse error ⇒ `false` ⇒ legacy path
/// ⇒ `"{}"` fallback (never panics).
fn is_versioned(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| value.get("version").and_then(serde_json::Value::as_u64))
        .is_some_and(|version| version >= 1)
}

/// Parse raw state.json text into the nested `PersistedState`. Versioned files deserialize directly;
/// legacy flat files go through `LegacyFlatState` → `From`. Any failure falls back to `"{}"` so every
/// serde default applies (notably the `default_true` ones). Pure — no filesystem (so tests are hermetic).
pub(crate) fn parse(raw: &str) -> PersistedState {
    if is_versioned(raw) {
        serde_json::from_str(raw)
            .or_else(|_| serde_json::from_str("{}"))
            .expect("empty object deserializes with serde defaults")
    } else {
        let legacy: LegacyFlatState = serde_json::from_str(raw)
            .or_else(|_| serde_json::from_str("{}"))
            .expect("empty object deserializes with serde defaults");
        PersistedState::from(legacy)
    }
}

/// Load persisted UI state. Prefer v3's `state-v3.json`; if it's absent (first v3 launch), SEED from
/// the legacy flat `state.json` (a pre-v3 build's file) and migrate it — without ever modifying
/// `state.json`, so older builds keep their own config intact. The first `save()` writes `state-v3.json`.
pub fn load() -> PersistedState {
    if let Some(v3) = state_path() {
        if let Ok(raw) = std::fs::read_to_string(&v3) {
            return parse(&raw); // existing v3 file (nested)
        }
    }
    // No state-v3.json yet — seed from the old flat state.json (left untouched), or defaults.
    let legacy = legacy_state_path().and_then(|path| std::fs::read_to_string(&path).ok());
    parse(legacy.as_deref().unwrap_or("{}"))
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
        // An OLD flat file (no `version`) with a single `roots` list and no `workspaces` →
        // workspaces["default"]. Goes through the legacy migration path via `parse`.
        let state = parse(r#"{"roots":["/a","/b"]}"#);
        let workspaces = state.workspaces.migrated();
        assert_eq!(workspaces.get("default"), Some(&vec!["/a".to_string(), "/b".to_string()]));
        // Named workspaces present → legacy roots are ignored (no migration).
        let state = parse(r#"{"roots":["/legacy"],"workspaces":{"work":["/x"]}}"#);
        let workspaces = state.workspaces.migrated();
        assert_eq!(workspaces.get("work"), Some(&vec!["/x".to_string()]));
        assert!(!workspaces.contains_key("default"));
        // Neither present → empty.
        assert!(PersistedState::default().workspaces.migrated().is_empty());
    }

    #[test]
    fn old_state_without_background_loads() {
        // A pre-split flat file has no `background` key; the legacy path → None.
        let state = parse(r#"{"contrast":"soft","theme":"dark"}"#);
        assert_eq!(state.theming.background, None);
        assert_eq!(resolve_background(state.theming.background, state.theming.contrast), Background::Soft);
    }

    #[test]
    fn removed_discovery_sort_loads_as_name_without_losing_other_fields() {
        // An old flat file with the removed "discovery" sort must not reset the whole file.
        let state = parse(r#"{"sort_column":"discovery","panel_padding":true,"grouping_enabled":true}"#);
        assert_eq!(state.lists.sort_column, SortColumn::Name);
        assert!(state.layout.panel_padding);
        assert!(state.lists.grouping_enabled);
    }

    #[test]
    fn base_overrides_default_empty_and_round_trip() {
        // An old flat file without the key loads with an empty override map (no panic, no reset).
        let old = parse(r#"{"panel_padding":true}"#);
        assert!(old.repo_page.base_overrides.is_empty());
        assert!(old.layout.panel_padding);
        // A set override round-trips through the NESTED (versioned) serialize → parse path.
        let mut state = PersistedState { version: SCHEMA_VERSION, ..Default::default() };
        state.repo_page.base_overrides.insert("/repo\u{1f}feature".to_string(), "origin/stage".to_string());
        let json = serde_json::to_string(&state).unwrap();
        let back = parse(&json);
        assert_eq!(
            back.repo_page.base_overrides.get("/repo\u{1f}feature").map(String::as_str),
            Some("origin/stage")
        );
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
            assert_eq!(parse(json).lists.sort_column, expected, "for {json}");
        }
    }

    #[test]
    fn legacy_flat_file_migrates_to_nested() {
        // A representative OLD flat file (no `version`) — values must land in the right sections.
        let flat = r#"{
            "theme":"light","contrast":"normal","split_ratio":0.625,
            "repo_page_info":false,"repo_page_maximized":true,
            "workspaces":{"work":["/x","/y"]},
            "base_overrides":{"repo-x":"origin/main"},
            "last_seen_version":"2.50.0","auto_pull_on_launch":false,"icon_style":"emoji"
        }"#;
        let state = parse(flat);
        assert_eq!(state.version, SCHEMA_VERSION);
        assert_eq!(state.theming.theme, Theme::Light);
        assert_eq!(state.theming.icon_style, IconStyle::Emoji);
        assert_eq!(state.layout.split_ratio, 0.625);
        assert!(!state.repo_page.repo_page_info);
        assert!(state.repo_page.repo_page_maximized);
        assert_eq!(state.workspaces.workspaces.get("work"), Some(&vec!["/x".to_string(), "/y".to_string()]));
        assert_eq!(state.repo_page.base_overrides.get("repo-x").map(String::as_str), Some("origin/main"));
        assert_eq!(state.session.last_seen_version, "2.50.0");
        assert!(!state.sync.auto_pull_on_launch);
    }

    #[test]
    fn versioned_file_loads_directly() {
        // A nested file with `version` deserializes straight through (no legacy remap).
        let mut state = PersistedState { version: SCHEMA_VERSION, ..Default::default() };
        state.theming.theme = Theme::Dark;
        state.sync.auto_pull_max_repos = 250;
        let json = serde_json::to_string(&state).unwrap();
        let back = parse(&json);
        assert_eq!(back.version, SCHEMA_VERSION);
        assert_eq!(back.theming.theme, Theme::Dark);
        assert_eq!(back.sync.auto_pull_max_repos, 250);
    }

    #[test]
    fn empty_object_uses_defaults() {
        // `{}` has no version → legacy path → the `default_true` carry-through must hold.
        let state = parse("{}");
        assert!(state.layout.show_result_panel);
        assert!(state.layout.panel_padding);
        assert!(state.repo_page.repo_page_info);
        assert!(state.sync.auto_pull_on_launch);
        assert!(state.lists.grouping_enabled);
        assert!(state.interaction.hover_effects);
        assert_eq!(state.sync.auto_pull_max_repos, 100);
        // A whole missing section also defaults correctly (sub-struct Default).
        assert_eq!(state.layout.splitter_mode, SplitterMode::Hover);
        assert_eq!(state.repo_page.repo_page_tabs, RepoTabsMode::Auto);
    }

    #[test]
    fn round4_trims_ratio_noise() {
        assert_eq!(round4(0.493_333_333_333_353_5), 0.4933);
        assert_eq!(round4(0.319_148_936_170_212_8), 0.3191);
        assert_eq!(round4(0.4), 0.4);
    }
}
