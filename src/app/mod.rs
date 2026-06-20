use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use crate::groups::{self, GroupSource, GroupsCache, GroupsConfig};

mod types;
pub use types::*;

mod dropdown;
mod state1;
mod state2;
mod state3;

#[cfg(test)]
mod tests;

/// The overall application state, shared between the async worker tasks and the UI.
pub struct AppState {
    /// Repos in alphabetical order.
    pub repos: Vec<SharedRepoState>,
    /// Worktree entries (discovered asynchronously).
    pub worktrees: Vec<WorktreeEntry>,
    /// Worktree discovery complete?
    pub worktrees_done: bool,
    /// Recursive repo discovery complete? Gates the "all done" edge so it can't fire on the
    /// empty repo set before the walker has streamed anything in.
    pub discovery_done: bool,
    /// Index of the selected item in the list (0 = first repo, repos.len() = Result).
    pub selected: usize,
    /// Whether the user has manually moved the selection (disables auto-select).
    pub user_navigated: bool,
    /// Which main panel has keyboard focus (drives scroll keys + the bright pane border).
    pub focus: Pane,
    /// Filter string (from `/` mode).
    pub filter: Option<String>,
    /// Status filter picked via the `f` leader (default: show all).
    pub status_filter: StatusFilter,
    /// Column the list is sorted by (default: discovery order). Persisted.
    pub sort_column: SortColumn,
    /// Sort direction for `sort_column`. Persisted.
    pub sort_dir: SortDir,
    /// Filter input mode active?
    pub filter_input_mode: bool,
    /// Repo selected when name-filter input began — restored on Esc (cancel), dropped on Enter
    /// (commit). Lets `/` temporarily preview the first match while typing. Never persisted.
    pub filter_prev_selection: Option<usize>,
    /// Wall-clock start time (reset to now whenever a fresh batch of work is kicked off).
    pub start: Instant,
    /// Frozen elapsed once everything finished; `None` while work is running. Restarts (back to
    /// `None`) on any re-run (`r`/`R`/`f`/`F`).
    pub finished_elapsed: Option<Duration>,
    /// All pulls are done?
    pub all_done: bool,
    /// Number of jobs configured.
    pub max_jobs: usize,
    /// Left-pane width as a fraction of the main area (clamped MIN_SPLIT..MAX_SPLIT).
    pub split_ratio: f64,
    /// Whether the result/log panel (the bottom half of the preview) is shown. Off → the info panel
    /// fills the preview pane. Persisted.
    pub show_result_panel: bool,
    /// Info-panel height as a fraction of the preview pane when both info + result are shown
    /// (clamped PREVIEW_SPLIT_MIN..PREVIEW_SPLIT_MAX). Persisted.
    pub preview_split_ratio: f64,
    /// Screen row of the info/result boundary inside the preview (the horizontal splitter), captured
    /// each render for drag hit-testing. `None` when the pane isn't split.
    pub preview_divider_row: Option<u16>,
    /// The preview inner area the `preview_split_ratio` is measured against, captured each render.
    pub preview_split_area: Rect,
    /// Docked repo-panel height as a fraction of the main area (clamped DOCK_MIN..DOCK_MAX).
    pub dock_ratio: f64,
    /// Screen row of the docked-panel top boundary (the horizontal splitter), captured each
    /// render for drag hit-testing. `None` when no dock is shown.
    pub dock_divider_row: Option<u16>,
    /// The full main area (panes + dock) the `dock_ratio` is measured against, captured each render.
    pub dock_full_area: Rect,
    /// The restored repo-page panel's outer rect, captured each render (empty when not restored).
    /// Mouse events outside it fall through to the list/preview so the restored panel is master-detail.
    pub dock_rect: Rect,
    /// When true, the preview shows the Result summary regardless of selection.
    pub result_overlay: bool,
    /// Main content area (above the status bar) — captured each render for hit-testing.
    pub main_area: Rect,
    /// Left list pane rect (outer, with border) — captured each render for hit-testing.
    pub list_area: Rect,
    /// The exact rect the repo rows render into (inner, below the 2-row header) — used for
    /// click→row mapping so it's correct regardless of border/padding/header offsets.
    pub list_rows_area: Rect,
    /// Clickable PR-cell regions in the list (PR column): (row, col_start, col_end, url). Rebuilt
    /// each render; a click opens the PR in the browser.
    pub pr_cell_click: Vec<(u16, u16, u16, String)>,
    /// Clickable favorite-star regions in the list: (row, col_start, col_end, repo_idx). Rebuilt
    /// each render; a click toggles that repo's favorite state.
    pub fav_cell_click: Vec<(u16, u16, u16, usize)>,
    /// The column-header rect (the 2 rows above the repo list) — for header click-to-sort.
    pub header_area: Rect,
    /// Header sort-cell hit map: (col_start, col_end, column). Rebuilt each render.
    pub header_click: Vec<(u16, u16, SortColumn)>,
    /// Right preview pane rect — captured each render for hit-testing.
    pub preview_area: Rect,
    /// Total content lines + visible height of the preview, captured each render so wheel/
    /// scrollbar scrolling clamps to the real content (not the log length) and never over-scrolls.
    pub preview_total: usize,
    pub preview_viewport: usize,
    /// The rect `render_scrollbar` drew the preview scrollbar on (the info-split lower chunk when
    /// the info block is pinned, else the full preview) — for scrollbar click/drag hit-testing.
    pub preview_scroll_area: Rect,
    /// Column of the divider between the panes (= preview_area.x).
    pub divider_col: u16,
    /// True while the user is dragging the pane divider (drives the live drag highlight).
    pub divider_dragging: bool,
    /// Scroll offset of the list widget, read back after render for row hit-testing.
    pub list_offset: usize,
    /// Manual list scroll position (viewport-top item row). The plain mouse wheel drives this
    /// independently of the selection (web-app style); keyboard / Alt+wheel nav scrolls it only
    /// enough to keep the selection visible.
    pub list_scroll: usize,
    /// What the right pane shows for the selected repo (log, info, or diff).
    pub right_view: RightView,
    /// Whether a compact info section is pinned above the log/diff (`I`).
    pub info_pinned: bool,
    /// Clickable regions in the info block / log copy button: `(row, col_start, col_end, action)`.
    /// Rebuilt every frame.
    pub info_click: Vec<(u16, u16, u16, InfoAction)>,
    /// Info-block field labels the user expanded (e.g. "Path", "Last commit") — show the full,
    /// wrapped value instead of a left-truncated one. Session-only.
    pub info_expanded: HashSet<String>,
    /// Whether the help modal (`?`) is open.
    pub show_help: bool,
    /// Which help tab is active (persisted so it reopens where you left it).
    pub help_tab: HelpTab,
    /// Scroll offset within the help modal.
    pub help_scroll: usize,
    /// Clickable links in the help modal: (absolute screen row, url). Rebuilt each render.
    pub help_links: Vec<(u16, String)>,
    /// Active filter over the Hotkeys help tab (`/` to start; `Some` = filtering). Session-only.
    pub help_filter: Option<String>,
    /// Whether the collapsible "Notes" link group in the About tab is expanded. Session-only.
    pub help_notes_expanded: bool,
    /// Screen row of the clickable "Notes" group toggle in the About tab. Rebuilt each render.
    pub help_notes_toggle_row: Option<u16>,
    /// A browser-style status line shown at the bottom-left while hovering a link (its URL).
    /// Set each frame by the hover handler; `None` clears it.
    pub status_hint: Option<String>,
    /// Whether the help modal is maximized (≈90% of the viewport). Session-only.
    pub help_maximized: bool,
    /// The clickable maximize/restore toggle in the help tab bar: (row, col_start, col_end).
    pub help_maximize_click: Option<(u16, u16, u16)>,
    /// Interactive CLI-builder state (the "CLI & Flags" help tab).
    pub cli_builder: CliBuilder,
    /// Clickable CLI-builder flag rows: (row, flag index). Rebuilt each render.
    pub cli_flag_click: Vec<(u16, usize)>,
    /// The clickable `[Copy]` button in the CLI builder: (row, col_start, col_end).
    pub cli_copy_click: Option<(u16, u16, u16)>,
    /// Clickable help-modal tab chips: (row, col_start, col_end, tab). Rebuilt each render.
    pub help_tab_click: Vec<(u16, u16, u16, HelpTab)>,
    /// The clickable `[esc]` close region in the help modal: (row, col_start, col_end).
    pub help_close_click: Option<(u16, u16, u16)>,
    /// Clickable radio regions on the Design System help tab: (row, col_start, col_end, settings
    /// row_idx, Option<option_idx>) — same shape as `settings_click`, dispatched the same way.
    pub help_design_click: Vec<(u16, u16, u16, usize, Option<usize>)>,
    // Keyboard viewer (a button on the Hotkeys help tab opens it):
    /// The interactive keyboard modal is open. While open it captures every keypress (Esc closes).
    pub show_keyboard: bool,
    /// The key the user last pressed/clicked on the board (its layout `code`); drives the panel.
    pub keyboard_selected: Option<&'static str>,
    /// Scroll offset in the keyboard modal's actions panel.
    pub keyboard_scroll: usize,
    /// The keyboard modal's outer area (for outside-click close).
    pub keyboard_area: Rect,
    /// The actions panel's area in the keyboard modal (for wheel-scroll hit-testing).
    pub keyboard_panel_area: Rect,
    /// The keyboard modal's `[esc]` close region: (row, col_start, col_end).
    pub keyboard_close_click: Option<(u16, u16, u16)>,
    /// Clickable key cells in the keyboard modal: (row, col_start, col_end, code). Rebuilt each render.
    pub keyboard_key_click: Vec<(u16, u16, u16, &'static str)>,
    /// The "keyboard" button region on the Hotkeys help tab: (row, col_start, col_end).
    pub help_keyboard_click: Option<(u16, u16, u16)>,
    /// When Some, the dedicated repo page is open for this absolute repo index.
    pub repo_page: Option<usize>,
    /// The repo page's scrolling content area (for hover row hit-testing). Set each render.
    pub repo_page_inner: Rect,
    /// Selected row within the repo page (index into its selectable branch/worktree rows).
    pub repo_page_selected: usize,
    /// Whether the repo page uses tabs for branches/worktrees/stashes (persisted).
    pub repo_page_tabs: RepoTabsMode,
    /// Repo-page window state: maximized (full-screen) vs restored (docked bottom panel). Default
    /// restored. This is the single source of truth for both the current state and the state a page
    /// opens in next time (sticky, Windows-like). Persisted.
    pub repo_page_maximized: bool,
    /// Periodic local branch/status refresh mode (persisted).
    pub branch_check: BranchCheck,
    /// The active repo-page tab (when tabbed). Session-only.
    pub repo_page_tab: RepoTab,
    /// Clickable repo-page tab chips: (row, col_start, col_end, tab). Rebuilt each render.
    pub repo_page_tab_click: Vec<(u16, u16, u16, RepoTab)>,
    /// Pending one-shot: snap the repo-page selection to the HEAD branch once its rows load.
    pub repo_page_focus_head: bool,
    /// Scroll offset within the repo page.
    pub repo_page_scroll: usize,
    /// Transient banner on the repo page (action result or error).
    pub repo_page_message: Option<String>,
    /// Active confirmation dialog, if any.
    pub confirm: Option<ConfirmDialog>,
    /// Which optional list columns are enabled.
    pub columns: ColumnFlags,
    /// Relative paths of repos marked as favorites (persisted).
    pub favorites: HashSet<String>,
    /// Pin a "★ Favorites" section to the top of the list (persisted).
    pub favorites_first: bool,
    /// A pending leader chord (e.g. `t` awaiting a column key).
    pub pending_leader: Option<Leader>,
    /// Whether the background "fetch details for all repos" pass has been spawned.
    pub details_pass_spawned: bool,
    /// Clickable command regions in the status bar (rebuilt each render).
    pub clickable: Vec<ClickRegion>,
    /// Clickable hint regions in the repo-page / modal footers, mapped to the key they fire
    /// (rebuilt each render).
    pub hint_click: Vec<HintClick>,
    /// Draggable scrollbars registered each render (preview, diff panels, help, repo page).
    pub scroll_hits: Vec<ScrollHit>,
    /// Which scrollbar is currently being dragged (drives the live drag highlight).
    pub scrollbar_dragging: Option<ScrollKind>,
    /// Repo-page row hit map: (absolute screen row, selectable index). Rebuilt each render.
    pub repo_page_click: Vec<(u16, usize)>,
    /// The 90% diff modal (stash diff or a dirty branch/worktree diff), if open.
    pub diff_modal: Option<DiffModal>,
    /// Visible line count of the diff modal's diff panel, captured at render for PgUp/PgDn.
    pub diff_modal_viewport: usize,
    /// Visible row count of the diff modal's file-list panel (to keep the selection in view).
    pub diff_files_viewport: usize,
    /// Inner rect of the diff modal's file-list panel (mouse hit-testing + wheel routing).
    pub diff_files_area: Rect,
    /// Inner rect of the diff modal's diff panel (wheel routing).
    pub diff_body_area: Rect,
    /// The directories/roots being scanned (each may itself be a single repo). Drives the per-root
    /// tree forest; worktree re-discovery derives parents from the repos.
    pub root_dirs: Vec<PathBuf>,
    /// All saved named workspaces (name → roots), loaded at startup. Persisted as-is; when a
    /// workspace is active its entry is refreshed from `root_dirs` on save.
    pub workspaces: HashMap<String, Vec<String>>,
    /// The active named workspace, if launched with `-w <name>` or via the `ws` picker. `None` for
    /// an ad-hoc cwd/CLI-dirs session — those never write a workspace, so the picker (`A`) is
    /// session-only until you launch under a name.
    pub active_workspace: Option<String>,
    // Settings (persisted):
    /// Draw 1-cell inner padding inside every bordered panel/modal.
    pub panel_padding: bool,
    /// Which glyph set to render (Unicode vs emoji).
    pub icon_style: IconStyle,
    /// Hide zero-count cells (emoji always hides; this extends it to the Unicode set).
    pub hide_zero_counts: bool,
    /// Hide the dash-fill leader lines in group / folder headers.
    pub hide_folder_lines: bool,
    /// Color theme (auto = detect terminal background; dark/light = forced).
    pub theme: Theme,
    /// Contrast level for the active palette (text + accent saturation).
    pub contrast: Contrast,
    /// Selected-row highlight style (blue bar vs subtle tint that keeps column colors).
    pub selection_style: SelectionStyle,
    /// Button hover style (reverse-video vs soft tint) for footer/modal hints, tabs, chips, keys.
    pub button_hover_style: ButtonHoverStyle,
    /// Background tone for the active palette (surface only), independent of `Contrast`.
    pub background: Background,
    /// Whether the terminal background was detected as dark at startup (resolves `Theme::Auto`).
    pub auto_dark: bool,
    /// Whether the settings modal (`,`) is open.
    pub show_settings: bool,
    /// Selected (global) row in the settings modal — see `SETTINGS_TABS` for the row order.
    pub settings_selected: usize,
    /// Accordion layout only: when `Some(section)`, the selection is on that section's header (the
    /// header is the active item, no child row is selected). `None` → a row is selected.
    pub settings_on_header: Option<usize>,
    /// Accordion-layout scroll offset (logical lines from the top), so a tall settings set scrolls
    /// to keep the selection visible instead of clipping lower sections.
    pub settings_scroll: usize,
    /// Active settings tab (index into `SETTINGS_TABS`) in the tabbed layout.
    pub settings_tab: usize,
    /// Settings search query (empty = no filter). When non-empty the modal shows a flat list of the
    /// matching rows with the matched chars highlighted. Session-only (reset on open).
    pub settings_search: String,
    /// Whether the settings search input is focused (typing edits the query). `/` focuses it.
    pub settings_search_focused: bool,
    /// The clickable settings search-box region: (row, col_start, col_end). Rebuilt each render.
    pub settings_search_click: Option<(u16, u16, u16)>,
    /// Settings modal layout (tabbed / accordion / flat). Persisted.
    pub settings_layout: SettingsLayout,
    /// Section names collapsed in the accordion settings layout. Persisted.
    pub collapsed_settings: HashSet<String>,
    /// Clickable settings tab labels: (row, col_start, col_end, tab index). Rebuilt each render.
    pub settings_tab_click: Vec<(u16, u16, u16, usize)>,
    /// Clickable accordion section headers: (row, col_start, col_end, tab index). Rebuilt each render.
    pub settings_section_click: Vec<(u16, u16, u16, usize)>,
    /// The accordion expand/collapse-all button region: (row, col_start, col_end). Rebuilt each render.
    pub settings_collapse_all_click: Option<(u16, u16, u16)>,
    /// The repo-page `y` copy menu, when open: the selected option (0 = path, 1 = branch, 2 = both).
    pub copy_menu: Option<usize>,
    /// A transient toast (auto-dismisses after `TOAST_DURATION`).
    pub toast: Option<Toast>,
    // Modal mouse geometry — captured each render (same pattern as `help_close_click`).
    // Close buttons are `(row, col_start, col_end)`; areas drive click-outside-closes.
    pub settings_area: Rect,
    pub settings_close_click: Option<(u16, u16, u16)>,
    /// Settings hit map: (row, col_start, col_end, settings row, Some(option) | None = label).
    pub settings_click: Vec<(u16, u16, u16, usize, Option<usize>)>,
    pub copy_menu_area: Rect,
    pub copy_menu_close_click: Option<(u16, u16, u16)>,
    /// Copy-menu option rows: (screen row, option index).
    pub copy_menu_click: Vec<(u16, usize)>,
    pub confirm_area: Rect,
    pub confirm_close_click: Option<(u16, u16, u16)>,
    pub confirm_yes_click: Option<(u16, u16, u16)>,
    pub confirm_no_click: Option<(u16, u16, u16)>,
    pub diff_modal_area: Rect,
    pub diff_modal_close_click: Option<(u16, u16, u16)>,
    /// Clickable status-filter chips in the diff modal: `(row, col_start, col_end, bucket)`
    /// where `bucket` is `None` for the "all" chip. Rebuilt every frame.
    pub diff_chips_click: Vec<(u16, u16, u16, Option<char>)>,
    pub help_area: Rect,
    /// The repo page's clickable `[esc back]` button on the top border.
    pub repo_page_back_click: Option<(u16, u16, u16)>,
    /// The repo page's clickable maximize/restore button on the top border (left of `[esc back]`).
    pub repo_page_window_click: Option<(u16, u16, u16)>,
    /// The repo page's clickable PR cell on the current-branch row: (row, col_start, col_end, url).
    pub repo_page_pr_click: Option<(u16, u16, u16, String)>,
    /// An open header `[… ▾]` dropdown (columns / sort), the mouse companion to the t/s leaders.
    pub dropdown: Option<Dropdown>,
    /// Dropdown geometry, captured each render: its outer rect, the `[x]` close region, and the
    /// per-item click rows `(row, col_start, col_end, item index)`.
    pub dropdown_area: Rect,
    pub dropdown_close_click: Option<(u16, u16, u16)>,
    pub dropdown_item_click: Vec<(u16, u16, u16, usize)>,
    /// The clickable `[cols ▾]` / `[sort ▾]` chips on the list header and the repo-page title bar.
    pub list_cols_click: Option<(u16, u16, u16)>,
    pub list_sort_click: Option<(u16, u16, u16)>,
    pub page_cols_click: Option<(u16, u16, u16)>,
    pub page_sort_click: Option<(u16, u16, u16)>,
    /// Which repo-page branch columns are shown (persisted).
    pub repo_page_columns: RepoPageColumns,
    /// The page-local `t` column-toggle menu is open.
    pub repo_page_toggle: bool,
    /// Clickable repo-page column-toggle chips: `(row, col_start, col_end, column)`.
    pub repo_page_toggle_click: Vec<(u16, u16, u16, RepoPageColumn)>,
    /// Repo-page branch-table sort column; `None` = natural order (HEAD first). Session-only.
    pub repo_page_sort: Option<RepoPageSort>,
    /// Direction for `repo_page_sort`.
    pub repo_page_sort_dir: SortDir,
    /// Clickable repo-page sort headers: `(row, col_start, col_end, sort)`. Rebuilt each render.
    pub repo_page_sort_click: Vec<(u16, u16, u16, RepoPageSort)>,
    /// Show the bottom info panel on the repo page (persisted, default on).
    pub repo_page_info: bool,
    /// The open base-branch picker (clicking a `base` cell or pressing `b`), if any.
    pub base_picker: Option<BasePicker>,
    pub base_picker_area: Rect,
    pub base_picker_close_click: Option<(u16, u16, u16)>,
    /// Base-picker option rows: (screen row, option index — 0 = detected, then candidates).
    pub base_picker_click: Vec<(u16, usize)>,
    /// The fzf-style finder overlay (`P`), when open. Searches all repos to jump the selection.
    pub finder: Option<tui_pick::finder::FinderState>,
    /// Shared goto-repo usage history, consulted for recent/most-used sort and appended on jump.
    pub finder_history: tui_pick::History,
    pub finder_area: Rect,
    pub finder_close_click: Option<(u16, u16, u16)>,
    /// Finder row hit map: (screen row, view index). Rebuilt each render.
    pub finder_rows_click: Vec<(u16, usize)>,
    /// The folder picker overlay (`A`), when open. Selecting a folder/repo adds it as a root.
    pub picker: Option<tui_pick::picker::PickerState>,
    pub picker_area: Rect,
    pub picker_close_click: Option<(u16, u16, u16)>,
    /// Picker row hit map: (screen row, view index). Rebuilt each render.
    pub picker_rows_click: Vec<(u16, usize)>,
    /// Picker breadcrumb hit map: (row, col_start, col_end, target path). Rebuilt each render.
    pub picker_crumbs_click: Vec<(u16, u16, u16, PathBuf)>,
    /// Bookmarked folders for the picker (persisted; absolute paths).
    pub folder_bookmarks: Vec<String>,
    /// Discovery config captured at launch so a root added at runtime (via the picker) can be
    /// scanned with the same settings from the event loop.
    pub discovery_max_depth: usize,
    pub discovery_timeout_secs: u64,
    pub discovery_no_worktrees: bool,
    /// Clickable `base` cells on the repo page: `(row, col_start, col_end, selectable index)`.
    pub base_cell_click: Vec<(u16, u16, u16, usize)>,
    /// Persisted base-branch overrides, keyed `"{repo_abs_path}\u{1f}{branch}"` → base ref.
    pub base_overrides: HashMap<String, String>,
    // New-build notice (a newer binary landed at this executable's path while running):
    pub update_available: bool,
    pub update_dismissed: bool,
    pub update_reload_click: Option<(u16, u16, u16)>,
    pub update_close_click: Option<(u16, u16, u16)>,
    /// When the running binary was built (its mtime at startup) — shown as "built … ago".
    pub binary_built: Option<std::time::SystemTime>,
    /// The watched executable path (resolved at startup) — shown in the build-info modal.
    pub exe_path: String,
    /// Whether the build-info modal (the clickable "built … ago" tag) is open.
    pub show_build_info: bool,
    /// The build-info modal's `[x]` close button region.
    pub build_info_close_click: Option<(u16, u16, u16)>,
    /// Build-info details, captured when the modal opens: running binary size (bytes), the
    /// settings file path, the count of files in the config dir, the settings JSON as lines (for
    /// the scrollable, syntax-highlighted preview), and the preview's scroll offset.
    pub build_info_binary_size: u64,
    pub build_info_settings_path: String,
    pub build_info_config_count: usize,
    pub build_info_settings_preview: Vec<String>,
    pub build_info_scroll: usize,
    // Grouping (`z`, groups from ~/.config/polygit/groups.json):
    /// Render the list grouped (`z` toggles; persisted). Inert while `groups` is empty.
    pub grouping_enabled: bool,
    /// Configured groups in config order (empty when groups.json is missing/empty).
    pub groups: Vec<GroupRuntime>,
    /// Repo index → group index (None = ungrouped). Rebuilt on membership changes, not per frame.
    pub repo_group_map: Vec<Option<usize>>,
    /// Names of collapsed groups (persisted).
    pub collapsed_groups: HashSet<String>,
    /// Groups with more members than this get collapsible headers.
    pub collapse_threshold: usize,
    /// Dynamic-source cache freshness in minutes.
    pub group_cache_ttl_minutes: u64,
    // Tree view (`v t`, directory tree from recursive discovery):
    /// Render the list as a collapsible directory tree (`v t` toggles; persisted).
    /// Inert when every repo is at the scan root (no folders to nest).
    pub tree_enabled: bool,
    /// Folder nodes built from the repos' relative paths (rebuilt as repos stream in).
    pub tree_nodes: Vec<TreeNode>,
    /// Relative paths of collapsed folders (persisted).
    pub collapsed_folders: HashSet<String>,
    /// Shared concurrency gate + throttle adaptation, used by every pull path.
    pub throttle: Arc<ThrottleControl>,
    // Auto-pull-on-launch policy (Settings → Sync; persisted):
    /// Pull every repo automatically on launch (default on).
    pub auto_pull_on_launch: bool,
    /// Skip the launch auto-pull above this repo count. `0` = no limit.
    pub auto_pull_max_repos: u32,
    /// Allow the launch auto-pull while the tree view is active (default off).
    pub auto_pull_in_tree: bool,
    /// Highlight actionable elements under the cursor (persisted). Enabling it turns on all-motion
    /// mouse tracking (main.rs syncs the terminal mode to this flag).
    pub hover_effects: bool,
    /// Draw borders around the two main panes (persisted, default on).
    pub show_borders: bool,
    /// Draw the draggable splitter grip between the panes (persisted, default on).
    pub show_splitter: bool,
    /// Pulse changed cells after a pull/refresh (persisted, default on).
    pub changed_row_flash: bool,
    /// Steadily highlight changed cells for the attention window (persisted, default off).
    pub changed_row_highlight: bool,
    /// Current mouse position `(col, row)` while `hover_effects` is on, else `None`. Drives the
    /// post-render hover highlight; never persisted.
    pub hover: Option<(u16, u16)>,
    /// The active dwell tooltip, set after dwelling ~1s on a hoverable element; rendered as a small
    /// popup placed by the floating engine (flip + shift). Never persisted.
    pub hover_tooltip: Option<HoverTip>,
    /// Dwell-tooltip regions captured each frame. Covers status-bar commands (via [`Self::command_at`]),
    /// the column headers, and group/folder count tails. Hovering one ~1s shows its text.
    pub hover_tooltips: Vec<TooltipRegion>,
    /// The active tooltip popup's rect (captured each render) — lets the dwell keep the tooltip
    /// alive while the cursor moves onto it (so its `[x]` hide-column button stays clickable).
    pub tooltip_rect: Rect,
    /// The tooltip's clickable `[x]` hide-column button: (row, col_start, col_end, column).
    pub tooltip_hide_click: Option<(u16, u16, u16, Column)>,
    /// Set once discovery completes and the launch decision skipped pulling — the run is then
    /// "settled" without any repo being pulled, and the footer offers a manual pull-everything.
    pub auto_pull_suppressed: bool,
    /// Persisted per-repo last-known state, loaded at startup to seed the list instantly and
    /// upserted as repos are pulled/refreshed. Flushed to disk on settle + quit.
    pub status_cache: crate::cache::StatusCache,
    /// Persisted PR cache (repo+branch → open PR + timestamp, 5-min TTL). Consulted before a `gh`
    /// lookup and upserted from resolved repos on flush.
    pub pr_cache: crate::pr_cache::PrCache,
    /// Set once the all-repos PR background pass has been spawned (when the PR column is enabled).
    /// Reset when the column is toggled off so re-enabling re-arms it.
    pub pr_pass_spawned: bool,
    /// Latched true once any pull has landed a delta this session. Keeps the pulled/chg columns
    /// stable: a retry/refetch (which clears `pull_result` at pull start, then resets it) no longer
    /// flickers the column in and back out. Runtime-only, fresh per launch.
    pub pulled_seen: bool,
}

impl AppState {
    pub fn new(repos: Vec<SharedRepoState>, max_jobs: usize, auto_dark: bool) -> Self {
        // Restore persisted UI preferences (columns, info state, splitter), falling back to
        // defaults for anything missing or invalid.
        let persisted = crate::persist::load();
        let split_ratio = if persisted.split_ratio >= Self::MIN_SPLIT {
            persisted.split_ratio.clamp(Self::MIN_SPLIT, Self::MAX_SPLIT)
        } else {
            Self::DEFAULT_SPLIT
        };
        let dock_ratio = if persisted.dock_ratio >= Self::DOCK_MIN {
            persisted.dock_ratio.clamp(Self::DOCK_MIN, Self::DOCK_MAX)
        } else {
            Self::DOCK_DEFAULT
        };
        let preview_split_ratio = if persisted.preview_split_ratio >= Self::PREVIEW_SPLIT_MIN {
            persisted.preview_split_ratio.clamp(Self::PREVIEW_SPLIT_MIN, Self::PREVIEW_SPLIT_MAX)
        } else {
            Self::PREVIEW_SPLIT_DEFAULT
        };
        // Compute before the struct literal moves other `persisted` fields out.
        let workspaces = persisted.workspaces_migrated();
        AppState {
            repos,
            worktrees: Vec::new(),
            worktrees_done: false,
            discovery_done: false,
            selected: 0,
            user_navigated: false,
            focus: Pane::default(),
            filter: None,
            status_filter: StatusFilter::default(),
            sort_column: persisted.sort_column,
            sort_dir: persisted.sort_dir,
            filter_input_mode: false,
            filter_prev_selection: None,
            start: Instant::now(),
            finished_elapsed: None,
            all_done: false,
            max_jobs,
            split_ratio,
            show_result_panel: persisted.show_result_panel,
            preview_split_ratio,
            preview_divider_row: None,
            preview_split_area: Rect::default(),
            dock_ratio,
            dock_divider_row: None,
            dock_full_area: Rect::default(),
            dock_rect: Rect::default(),
            result_overlay: false,
            main_area: Rect::default(),
            list_area: Rect::default(),
            list_rows_area: Rect::default(),
            pr_cell_click: Vec::new(),
            fav_cell_click: Vec::new(),
            header_area: Rect::default(),
            header_click: Vec::new(),
            preview_area: Rect::default(),
            preview_total: 0,
            preview_viewport: 0,
            preview_scroll_area: Rect::default(),
            divider_col: 0,
            divider_dragging: false,
            list_offset: 0,
            list_scroll: 0,
            right_view: RightView::Log,
            info_pinned: persisted.info_pinned,
            info_click: Vec::new(),
            info_expanded: HashSet::new(),
            show_help: false,
            help_tab: persisted.help_tab,
            help_scroll: 0,
            help_links: Vec::new(),
            help_filter: None,
            help_notes_expanded: false,
            help_notes_toggle_row: None,
            status_hint: None,
            help_maximized: false,
            help_maximize_click: None,
            cli_builder: CliBuilder {
                selected: 0,
                on: vec![false; CLI_FLAGS.len()],
                values: vec![String::new(); CLI_FLAGS.len()],
                editing: None,
                show_help: true,
            },
            cli_flag_click: Vec::new(),
            cli_copy_click: None,
            help_tab_click: Vec::new(),
            help_close_click: None,
            help_design_click: Vec::new(),
            show_keyboard: false,
            keyboard_selected: None,
            keyboard_scroll: 0,
            keyboard_area: Rect::default(),
            keyboard_panel_area: Rect::default(),
            keyboard_close_click: None,
            keyboard_key_click: Vec::new(),
            help_keyboard_click: None,
            repo_page: None,
            repo_page_inner: Rect::default(),
            repo_page_selected: 0,
            repo_page_tabs: persisted.repo_page_tabs,
            repo_page_maximized: persisted.repo_page_maximized,
            branch_check: persisted.branch_check,
            repo_page_tab: RepoTab::Branches,
            repo_page_tab_click: Vec::new(),
            repo_page_focus_head: false,
            repo_page_scroll: 0,
            repo_page_message: None,
            confirm: None,
            columns: persisted.columns,
            favorites: persisted.favorites.into_iter().collect(),
            favorites_first: persisted.favorites_first,
            pending_leader: None,
            details_pass_spawned: false,
            clickable: Vec::new(),
            hint_click: Vec::new(),
            scroll_hits: Vec::new(),
            scrollbar_dragging: None,
            repo_page_click: Vec::new(),
            diff_modal: None,
            diff_modal_viewport: 0,
            diff_files_viewport: 0,
            diff_files_area: Rect::default(),
            diff_body_area: Rect::default(),
            root_dirs: Vec::new(),
            workspaces,
            active_workspace: None,
            panel_padding: persisted.panel_padding,
            icon_style: persisted.icon_style,
            hide_zero_counts: persisted.hide_zero_counts,
            hide_folder_lines: persisted.hide_folder_lines,
            theme: persisted.theme,
            contrast: persisted.contrast,
            selection_style: persisted.selection_style,
            button_hover_style: persisted.button_hover_style,
            background: crate::persist::resolve_background(persisted.background, persisted.contrast),
            auto_dark,
            show_settings: false,
            settings_selected: 0,
            settings_on_header: None,
            settings_scroll: 0,
            settings_tab: 0,
            settings_search: String::new(),
            settings_search_focused: false,
            settings_search_click: None,
            settings_layout: persisted.settings_layout,
            collapsed_settings: persisted.collapsed_settings.into_iter().collect(),
            settings_tab_click: Vec::new(),
            settings_section_click: Vec::new(),
            settings_collapse_all_click: None,
            copy_menu: None,
            toast: None,
            settings_area: Rect::default(),
            settings_close_click: None,
            settings_click: Vec::new(),
            copy_menu_area: Rect::default(),
            copy_menu_close_click: None,
            copy_menu_click: Vec::new(),
            confirm_area: Rect::default(),
            confirm_close_click: None,
            confirm_yes_click: None,
            confirm_no_click: None,
            diff_modal_area: Rect::default(),
            diff_modal_close_click: None,
            diff_chips_click: Vec::new(),
            help_area: Rect::default(),
            repo_page_back_click: None,
            repo_page_window_click: None,
            repo_page_pr_click: None,
            dropdown: None,
            dropdown_area: Rect::default(),
            dropdown_close_click: None,
            dropdown_item_click: Vec::new(),
            list_cols_click: None,
            list_sort_click: None,
            page_cols_click: None,
            page_sort_click: None,
            repo_page_columns: persisted.repo_page_columns,
            repo_page_toggle: false,
            repo_page_toggle_click: Vec::new(),
            repo_page_sort: None,
            repo_page_sort_dir: SortDir::Asc,
            repo_page_sort_click: Vec::new(),
            repo_page_info: persisted.repo_page_info,
            base_picker: None,
            base_picker_area: Rect::default(),
            base_picker_close_click: None,
            base_picker_click: Vec::new(),
            finder: None,
            finder_history: tui_pick::History::load_default(),
            finder_area: Rect::default(),
            finder_close_click: None,
            finder_rows_click: Vec::new(),
            picker: None,
            picker_area: Rect::default(),
            picker_close_click: None,
            picker_rows_click: Vec::new(),
            picker_crumbs_click: Vec::new(),
            folder_bookmarks: persisted.folder_bookmarks.clone(),
            discovery_max_depth: 16,
            discovery_timeout_secs: 30,
            discovery_no_worktrees: false,
            base_cell_click: Vec::new(),
            base_overrides: persisted.base_overrides,
            update_available: false,
            update_dismissed: false,
            update_reload_click: None,
            update_close_click: None,
            binary_built: std::env::current_exe()
                .ok()
                .and_then(|exe| std::fs::metadata(exe).ok())
                .and_then(|meta| meta.modified().ok()),
            exe_path: std::env::current_exe()
                .map(|exe| exe.display().to_string())
                .unwrap_or_else(|_| "polygit".to_string()),
            show_build_info: false,
            build_info_close_click: None,
            build_info_binary_size: 0,
            build_info_settings_path: String::new(),
            build_info_config_count: 0,
            build_info_settings_preview: Vec::new(),
            build_info_scroll: 0,
            grouping_enabled: persisted.grouping_enabled,
            groups: Vec::new(),
            repo_group_map: Vec::new(),
            collapsed_groups: persisted.collapsed_groups.into_iter().collect(),
            collapse_threshold: groups::DEFAULT_COLLAPSE_THRESHOLD,
            group_cache_ttl_minutes: groups::DEFAULT_CACHE_TTL_MINUTES,
            tree_enabled: persisted.tree_enabled,
            tree_nodes: Vec::new(),
            collapsed_folders: persisted.collapsed_folders.into_iter().collect(),
            throttle: ThrottleControl::new(max_jobs),
            auto_pull_on_launch: persisted.auto_pull_on_launch,
            auto_pull_max_repos: persisted.auto_pull_max_repos,
            auto_pull_in_tree: persisted.auto_pull_in_tree,
            hover_effects: persisted.hover_effects,
            show_borders: persisted.show_borders,
            show_splitter: persisted.show_splitter,
            changed_row_flash: persisted.changed_row_flash,
            changed_row_highlight: persisted.changed_row_highlight,
            hover: None,
            hover_tooltip: None,
            hover_tooltips: Vec::new(),
            tooltip_rect: Rect::default(),
            tooltip_hide_click: None,
            auto_pull_suppressed: false,
            status_cache: crate::cache::load(),
            pr_cache: crate::pr_cache::load(),
            pr_pass_spawned: false,
            pulled_seen: false,
        }
    }
}
