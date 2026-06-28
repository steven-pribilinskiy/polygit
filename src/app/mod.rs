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
    /// The pinned list footer's rect (the Result / Errors summary below the scrolling rows) — for
    /// click→Result/Errors-row mapping. Empty when there's no footer.
    pub list_footer_area: Rect,
    /// Clickable PR-cell regions in the list (PR column): (row, col_start, col_end, url). Rebuilt
    /// each render; a click opens the PR in the browser.
    pub pr_cell_click: Vec<(u16, u16, u16, usize)>,
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
    /// Info pane ([2]) geometry captured each render so the wheel can scroll it (when the cursor is
    /// over it) and clamp to the real content, mirroring the preview's `preview_*` fields.
    pub info_area: Rect,
    pub info_total: usize,
    pub info_viewport: usize,
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
    /// The last non-About help tab — the value persisted (and reopened on). Switching to About
    /// (credits/links) never changes it, so reopening lands back on the last useful tab.
    pub help_tab_persist: HelpTab,
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
    /// Clickable help-display-mode chips in the CLI builder: (row, col_start, col_end, mode index).
    pub cli_helpmode_click: Vec<(u16, u16, u16, usize)>,
    /// Clickable tokens in the built-command preview: (row, flag index) — click removes that flag,
    /// hover highlights its row above. One token per line, so a row hit-test suffices.
    pub cli_command_click: Vec<(u16, usize)>,
    /// Clickable help-modal tab chips: (row, col_start, col_end, tab). Rebuilt each render.
    pub help_tab_click: Vec<(u16, u16, u16, HelpTab)>,
    /// The clickable `[esc]` close region in the help modal: (row, col_start, col_end).
    pub help_close_click: Option<(u16, u16, u16)>,
    /// Clickable radio regions on the Design System help tab: (row, col_start, col_end, settings
    /// row_idx, Option<option_idx>) — same shape as `settings_click`, dispatched the same way.
    pub help_design_click: Vec<(u16, u16, u16, usize, Option<usize>)>,
    /// The Design System tab's "preview confirm dialog" button region: (row, col_start, col_end).
    pub help_preview_click: Option<(u16, u16, u16)>,
    /// Design System tab layout (flat / tabbed-with-vertical-tabs). Persisted.
    pub design_layout: DesignLayout,
    /// Active Design System section (index) when the tab is in `Tabbed` layout.
    pub design_section: usize,
    /// Clickable vertical-tab regions on the Design System tab: (row, col_start, col_end, section).
    pub help_design_tab_click: Vec<(u16, u16, u16, usize)>,
    // Keyboard viewer (a button on the Hotkeys help tab opens it):
    /// The interactive keyboard modal is open. While open it captures every keypress (Esc closes).
    pub show_keyboard: bool,
    /// The key the user last pressed/clicked on the board (its layout `code`); drives the panel.
    pub keyboard_selected: Option<&'static str>,
    /// Modifiers held when `keyboard_selected` was set — `(shift, ctrl, alt)`. The actions panel
    /// filters to the chord that exactly matches them, so `Shift+G` reads only the `Shift+G` binding.
    pub keyboard_mods: (bool, bool, bool),
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
    /// In the maximized repo page, show the tabbed view instead of the flat stacked one (`v`
    /// toggle, persisted).
    pub repo_page_maximized_tabbed: bool,
    /// Session-only override of the restored repo page's tabbed/flat view: `v` sets it to flip the
    /// current view WITHOUT mutating the persisted `repo_page_tabs` preference (so an `Auto` setting
    /// survives a manual flip). `None` = follow the auto decision; cleared when the setting changes.
    pub repo_page_tabbed_override: Option<bool>,
    /// Repo-page sections collapsed in the flat (stacked) view, by section name (persisted).
    pub repo_page_collapsed_sections: HashSet<String>,
    /// Click/hover regions for the flat-view section headers: `(row, start, end, section)`. Rebuilt
    /// per frame; clicking toggles that section's collapse.
    pub repo_page_section_click: Vec<(u16, u16, u16, crate::app::RepoTab)>,
    /// Which pane (if any) is maximized to fill the screen — the single source of truth for every
    /// pane's maximize state. `Some(RepoPage)` is the only value that persists (round-tripped via
    /// the legacy `repo_page_maximized` field, so a repo page still opens maximized when sticky);
    /// maximizing List/Info/Result is a session-only action. `None` = the normal multi-pane layout.
    pub maximized: Option<Pane>,
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
    /// Click region of the confirm dialog's copyable line (e.g. the return-to-latest command),
    /// captured each render. Clicking it copies `confirm.copy_line` without accepting the dialog.
    pub confirm_copy_click: Option<(u16, u16, u16)>,
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
    /// Persisted diff-modal render style (raw / unified / split); new modals open in it.
    pub diff_view: DiffView,
    /// Surface merged & closed PRs (not just open) in the PR column + info panel. Off by default;
    /// detection always finds all states, so this gates display only (instant toggle, no re-query).
    pub show_merged_prs: bool,
    /// Visible line count of the diff modal's diff panel, captured at render for PgUp/PgDn.
    pub diff_modal_viewport: usize,
    /// Visible row count of the diff modal's file-list panel (to keep the selection in view).
    pub diff_files_viewport: usize,
    /// Inner rect of the diff modal's file-list panel (mouse hit-testing + wheel routing).
    pub diff_files_area: Rect,
    /// Inner rect of the diff modal's diff panel (wheel routing).
    pub diff_body_area: Rect,
    /// The PR viewer modal (full PR data + every comment, markdown-rendered), if open. Opened by
    /// clicking a PR `#N`; the body loads async via `gh pr view`.
    pub pr_modal: Option<PrModalState>,
    /// The PR modal's outer rect (outside-click closes) + its `[x]` close button region.
    pub pr_modal_area: Rect,
    pub pr_modal_close_click: Option<(u16, u16, u16)>,
    /// PR-modal collapsible section headers: `(row, col_start, col_end, section_idx)`.
    pub pr_section_click: Vec<(u16, u16, u16, usize)>,
    /// PR-modal `[collapse all] / [expand all]` control region (shown with >1 comment).
    pub pr_collapse_all_click: Option<(u16, u16, u16)>,
    /// PR-modal search box region (click to focus, then type to filter).
    pub pr_search_click: Option<(u16, u16, u16)>,
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
    /// Which AI coding-agent CLI the `c` hotkey launches (claude / codex / gemini).
    pub claude_agent: ClaudeAgent,
    /// Append the agent's "bypass all approval prompts" flag when launching with `c`.
    pub claude_skip_permissions: bool,
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
    /// The open kebab (`⋮`) row menu, if any (state-aware actions for the selected repo).
    pub kebab: Option<KebabMenu>,
    /// The kebab menu's outer rect (outside-click closes) + per-row item click regions + close button.
    pub kebab_area: Rect,
    pub kebab_click: Vec<(u16, usize)>,
    pub kebab_close_click: Option<(u16, u16, u16)>,
    /// Per-repo-row `⋮` kebab affordance click regions (rightmost column): `(row, col_start, col_end, repo_idx)`.
    pub kebab_open_click: Vec<(u16, u16, u16, usize)>,
    /// The "wrap copied prompt in `cd <repo> && claude '…'`" checkbox state (persisted).
    pub kebab_session_prefix: bool,
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
    /// Clickable maximize/restore buttons on the List/Info/Result panes' top borders:
    /// (row, col_start, col_end, pane). Rebuilt each frame; the repo page uses `repo_page_window_click`.
    pub max_click: Vec<(u16, u16, u16, Pane)>,
    /// The repo page's clickable PR cell on the current-branch row: (row, col_start, col_end, url).
    pub repo_page_pr_click: Option<(u16, u16, u16, String)>,
    /// An open header `[… ▾]` dropdown (columns / sort), the mouse companion to the t/s leaders.
    pub dropdown: Option<Dropdown>,
    /// Dropdown geometry, captured each render: its outer rect, the `[x]` close region, and the
    /// per-item click rows `(row, col_start, col_end, item index)`.
    pub dropdown_area: Rect,
    pub dropdown_close_click: Option<(u16, u16, u16)>,
    pub dropdown_item_click: Vec<(u16, u16, u16, usize)>,
    /// Columns-dropdown footer buttons (select/deselect-all, reset): `(row, col_start, col_end, action)`.
    pub dropdown_action_click: Vec<(u16, u16, u16, DropdownColAction)>,
    /// The clickable `[cols ▾]` / `[sort ▾]` chips on the list header and the repo-page title bar.
    pub list_cols_click: Option<(u16, u16, u16)>,
    pub list_sort_click: Option<(u16, u16, u16)>,
    /// The list pane's top-border `f by-status` trigger region (opens the status-filter dropdown).
    pub list_filter_click: Option<(u16, u16, u16)>,
    pub page_cols_click: Option<(u16, u16, u16)>,
    pub page_sort_click: Option<(u16, u16, u16)>,
    /// Which repo-page branch columns are shown (persisted).
    pub repo_page_columns: RepoPageColumns,
    /// Which optional Stashes-tab columns are shown — age / stats (persisted).
    pub repo_page_stash_columns: RepoPageStashColumns,
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
    /// The branch-checkout picker (kebab → "Checkout branch…"), when open.
    pub branch_picker: Option<BranchPicker>,
    pub branch_picker_area: Rect,
    pub branch_picker_close_click: Option<(u16, u16, u16)>,
    /// Branch-picker rows: (screen row, index into the *filtered* branch list).
    pub branch_picker_click: Vec<(u16, usize)>,
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
    /// How long the last build took, in seconds (from the `.polygit.build` sidecar `make` writes
    /// beside the installed binary) — shown in the build-info modal. `None` if the sidecar is absent.
    pub build_duration: Option<u64>,
    /// The build-info modal's outer rect (for outside-click-to-close).
    pub build_info_area: Rect,
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
    /// The preview viewport height captured by the last render, so keyboard nav can keep the
    /// selection in view without recomputing geometry (analog of `list_rows_area.height`).
    pub build_info_viewport: usize,
    /// The settings preview parsed into a collapsible tree (`None` if it isn't valid JSON — then the
    /// raw `build_info_settings_preview` lines render instead). Built when the modal opens.
    pub build_info_tree: Option<crate::treeview::DataNode>,
    /// Expanded container paths in the settings tree (everything else is collapsed by default).
    pub build_info_tree_expanded: std::collections::HashSet<String>,
    /// Selected row in the settings tree (index into the flattened visible rows).
    pub build_info_tree_selected: usize,
    /// Clickable container-row regions in the settings tree: (row, col_start, col_end, row index).
    pub build_info_tree_click: Vec<(u16, u16, u16, usize)>,
    /// The settings-tree fold-all / unfold-all hint-button regions.
    pub build_info_fold_all_click: Option<(u16, u16, u16)>,
    pub build_info_unfold_all_click: Option<(u16, u16, u16)>,
    // Changelog / What's New modal (the `vX.Y.Z` status-bar tag opens the changelog; an update
    // pops the What's New view — releases newer than the last-seen version, all expanded):
    /// The changelog modal is open.
    pub show_changelog: bool,
    /// In "What's New" mode (filtered to releases since `whats_new_since`, all expanded) vs the full
    /// changelog (every release, accordion, the latest two expanded).
    pub changelog_whats_new: bool,
    /// The version last run — releases above it are "new" in the What's New view. Empty on first run.
    pub whats_new_since: String,
    /// Collapsed release versions in the full changelog accordion.
    pub changelog_collapsed: std::collections::HashSet<String>,
    /// Selected release (index into the release list) for keyboard fold/nav in the full changelog.
    pub changelog_selected: usize,
    pub changelog_scroll: usize,
    pub changelog_area: Rect,
    pub changelog_close_click: Option<(u16, u16, u16)>,
    /// Clickable accordion-header regions: (row, col_start, col_end, release index).
    pub changelog_header_click: Vec<(u16, u16, u16, usize)>,
    /// One-shot: scroll to keep the selected release in view on the next render. Set by selection
    /// moves (j/k/g/G), expand/collapse, and header clicks — NOT by the wheel — so wheel scrolling
    /// is free (web-app style, like the main list) instead of snapping back to the selection.
    pub changelog_ensure_visible: bool,
    /// Settings modal: snap the view to the selected setting on the NEXT render. Set by keyboard
    /// nav / value changes (and on open / layout switch), consumed once per render — NOT by the
    /// wheel, so wheel scrolling is free (web-app style: scroll the container; a keyboard command
    /// scrolls back to the selected setting).
    pub settings_ensure_visible: bool,
    /// Maximize ⇄ restore the changelog / What's New / version-picker modal (runtime-only, like the
    /// help modal): `true` fills ~90% of the viewport. `m` or its title-bar button toggles it.
    pub changelog_maximized: bool,
    /// The modal's `[m maximize]`/`[m restore]` title-bar button region.
    pub changelog_maximize_click: Option<(u16, u16, u16)>,
    // Version picker (build-info → "pin version"): the changelog modal in a pin sub-mode that
    // lists live releases and installs a chosen one over the running binary, then auto-reloads.
    /// Pin sub-mode of the changelog modal is active.
    pub changelog_pin_mode: bool,
    /// The live release list (merged with embedded notes), newest-first. Populated by the fetch.
    pub pin_releases: Vec<PinRelease>,
    /// Show pre-floor (no in-app switch) versions too. Off by default; the `a` toggle flips it.
    pub pin_show_all: bool,
    /// The release list is being fetched.
    pub pin_releases_loading: bool,
    /// A fetch/download/install error to surface inline in the picker.
    pub pin_error: Option<String>,
    /// Transient status while a pin is downloading/installing (e.g. "downloading v2.50.0…").
    pub pin_status: Option<String>,
    /// Keyboard selection into the *visible* (filtered) picker rows.
    pub pin_selected: usize,
    /// Clickable `[pin]` regions: (row, col_start, col_end, version).
    pub pin_row_click: Vec<(u16, u16, u16, String)>,
    /// Clickable release-header regions in the picker: (row, col_start, col_end, visible index).
    /// Clicking selects + expands that release (accordion).
    pub pin_header_click: Vec<(u16, u16, u16, usize)>,
    /// The `show older / hide older` toggle's click region.
    pub pin_toggle_click: Option<(u16, u16, u16)>,
    /// Set by the worker after a successful install; the event loop re-execs into the new binary.
    pub pin_auto_reload: bool,
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
    /// How the pane splitters are presented: a dedicated 1-cell lane vs a thin on-hover grip
    /// (persisted, default dedicated).
    pub splitter_mode: SplitterMode,
    /// Post-change attention indicator on changed cells: off / flash / highlight (persisted).
    pub changed_row_effect: ChangedRowEffect,
    /// Per-area tooltip enablement (master + footer/headers/counts/settings/links). Persisted,
    /// all default on. Tooltips still require `hover_effects` (for cursor tracking).
    pub tooltips: TooltipPrefs,
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

