//! User-remappable keyboard shortcuts. The big `match key.code` blocks in `main.rs` stay intact;
//! this module sits *in front* of them as a translation layer:
//!
//!   pressed chord ──resolve(context)──▶ Rewrite(canonical key) │ Swallow │ Passthrough
//!
//! Every remappable action has a fixed *canonical* key event that the existing match already
//! recognizes (e.g. `OpenFinder` → Ctrl+P, `NavBottom` → `G`). A user binding maps an arbitrary
//! chord to an action; we look the pressed chord up and rewrite it to that action's canonical key,
//! so the unchanged match runs the right arm. A pressed chord that is a *default* key for some
//! action but is no longer bound to it (the user moved it) is **swallowed** (no-op), so unbinding
//! actually frees the key. Anything else passes through untouched.
//!
//! Overrides persist to `~/.config/polygit/keybindings.json` as `{ "<action-id>": ["<chord>", …] }`
//! — only actions whose chord set differs from the default are written, so the file stays small and
//! new default keys in later versions still reach old config files.

use std::collections::HashMap;
use std::sync::OnceLock;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

/// Which top-level handler a chord is resolved against. `Global` actions resolve in every context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Context {
    /// The main two-pane dashboard ("Normal key handling").
    List,
    /// The dedicated repo page (panel 4).
    RepoPage,
    /// Works from both List and RepoPage (help, settings, finder, pane focus, build-notice).
    Global,
}

/// A normalized key chord. For `Char` keys the shift state is folded into the character itself
/// (`G` already implies shift, `?` already implies the symbol), matching how the `match` arms read
/// — so we only track Ctrl/Alt there. For named keys (arrows, Enter, …) shift is explicit, and
/// `BackTab` is normalized to Shift+Tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyChord {
    /// Normalize a live key event into a chord (the form used for lookups + display).
    pub fn from_event(event: &KeyEvent) -> KeyChord {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let alt = event.modifiers.contains(KeyModifiers::ALT);
        let shift = event.modifiers.contains(KeyModifiers::SHIFT);
        match event.code {
            // Shift is carried by the character's case/symbol — don't double-count it.
            KeyCode::Char(ch) => KeyChord { code: KeyCode::Char(ch), ctrl, alt, shift: false },
            // Shift+Tab arrives as its own keycode; normalize so it round-trips as "shift+tab".
            KeyCode::BackTab => KeyChord { code: KeyCode::Tab, ctrl, alt, shift: true },
            other => KeyChord { code: other, ctrl, alt, shift },
        }
    }

    /// The key event a binding to this chord should produce. Inverse of `from_event` enough for the
    /// match arms: a shift+Tab chord becomes `BackTab`, characters carry no shift modifier.
    pub fn to_event(self) -> KeyEvent {
        let mut mods = KeyModifiers::NONE;
        if self.ctrl {
            mods |= KeyModifiers::CONTROL;
        }
        if self.alt {
            mods |= KeyModifiers::ALT;
        }
        match self.code {
            KeyCode::Tab if self.shift => KeyEvent::new(KeyCode::BackTab, mods),
            KeyCode::Char(ch) => KeyEvent::new(KeyCode::Char(ch), mods),
            other => {
                if self.shift {
                    mods |= KeyModifiers::SHIFT;
                }
                KeyEvent::new(other, mods)
            }
        }
    }

    /// Human + config string, e.g. `ctrl+p`, `G`, `/`, `shift+enter`, `backtab`, `space`.
    pub fn display(self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("ctrl+");
        }
        if self.alt {
            out.push_str("alt+");
        }
        if self.shift {
            out.push_str("shift+");
        }
        out.push_str(&code_token(self.code));
        out
    }

    /// Parse a config/display string back into a chord. Returns `None` for tokens we can't place.
    pub fn parse(text: &str) -> Option<KeyChord> {
        let mut rest = text.trim();
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        loop {
            let lower = rest.to_ascii_lowercase();
            if let Some(stripped) = lower.strip_prefix("ctrl+") {
                ctrl = true;
                rest = &rest[rest.len() - stripped.len()..];
            } else if let Some(stripped) = lower.strip_prefix("alt+") {
                alt = true;
                rest = &rest[rest.len() - stripped.len()..];
            } else if let Some(stripped) = lower.strip_prefix("shift+") {
                shift = true;
                rest = &rest[rest.len() - stripped.len()..];
            } else {
                break;
            }
        }
        let mut code = token_to_code(rest)?;
        // Fold shift into the character for single-char tokens, so `shift+g` == `G` and a live
        // Shift+G event (delivered as `Char('G')`) matches a parsed binding either way.
        if let KeyCode::Char(ch) = code {
            if shift && ch.is_ascii_lowercase() {
                code = KeyCode::Char(ch.to_ascii_uppercase());
            }
            shift = false;
        }
        Some(KeyChord { code, ctrl, alt, shift })
    }
}