/// The PR viewer modal's state: which repo+PR it shows, the data (None while loading), the remote
/// URL (for "open in browser"), the scroll offset, which sections are collapsed, and the live search
/// query. Rendered at a fixed ~90% size like the diff modal; content re-wraps to the width per frame.
pub struct PrModalState {
    pub repo_idx: usize,
    pub number: u32,
    pub url: String,
    pub title: String,
    /// The structured PR data, or `None` while the `gh pr view` fetch is in flight.
    pub view: Option<crate::app::PrView>,
    pub scroll: usize,
    /// Collapsed section indices: 0 = Description, 1.. = `view.comments[idx-1]`.
    pub collapsed: std::collections::HashSet<usize>,
    /// Live search query (filters + highlights sections); empty = no filter.
    pub search: String,
    /// Whether the search box has focus (typing edits the query).
    pub search_focused: bool,
}

impl PrModalState {
    /// Total collapsible sections: Description (always) + one per comment/review.
    pub fn section_count(&self) -> usize {
        1 + self.view.as_ref().map_or(0, |view| view.comments.len())
    }

    /// Whether section `idx` is currently collapsed.
    pub fn is_collapsed(&self, idx: usize) -> bool {
        self.collapsed.contains(&idx)
    }

    /// Toggle one section's collapsed state.
    pub fn toggle_section(&mut self, idx: usize) {
        if !self.collapsed.remove(&idx) {
            self.collapsed.insert(idx);
        }
    }