/// Render a keycode as its config token (no modifiers).
fn code_token(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "backtab".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pgup".to_string(),
        KeyCode::PageDown => "pgdn".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        _ => String::new(),
    }
}

/// Parse a bare key token (already stripped of modifiers) into a keycode.
fn token_to_code(token: &str) -> Option<KeyCode> {
    let lower = token.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" => Some(KeyCode::Char(' ')),
        "up" => Some(KeyCode::Up),
        "down" => Some(KeyCode::Down),
        "left" => Some(KeyCode::Left),
        "right" => Some(KeyCode::Right),
        "enter" | "return" => Some(KeyCode::Enter),
        "esc" | "escape" => Some(KeyCode::Esc),
        "tab" => Some(KeyCode::Tab),
        "backtab" => Some(KeyCode::BackTab),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        "pgup" | "pageup" => Some(KeyCode::PageUp),
        "pgdn" | "pagedown" => Some(KeyCode::PageDown),
        "delete" | "del" => Some(KeyCode::Delete),
        "insert" | "ins" => Some(KeyCode::Insert),
        "backspace" => Some(KeyCode::Backspace),
        _ => None,
    };
    if let Some(code) = named {
        return Some(code);
    }
    if let Some(rest) = lower.strip_prefix('f') {
        if let Ok(n) = rest.parse::<u8>() {
            if (1..=12).contains(&n) {
                return Some(KeyCode::F(n));
            }
        }
    }
    // A single character (letter, digit, or symbol). Use the ORIGINAL token so case is preserved.
    let mut chars = token.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        return Some(KeyCode::Char(first));
    }
    None
}

/// Every remappable action. Names are stable — the `id()` string is the JSON key, so renaming a
/// variant requires keeping its id. New variants ripple to `ACTION_DEFS` (and only there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyAction {
    // ── List view ──────────────────────────────────────────────────────────────────────────────
    NavDown,
    NavUp,
    NavLeft,
    NavRight,
    NavTop,
    NavBottom,
    ToggleResultOverlay,
    ViewLeader,
    FoldLeader,
    CollapseAll,
    ExpandAll,
    ExpandSubtree,
    RefreshGroups,
    ColumnsDropdown,
    SortDropdown,
    FilterDropdown,
    SplitNarrow,
    SplitWiden,
    NameFilter,
    MaximizePane,
    ToggleFavorite,
    FavoritesFirst,
    Kebab,
    OpenFinder,
    OpenPr,
    OpenPrWeb,
    AddFolder,
    RemoveRoot,
    ClearLog,
    ToggleInfo,
    ToggleResultPanel,
    CycleInfoLayout,
    CycleResultView,
    OpenRemote,
    OpenDocs,
    CopyPath,
    CopyRemote,
    Claude,
    Lazygit,
    OpenPageOrToggle,
    Retry,
    RetryAll,
    Refetch,
    RefetchAll,
    RefreshFacts,
    RefreshFactsAll,
    // ── Global (List + RepoPage) ────────────────────────────────────────────────────────────────
    Help,
    Settings,
    OpenKeybindings,
    OpenExplorer,
    FocusList,
    FocusInfo,
    FocusResult,
    FocusRepoPage,
    // ── Repo page ───────────────────────────────────────────────────────────────────────────────
    PageMaximize,
    PageColumns,
    PageSort,
    PageToggleInfo,
    PageNavDown,
    PageNavUp,
    PageNavTop,
    PageNavBottom,
    PageCycleTab,
    PageCycleTabBack,
    PageToggleView,
    PageFoldSection,
    PageFoldAll,
    PageOpenDiff,
    PageCheckout,
    PageDeleteRow,
    PageClaude,
    PageLazygit,
    PageCopyMenu,
    PageBasePicker,
    PageOpenBranchRemote,
    PagePullBranch,
    PagePullAll,
}

/// A static descriptor for one action: how it's labeled in the editor, which handler it belongs to,
/// the canonical key the `match` recognizes, and its built-in default chords.
pub struct ActionDef {
    pub action: KeyAction,
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub context: Context,
    /// The key event the resolver rewrites a binding into. `(code, ctrl, alt, shift)`.
    canonical: (KeyCode, bool, bool, bool),
    /// Default chord strings (e.g. `&["j", "down"]`).
    defaults: &'static [&'static str],
}

impl ActionDef {
    /// The canonical key event for this action (what the unchanged match arm expects).
    pub fn canonical_event(&self) -> KeyEvent {
        let (code, ctrl, alt, shift) = self.canonical;
        KeyChord { code, ctrl, alt, shift }.to_event()
    }

    /// Parsed default chords.
    pub fn default_chords(&self) -> Vec<KeyChord> {
        self.defaults.iter().filter_map(|token| KeyChord::parse(token)).collect()
    }
}

macro_rules! action_defs {
    ( $( ($action:ident, $id:literal, $label:literal, $group:literal, $ctx:ident, $canon:expr, $defaults:expr) ),* $(,)? ) => {
        vec![ $( ActionDef {
            action: KeyAction::$action,
            id: $id,
            label: $label,
            group: $group,
            context: Context::$ctx,
            canonical: $canon,
            defaults: $defaults,
        } ),* ]
    };
}

const C: fn(char) -> KeyCode = KeyCode::Char;

/// The full action table — the single source of truth for labels, defaults, and canonical keys.
pub fn action_defs() -> &'static [ActionDef] {
    static DEFS: OnceLock<Vec<ActionDef>> = OnceLock::new();
    DEFS.get_or_init(build_action_defs)
}

#[rustfmt::skip]
fn build_action_defs() -> Vec<ActionDef> {
    use KeyCode::*;
    action_defs![
        // List — navigate
        (NavDown,             "nav_down",            "Next repo",                       "Navigate",     List, (C('j'), false, false, false), &["j", "down"]),
        (NavUp,               "nav_up",              "Previous repo",                   "Navigate",     List, (C('k'), false, false, false), &["k", "up"]),
        (NavLeft,             "nav_left",            "Collapse / jump to parent",       "Navigate",     List, (Left,   false, false, false), &["left"]),
        (NavRight,            "nav_right",           "Expand folder / group",           "Navigate",     List, (Right,  false, false, false), &["right"]),
        (NavTop,              "nav_top",             "Jump to top",                     "Navigate",     List, (C('g'), false, false, false), &["g"]),
        (NavBottom,           "nav_bottom",          "Jump to bottom",                  "Navigate",     List, (C('G'), false, false, false), &["G"]),
        // List — folding & views
        (ToggleResultOverlay, "result_overlay",      "Toggle Result summary / fold",    "Folding & views", List, (C(' '), false, false, false), &["space"]),
        (ViewLeader,          "view_leader",         "View leader (vg groups · vt tree)", "Folding & views", List, (C('v'), false, false, false), &["v"]),
        (FoldLeader,          "fold_leader",         "Fold leader (za/zo/zc/zO/zM/zR)", "Folding & views", List, (C('z'), false, false, false), &["z"]),
        (CollapseAll,         "collapse_all",        "Collapse all folders & groups",   "Folding & views", List, (C('-'), false, false, false), &["-"]),
        (ExpandAll,           "expand_all",          "Expand all folders & groups",     "Folding & views", List, (C('='), false, false, false), &["=", "+"]),
        (ExpandSubtree,       "expand_subtree",      "Expand selected subtree",         "Folding & views", List, (C('*'), false, false, false), &["*"]),
        (RefreshGroups,       "refresh_groups",      "Refresh dynamic group members",   "Folding & views", List, (C('Z'), false, false, false), &["Z"]),
        // List — find & sort
        (ColumnsDropdown,     "columns_dropdown",    "Open the columns dropdown",       "Find & sort",  List, (C('t'), false, false, false), &["t"]),
        (SortDropdown,        "sort_dropdown",       "Open the sort dropdown",          "Find & sort",  List, (C('s'), false, false, false), &["s"]),
        (FilterDropdown,      "filter_dropdown",     "Open the status-filter dropdown", "Find & sort",  List, (C('f'), false, false, false), &["f"]),
        (NameFilter,          "name_filter",         "Fuzzy-filter repos by name",      "Find & sort",  List, (C('/'), false, false, false), &["/"]),
        (OpenFinder,          "open_finder",         "Open the fuzzy finder overlay",   "Find & sort",  List, (C('p'), true,  false, false), &["ctrl+p"]),
        // List — panes & layout
        (SplitNarrow,         "split_narrow",        "Narrow the left pane",            "Panes & views", List, (C('['), false, false, false), &["["]),
        (SplitWiden,          "split_widen",         "Widen the left pane",             "Panes & views", List, (C(']'), false, false, false), &["]"]),
        (MaximizePane,        "maximize_pane",       "Maximize / restore the pane",     "Panes & views", List, (C('m'), false, false, false), &["m"]),
        (ToggleInfo,          "toggle_info",         "Toggle the info panel",           "Panes & views", List, (C('i'), false, false, false), &["i"]),
        (ToggleResultPanel,   "toggle_result_panel", "Toggle the result/log panel",     "Panes & views", List, (C('I'), false, false, false), &["I"]),
        (CycleInfoLayout,     "cycle_info_layout",   "Cycle the info-panel layout",     "Panes & views", List, (C('L'), false, false, false), &["L"]),
        (CycleResultView,     "cycle_result_view",   "Cycle result view (log/diff)",    "Panes & views", List, (C('d'), false, false, false), &["d"]),
        // List — pull / retry
        (Retry,               "retry",               "Retry the selected repo",         "Pull / retry", List, (C('r'), false, false, false), &["r"]),
        (RetryAll,            "retry_all",           "Retry all repos with an issue",   "Pull / retry", List, (C('R'), false, false, false), &["R"]),
        (Refetch,             "refetch",             "Refetch the selected repo",       "Pull / retry", List, (C('e'), false, false, false), &["e"]),
        (RefetchAll,          "refetch_all",         "Refetch all repos",               "Pull / retry", List, (C('E'), false, false, false), &["E"]),
        (RefreshFacts,        "refresh_facts",       "Refresh selected repo's git facts", "Pull / retry", List, (C('u'), false, false, false), &["u"]),
        (RefreshFactsAll,     "refresh_facts_all",   "Refresh all repos' git facts",    "Pull / retry", List, (C('U'), false, false, false), &["U"]),
        // List — favorites
        (ToggleFavorite,      "toggle_favorite",     "Toggle the selected favorite",    "Favorites & menu", List, (C('b'), false, false, false), &["b"]),
        (FavoritesFirst,      "favorites_first",     "Pin Favorites to the top",        "Favorites & menu", List, (C('B'), false, false, false), &["B"]),
        (Kebab,               "kebab",               "Open the repo kebab (⋮) menu",    "Favorites & menu", List, (C('.'), false, false, false), &["."]),
        // List — clipboard & run
        (OpenRemote,          "open_remote",         "Open the repo remote in browser", "Clipboard & run", List, (C('o'), false, false, false), &["o"]),
        (OpenPr,              "open_pr",             "Open the PR in the PR viewer",    "Clipboard & run", List, (C('p'), false, false, false), &["p"]),
        (OpenPrWeb,           "open_pr_web",         "Open the PR / compare on GitHub", "Clipboard & run", List, (C('P'), false, false, false), &["P"]),
        (CopyPath,            "copy_path",           "Copy the repo's absolute path",   "Clipboard & run", List, (C('y'), false, false, false), &["y"]),
        (CopyRemote,          "copy_remote",         "Copy the repo's remote URL",      "Clipboard & run", List, (C('Y'), false, false, false), &["Y"]),
        (Claude,              "claude",              "Launch the AI agent in the repo", "Clipboard & run", List, (C('c'), false, false, false), &["c"]),
        (Lazygit,             "lazygit",             "Open lazygit in the repo",        "Clipboard & run", List, (C('l'), false, false, false), &["l"]),
        (ClearLog,            "clear_log",           "Clear the repo's log buffer",     "Clipboard & run", List, (C('x'), false, false, false), &["x"]),
        (OpenPageOrToggle,    "open_page",           "Open the repo page / fold header","Navigate",        List, (Enter,  false, false, false), &["enter"]),
        // List — workspace
        (AddFolder,           "add_folder",          "Add a folder to the workspace",   "Workspace",    List, (C('A'), false, false, false), &["A"]),
        (RemoveRoot,          "remove_root",         "Remove the folder from workspace","Workspace",    List, (C('X'), false, false, false), &["X", "delete"]),

        // Global (List + RepoPage)
        (Help,                "help",                "Open the help modal",             "App & modals", Global, (C('?'), false, false, false), &["?"]),
        (Settings,            "settings",            "Open the settings modal",         "App & modals", Global, (C(','), false, false, false), &[","]),
        (OpenKeybindings,     "open_keybindings",    "Open the keybindings editor",     "App & modals", Global, (C('k'), true,  false, false), &["ctrl+k"]),
        (OpenExplorer,        "open_explorer",       "Open the file explorer for the repo","Clipboard & run", List, (C('e'), true,  false, false), &["ctrl+e"]),
        (OpenDocs,            "open_docs",           "Open the docs website",           "App & modals", Global, (C('D'), false, false, false), &["D"]),
        (FocusList,           "focus_list",          "Focus / maximize the list pane",  "Panes & views", Global, (C('1'), false, false, false), &["1"]),
        (FocusInfo,           "focus_info",          "Focus / maximize the info pane",  "Panes & views", Global, (C('2'), false, false, false), &["2"]),
        (FocusResult,         "focus_result",        "Focus / maximize the result pane","Panes & views", Global, (C('3'), false, false, false), &["3"]),
        (FocusRepoPage,       "focus_repo_page",     "Focus / maximize the repo page",  "Panes & views", Global, (C('4'), false, false, false), &["4"]),

        // Repo page
        (PageMaximize,        "page_maximize",       "Maximize / restore the repo page","Repo page · panes", RepoPage, (C('m'), false, false, false), &["m"]),
        (PageColumns,         "page_columns",        "Open the columns dropdown",       "Repo page · columns", RepoPage, (C('t'), false, false, false), &["t"]),
        (PageSort,            "page_sort",           "Open the sort dropdown",          "Repo page · columns", RepoPage, (C('s'), false, false, false), &["s"]),
        (PageToggleInfo,      "page_toggle_info",    "Toggle the bottom info panel",    "Repo page · columns", RepoPage, (C('i'), false, false, false), &["i"]),
        (PageNavDown,         "page_nav_down",       "Next row",                        "Repo page · navigate", RepoPage, (C('j'), false, false, false), &["j", "down"]),
        (PageNavUp,           "page_nav_up",         "Previous row",                    "Repo page · navigate", RepoPage, (C('k'), false, false, false), &["k", "up"]),
        (PageNavTop,          "page_nav_top",        "First row",                       "Repo page · navigate", RepoPage, (C('g'), false, false, false), &["g"]),
        (PageNavBottom,       "page_nav_bottom",     "Last row",                        "Repo page · navigate", RepoPage, (C('G'), false, false, false), &["G"]),
        (PageCycleTab,        "page_cycle_tab",      "Next repo-page tab",              "Repo page · panes", RepoPage, (Tab,    false, false, false), &["tab"]),
        (PageCycleTabBack,    "page_cycle_tab_back", "Previous repo-page tab",          "Repo page · panes", RepoPage, (Tab,    false, false, true),  &["shift+tab"]),
        (PageToggleView,      "page_toggle_view",    "Toggle tabbed / flat view",       "Repo page · panes", RepoPage, (C('v'), false, false, false), &["v"]),
        (PageFoldSection,     "page_fold_section",   "Fold the selected section",       "Repo page · panes", RepoPage, (C('z'), false, false, false), &["z"]),
        (PageFoldAll,         "page_fold_all",       "Fold / unfold all sections",      "Repo page · panes", RepoPage, (C('Z'), false, false, false), &["Z"]),
        (PageOpenDiff,        "page_open_diff",      "Open the diff modal",             "Repo page · rows", RepoPage, (Enter,  false, false, false), &["enter"]),
        (PageCheckout,        "page_checkout",       "Check out the selected branch",   "Repo page · rows", RepoPage, (Enter,  false, false, true),  &["shift+enter"]),
        (PageDeleteRow,       "page_delete_row",     "Delete branch / drop / discard",  "Repo page · rows", RepoPage, (C('d'), false, false, false), &["d"]),
        (PageClaude,          "page_claude",         "Launch the AI agent in the row",  "Repo page · rows", RepoPage, (C('c'), false, false, false), &["c"]),
        (PageLazygit,         "page_lazygit",        "Open lazygit in the row",         "Repo page · rows", RepoPage, (C('l'), false, false, false), &["l"]),
        (PageCopyMenu,        "page_copy_menu",      "Copy menu (path / branch / both)","Repo page · rows", RepoPage, (C('y'), false, false, false), &["y"]),
        (PageBasePicker,      "page_base_picker",    "Base-branch picker for the row",  "Repo page · rows", RepoPage, (C('b'), false, false, false), &["b"]),
        (PageOpenBranchRemote,"page_open_branch",    "Open the branch on the remote",   "Repo page · rows", RepoPage, (C('o'), false, false, false), &["o"]),
        (PagePullBranch,      "page_pull_branch",    "Fast-forward the selected branch","Repo page · rows", RepoPage, (C('p'), false, false, false), &["p"]),
        (PagePullAll,         "page_pull_all",       "Fast-forward every branch",       "Repo page · rows", RepoPage, (C('P'), false, false, false), &["P"]),
    ]
}