    /// True when every section is collapsed (drives the expand/collapse-all label).
    pub fn all_collapsed(&self) -> bool {
        (0..self.section_count()).all(|idx| self.collapsed.contains(&idx))
    }

    /// Collapse or expand every section at once.
    pub fn set_all_collapsed(&mut self, collapse: bool) {
        self.collapsed.clear();
        if collapse {
            self.collapsed.extend(0..self.section_count());
        }
    }
}

impl AppState {
    pub fn new(repos: Vec<SharedRepoState>, max_jobs: usize, auto_dark: bool) -> Self {
        // Restore persisted UI preferences (columns, info state, splitter), falling back to
        // defaults for anything missing or invalid.
        let persisted = crate::persist::load();
        // "What's New" pops when this build is newer than the version last run (skipped on first
        // run, when there's nothing to compare against).
        let prev_seen_version = persisted.last_seen_version.clone();
        let show_whats_new = !prev_seen_version.is_empty()
            && crate::changelog::version_cmp(env!("CARGO_PKG_VERSION"), &prev_seen_version)
                == std::cmp::Ordering::Greater;
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
            list_footer_area: Rect::default(),
            pr_cell_click: Vec::new(),
            fav_cell_click: Vec::new(),
            header_area: Rect::default(),
            header_click: Vec::new(),
            preview_area: Rect::default(),
            preview_total: 0,
            preview_viewport: 0,
            preview_scroll_area: Rect::default(),
            info_area: Rect::default(),
            info_total: 0,
            info_viewport: 0,
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
            help_tab_persist: persisted.help_tab,
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
                use_short: vec![false; CLI_FLAGS.len()],
                editing: None,
                help_mode: persisted.cli_help_mode,
            },
            cli_flag_click: Vec::new(),
            cli_copy_click: None,
            cli_helpmode_click: Vec::new(),
            cli_command_click: Vec::new(),
            help_tab_click: Vec::new(),
            help_close_click: None,
            help_design_click: Vec::new(),
            help_preview_click: None,
            design_layout: persisted.design_layout,
            design_section: 0,
            help_design_tab_click: Vec::new(),
            show_keyboard: false,
            keyboard_selected: None,
            keyboard_mods: (false, false, false),
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
            // Only the repo page's maximize is sticky; restore it from the legacy persisted bool.
            maximized: persisted.repo_page_maximized.then_some(Pane::RepoPage),
            repo_page_maximized_tabbed: persisted.repo_page_maximized_tabbed,
            repo_page_tabbed_override: None,
            repo_page_collapsed_sections: persisted.repo_page_collapsed_sections.into_iter().collect(),
            repo_page_section_click: Vec::new(),
            branch_check: persisted.branch_check,
            repo_page_tab: RepoTab::Branches,
            repo_page_tab_click: Vec::new(),
            repo_page_focus_head: false,
            repo_page_scroll: 0,
            repo_page_message: None,
            confirm: None,
            confirm_copy_click: None,
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
            diff_view: persisted.diff_view,
            show_merged_prs: persisted.show_merged_prs,
            diff_modal_viewport: 0,
            diff_files_viewport: 0,
            diff_files_area: Rect::default(),
            diff_body_area: Rect::default(),
            pr_modal: None,
            pr_modal_area: Rect::default(),
            pr_modal_close_click: None,
            pr_section_click: Vec::new(),
            pr_collapse_all_click: None,
            pr_search_click: None,
            root_dirs: Vec::new(),
            workspaces,
            active_workspace: None,
            panel_padding: persisted.panel_padding,
            icon_style: persisted.icon_style,
            hide_zero_counts: persisted.hide_zero_counts,
            hide_folder_lines: persisted.hide_folder_lines,
            claude_agent: persisted.claude_agent,
            claude_skip_permissions: persisted.claude_skip_permissions,
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
            kebab: None,
            kebab_area: Rect::default(),
            kebab_click: Vec::new(),
            kebab_close_click: None,
            kebab_open_click: Vec::new(),
            kebab_session_prefix: persisted.kebab_session_prefix,
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
            diff_modal_area: Rect::default(),
            diff_modal_close_click: None,
            diff_chips_click: Vec::new(),
            help_area: Rect::default(),
            repo_page_back_click: None,
            repo_page_window_click: None,
            max_click: Vec::new(),
            repo_page_pr_click: None,
            dropdown: None,
            dropdown_area: Rect::default(),
            dropdown_close_click: None,
            dropdown_item_click: Vec::new(),
            dropdown_action_click: Vec::new(),
            list_cols_click: None,
            list_sort_click: None,
            list_filter_click: None,
            page_cols_click: None,
            page_sort_click: None,
            repo_page_columns: persisted.repo_page_columns,
            repo_page_stash_columns: persisted.repo_page_stash_columns,
            repo_page_sort: None,
            repo_page_sort_dir: SortDir::Asc,
            repo_page_sort_click: Vec::new(),
            repo_page_info: persisted.repo_page_info,
            base_picker: None,
            base_picker_area: Rect::default(),
            base_picker_close_click: None,
            base_picker_click: Vec::new(),
            branch_picker: None,
            branch_picker_area: Rect::default(),
            branch_picker_close_click: None,
            branch_picker_click: Vec::new(),
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
            build_duration: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join(".polygit.build")))
                .and_then(|path| std::fs::read_to_string(path).ok())
                .and_then(|text| text.trim().parse::<u64>().ok()),
            build_info_area: Rect::default(),
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
            build_info_viewport: 0,
            build_info_tree: None,
            build_info_tree_expanded: std::collections::HashSet::new(),
            build_info_tree_selected: 0,
            build_info_tree_click: Vec::new(),
            build_info_fold_all_click: None,
            build_info_unfold_all_click: None,
            // Pop "What's New" when this build is newer than the one last run (e.g. after a reload).
            show_changelog: show_whats_new,
            changelog_whats_new: show_whats_new,
            whats_new_since: prev_seen_version,
            changelog_collapsed: std::collections::HashSet::new(),
            changelog_selected: 0,
            changelog_scroll: 0,
            changelog_area: Rect::default(),
            changelog_close_click: None,
            changelog_header_click: Vec::new(),
            changelog_ensure_visible: true,
            settings_ensure_visible: true,
            changelog_maximized: false,
            changelog_maximize_click: None,
            changelog_pin_mode: false,
            pin_releases: Vec::new(),
            pin_show_all: false,
            pin_releases_loading: false,
            pin_error: None,
            pin_status: None,
            pin_selected: 0,
            pin_row_click: Vec::new(),
            pin_header_click: Vec::new(),
            pin_toggle_click: None,
            pin_auto_reload: false,
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
            splitter_mode: persisted.splitter_mode,
            changed_row_effect: persisted.changed_row_effect,
            tooltips: persisted.tooltips,
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