/// The editor's flattened rows: `None` = a group header, `Some(idx)` = the action at `action_defs()`
/// index `idx`. The editor's scroll position indexes into this list. Single source of truth shared by
/// the renderer and the scrollbar-drag handler so they agree on where each action sits.
pub fn flat_layout() -> Vec<Option<usize>> {
    let mut rows = Vec::new();
    let mut last_group = "";
    for (idx, def) in action_defs().iter().enumerate() {
        if def.group != last_group {
            rows.push(None);
            last_group = def.group;
        }
        rows.push(Some(idx));
    }
    rows
}

/// Look up an action's descriptor.
pub fn def_for(action: KeyAction) -> &'static ActionDef {
    static MAP: OnceLock<HashMap<KeyAction, usize>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        action_defs().iter().enumerate().map(|(idx, def)| (def.action, idx)).collect()
    });
    &action_defs()[map[&action]]
}

/// What the resolver decided to do with a pressed key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// The chord is bound — feed this key event to the existing match instead.
    Rewrite(KeyEvent),
    /// The chord is an unbound default key in this context — do nothing (the user freed it).
    Swallow,
    /// Not a remappable chord — let the existing match handle the original key.
    Passthrough,
}

/// The live keymap: resolved chord set per action (defaults with user overrides applied).
#[derive(Debug, Clone)]
pub struct Keybindings {
    /// action → its current chords (after overrides).
    chords: HashMap<KeyAction, Vec<KeyChord>>,
    /// Non-fatal load diagnostics (bad JSON, unknown ids, unparseable chords).
    pub load_error: Option<String>,
}

impl Default for Keybindings {
    fn default() -> Self {
        Keybindings::with_defaults()
    }
}

impl Keybindings {
    /// A fresh keymap with every action at its built-in default chords.
    pub fn with_defaults() -> Keybindings {
        let chords = action_defs()
            .iter()
            .map(|def| (def.action, def.default_chords()))
            .collect();
        Keybindings { chords, load_error: None }
    }

    /// The current chords bound to an action.
    pub fn chords_for(&self, action: KeyAction) -> &[KeyChord] {
        self.chords.get(&action).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Whether an action's chords differ from its defaults (i.e. it's an override worth persisting).
    pub fn is_overridden(&self, action: KeyAction) -> bool {
        let defaults = def_for(action).default_chords();
        let current = self.chords_for(action);
        // Order-insensitive comparison.
        current.len() != defaults.len()
            || !defaults.iter().all(|chord| current.contains(chord))
    }

    /// Resolve a pressed key in a given runtime context (List or RepoPage). Global actions are
    /// always considered. See the module doc for Rewrite/Swallow/Passthrough semantics.
    pub fn resolve(&self, context: Context, event: &KeyEvent) -> Resolution {
        let chord = KeyChord::from_event(event);
        let mut is_default_here = false;
        for def in action_defs() {
            if !context_matches(def.context, context) {
                continue;
            }
            if self.chords_for(def.action).contains(&chord) {
                return Resolution::Rewrite(def.canonical_event());
            }
            if def.default_chords().contains(&chord) {
                is_default_here = true;
            }
        }
        // The chord is a built-in default for some action in this context but nothing is bound to it
        // now (the user moved it elsewhere) → swallow so the freed key doesn't trigger old behavior.
        if is_default_here {
            Resolution::Swallow
        } else {
            Resolution::Passthrough
        }
    }

    /// The action (other than `action`) a chord would collide with if bound to `action`, if any.
    /// A `Global` action's keys fire in both List and RepoPage, so it's checked against both; a
    /// context action is checked against its own context (plus globals, via `owner`).
    pub fn conflict(&self, action: KeyAction, chord: KeyChord) -> Option<KeyAction> {
        let scopes: &[Context] = match def_for(action).context {
            Context::Global => &[Context::List, Context::RepoPage],
            Context::List => &[Context::List],
            Context::RepoPage => &[Context::RepoPage],
        };
        for &scope in scopes {
            if let Some(other) = self.owner(scope, chord) {
                if other != action {
                    return Some(other);
                }
            }
        }
        None
    }

    /// The action a chord is bound to within a context (incl. Global), if any — for conflict checks.
    pub fn owner(&self, context: Context, chord: KeyChord) -> Option<KeyAction> {
        action_defs()
            .iter()
            .filter(|def| context_matches(def.context, context))
            .find(|def| self.chords_for(def.action).contains(&chord))
            .map(|def| def.action)
    }

    /// Replace an action's binding with a single chord (the common "rebind to X" path).
    pub fn set_only(&mut self, action: KeyAction, chord: KeyChord) {
        self.chords.insert(action, vec![chord]);
    }

    /// Remove a chord from whichever action holds it (so reassigning can clear a conflict).
    pub fn unbind(&mut self, chord: KeyChord) {
        for chords in self.chords.values_mut() {
            chords.retain(|existing| *existing != chord);
        }
    }

    /// Clear all of an action's chords (the key does nothing until rebound).
    pub fn clear(&mut self, action: KeyAction) {
        self.chords.insert(action, Vec::new());
    }

    /// Reset one action to its default chords.
    pub fn reset(&mut self, action: KeyAction) {
        self.chords.insert(action, def_for(action).default_chords());
    }

    /// Reset every action to defaults.
    pub fn reset_all(&mut self) {
        for def in action_defs() {
            self.chords.insert(def.action, def.default_chords());
        }
    }

    /// Build the persisted override map: only actions whose chords differ from default.
    fn overrides(&self) -> HashMap<String, Vec<String>> {
        action_defs()
            .iter()
            .filter(|def| self.is_overridden(def.action))
            .map(|def| {
                let chords = self.chords_for(def.action).iter().map(|chord| chord.display()).collect();
                (def.id.to_string(), chords)
            })
            .collect()
    }

    /// Apply a parsed override map over defaults, collecting any diagnostics.
    fn apply_overrides(raw: HashMap<String, Vec<String>>) -> Keybindings {
        let id_map: HashMap<&str, KeyAction> =
            action_defs().iter().map(|def| (def.id, def.action)).collect();
        let mut keymap = Keybindings::with_defaults();
        let mut problems: Vec<String> = Vec::new();
        for (id, chord_strs) in raw {
            let Some(&action) = id_map.get(id.as_str()) else {
                problems.push(format!("unknown action '{id}'"));
                continue;
            };
            let mut chords = Vec::new();
            for token in &chord_strs {
                match KeyChord::parse(token) {
                    Some(chord) => chords.push(chord),
                    None => problems.push(format!("'{id}': bad key '{token}'")),
                }
            }
            keymap.chords.insert(action, chords);
        }
        if !problems.is_empty() {
            keymap.load_error = Some(problems.join("; "));
        }
        keymap
    }

    /// Under test, never read the user's real keybindings.json — keep unit tests hermetic.
    #[cfg(test)]
    pub fn load() -> Keybindings {
        Keybindings::with_defaults()
    }

    /// Load from `~/.config/polygit/keybindings.json` (missing file → defaults).
    #[cfg(not(test))]
    pub fn load() -> Keybindings {
        let Some(path) = keybindings_path() else {
            return Keybindings::with_defaults();
        };
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return Keybindings::with_defaults();
        };
        match serde_json::from_str::<HashMap<String, Vec<String>>>(&contents) {
            Ok(raw) => Keybindings::apply_overrides(raw),
            Err(err) => {
                let mut keymap = Keybindings::with_defaults();
                keymap.load_error = Some(format!("keybindings.json: {err}"));
                keymap
            }
        }
    }

    /// Under test, never touch the user's real keybindings.json.
    #[cfg(test)]
    pub fn save(&self) {}

    /// Persist overrides (best-effort). An all-defaults keymap removes the file.
    #[cfg(not(test))]
    pub fn save(&self) {
        let Some(path) = keybindings_path() else {
            return;
        };
        let overrides = self.overrides();
        if overrides.is_empty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(contents) = serde_json::to_string_pretty(&overrides) {
            let _ = std::fs::write(&path, contents);
        }
    }
}

/// Whether a definition's context applies in a runtime context. `Global` defs apply everywhere; a
/// runtime `Global` lookup (rare) only matches `Global` defs.
fn context_matches(def_context: Context, runtime: Context) -> bool {
    def_context == runtime || def_context == Context::Global
}

#[cfg_attr(test, allow(dead_code))]
fn keybindings_path() -> Option<std::path::PathBuf> {
    Some(crate::persist::config_dir()?.join("keybindings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn chord_round_trips_through_string() {
        let cases = ["ctrl+p", "g", "G", "/", "shift+enter", "backtab", "space", "delete", "alt+f4"];
        for case in cases {
            let chord = KeyChord::parse(case).unwrap_or_else(|| panic!("parse {case}"));
            assert_eq!(chord.display(), case, "round-trip {case}");
        }
    }

    #[test]
    fn uppercase_and_shift_letter_are_equal() {
        // `G` and `shift+g` normalize to the same chord (shift folded into the char).
        assert_eq!(KeyChord::parse("G"), KeyChord::parse("shift+g"));
        // And a live Shift+G event matches the parsed `G` chord.
        let pressed = KeyChord::from_event(&ev(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(pressed, KeyChord::parse("G").unwrap());
    }

    #[test]
    fn ctrl_p_event_matches_default_finder_binding() {
        let keymap = Keybindings::with_defaults();
        let event = ev(KeyCode::Char('p'), KeyModifiers::CONTROL);
        match keymap.resolve(Context::List, &event) {
            Resolution::Rewrite(out) => {
                assert_eq!(out.code, KeyCode::Char('p'));
                assert!(out.modifiers.contains(KeyModifiers::CONTROL));
            }
            other => panic!("expected rewrite, got {other:?}"),
        }
    }

    #[test]
    fn rebinding_finder_to_ctrl_f_resolves_and_frees_nothing() {
        // Kevin's exact ask: ctrl+f opens the finder.
        let mut keymap = Keybindings::with_defaults();
        keymap.set_only(KeyAction::OpenFinder, KeyChord::parse("ctrl+f").unwrap());
        let ctrl_f = ev(KeyCode::Char('f'), KeyModifiers::CONTROL);
        match keymap.resolve(Context::List, &ctrl_f) {
            Resolution::Rewrite(out) => {
                assert_eq!(out.code, KeyCode::Char('p'));
                assert!(out.modifiers.contains(KeyModifiers::CONTROL));
            }
            other => panic!("ctrl+f should open finder, got {other:?}"),
        }
        // Old ctrl+p is now an unbound default → swallowed (does nothing).
        let ctrl_p = ev(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(keymap.resolve(Context::List, &ctrl_p), Resolution::Swallow);
        // Plain `f` (the filter dropdown) is untouched — its own binding still resolves it.
        let plain_f = ev(KeyCode::Char('f'), KeyModifiers::NONE);
        assert!(matches!(keymap.resolve(Context::List, &plain_f), Resolution::Rewrite(_)));
    }

    #[test]
    fn unbound_chord_passes_through() {
        let keymap = Keybindings::with_defaults();
        // `9` isn't a default for anything → passthrough (the match's `_ => {}` ignores it).
        let event = ev(KeyCode::Char('9'), KeyModifiers::NONE);
        assert_eq!(keymap.resolve(Context::List, &event), Resolution::Passthrough);
    }

    #[test]
    fn context_separates_clashing_letters() {
        // `d` is CycleResultView in the list but DeleteRow on the repo page — same key, both bound.
        let keymap = Keybindings::with_defaults();
        let event = ev(KeyCode::Char('d'), KeyModifiers::NONE);
        let list = keymap.resolve(Context::List, &event);
        let page = keymap.resolve(Context::RepoPage, &event);
        assert!(matches!(list, Resolution::Rewrite(_)));
        assert!(matches!(page, Resolution::Rewrite(_)));
    }

    #[test]
    fn owner_detects_conflicts_within_context() {
        let keymap = Keybindings::with_defaults();
        // `j` is owned by NavDown in the list.
        assert_eq!(
            keymap.owner(Context::List, KeyChord::parse("j").unwrap()),
            Some(KeyAction::NavDown)
        );
        // ctrl+p (a List action) is visible from the RepoPage context only if global — it isn't,
        // so it's free there.
        assert_eq!(keymap.owner(Context::RepoPage, KeyChord::parse("ctrl+p").unwrap()), None);
        // Help is global → owned in both contexts.
        assert_eq!(
            keymap.owner(Context::RepoPage, KeyChord::parse("?").unwrap()),
            Some(KeyAction::Help)
        );
    }

    #[test]
    fn overrides_round_trip_and_skip_defaults() {
        let mut keymap = Keybindings::with_defaults();
        assert!(keymap.overrides().is_empty(), "fresh keymap has no overrides");
        keymap.set_only(KeyAction::OpenFinder, KeyChord::parse("ctrl+f").unwrap());
        let raw = keymap.overrides();
        assert_eq!(raw.get("open_finder"), Some(&vec!["ctrl+f".to_string()]));
        assert_eq!(raw.len(), 1, "only the changed action is persisted");
        // Re-applying the overrides reproduces the keymap.
        let restored = Keybindings::apply_overrides(raw);
        assert_eq!(restored.chords_for(KeyAction::OpenFinder), keymap.chords_for(KeyAction::OpenFinder));
        assert!(restored.load_error.is_none());
    }

    #[test]
    fn unknown_ids_and_bad_chords_are_reported_not_fatal() {
        let mut raw = HashMap::new();
        raw.insert("open_finder".to_string(), vec!["ctrl+f".to_string()]);
        raw.insert("bogus_action".to_string(), vec!["x".to_string()]);
        raw.insert("retry".to_string(), vec!["~~~".to_string(), "ctrl+y".to_string()]);
        let keymap = Keybindings::apply_overrides(raw);
        // Good override applied.
        assert_eq!(keymap.chords_for(KeyAction::OpenFinder), &[KeyChord::parse("ctrl+f").unwrap()]);
        // Bad chord dropped, good one kept.
        assert_eq!(keymap.chords_for(KeyAction::Retry), &[KeyChord::parse("ctrl+y").unwrap()]);
        // Diagnostics recorded.
        let error = keymap.load_error.expect("diagnostics present");
        assert!(error.contains("bogus_action"));
        assert!(error.contains("retry"));
    }

    #[test]
    fn every_default_chord_parses() {
        for def in action_defs() {
            for token in def.defaults {
                assert!(KeyChord::parse(token).is_some(), "default '{token}' for {} must parse", def.id);
            }
        }
    }

    #[test]
    fn action_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for def in action_defs() {
            assert!(seen.insert(def.id), "duplicate action id '{}'", def.id);
        }
    }

    #[test]
    fn reset_clears_overrides() {
        let mut keymap = Keybindings::with_defaults();
        keymap.set_only(KeyAction::OpenFinder, KeyChord::parse("ctrl+f").unwrap());
        assert!(keymap.is_overridden(KeyAction::OpenFinder));
        keymap.reset(KeyAction::OpenFinder);
        assert!(!keymap.is_overridden(KeyAction::OpenFinder));
        keymap.set_only(KeyAction::NavDown, KeyChord::parse("n").unwrap());
        keymap.reset_all();
        assert!(keymap.overrides().is_empty());
    }
}
