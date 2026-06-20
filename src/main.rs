mod app;
mod cache;
mod changelog;
mod git;
mod groups;
mod keymap;
mod persist;
mod plain;
mod pr_cache;
mod profile;
mod render;
mod theme;
mod treeview;
mod worker;

use std::io::{self, IsTerminal, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, KeyboardEnhancementFlags, MouseButton, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};
use std::collections::HashMap;

use app::{
    point_in, region_hit, AppState, Column, Command as Cmd, ConfirmAction, ConfirmDialog,
    DiffFocus, DiffSource, InfoAction, Leader, PageRow, PageRowKind, Pane, RepoPageColumn,
    RepoStatus, RightView, SharedRepoState, SortColumn, StatusFilter,
};
use worker::{
    run_all_details, run_branch_stats, run_checkout, run_delete, run_diff_modal,
    run_diff_modal_file, run_discard_changes, run_discovery, run_drop_stash, run_prepare_discard,
    run_prepare_drop_stash, run_pull_all_branches, run_pull_branch, run_refetch_batch,
    run_all_prs, run_pull_request, run_remove_worktree, run_repo_details, run_repo_diff,
    run_repo_page,
};

/// Current wall-clock time in Unix seconds (for status-cache timestamps). `0` if the clock is
/// before the epoch (never, in practice).
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Interactive polyrepo git dashboard — discover, status, and pull many repos.
#[derive(Parser, Debug)]
#[command(name = "polygit", version, about)]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    /// Subcommand (e.g. `ws` to manage workspaces); omit for the default scan.
    #[command(subcommand)]
    command: Option<Commands>,

    /// Directories to pull repos from — each may itself be a single repo. With none, scans the
    /// current directory. (Use `-w <name>` to open a saved workspace instead.)
    dirs: Vec<PathBuf>,

    /// Open a saved workspace by name. With DIRS, (re)defines that workspace as those folders;
    /// a new name with no DIRS starts from the cwd. The folder picker (`A`) edits the active one.
    #[arg(short = 'w', long, value_name = "NAME")]
    workspace: Option<String>,

    /// Maximum concurrent pulls (default: nproc)
    #[arg(short = 'j', long, env = "PULL_JOBS")]
    jobs: Option<usize>,

    /// Max directory depth to scan for repos (1 = immediate subdirs only)
    #[arg(long, value_name = "N", default_value = "16")]
    depth: usize,

    /// Scan only the immediate subdirectories (same as --depth 1)
    #[arg(long)]
    no_recursive: bool,

    /// Force plain streaming output (no TUI)
    #[arg(long)]
    no_tui: bool,

    /// Skip worktree discovery
    #[arg(long)]
    no_worktrees: bool,

    /// Per-pull timeout in seconds (default: 30)
    #[arg(long, env = "PULL_TIMEOUT", default_value = "30")]
    timeout: u64,

    /// Emit a per-repo timing report (slowest first) after the run
    #[arg(long)]
    profile: bool,

    /// Write the profile report to this file instead of stderr
    #[arg(long, value_name = "FILE")]
    profile_out: Option<PathBuf>,
}

/// Top-level subcommands. New commands slot in here; each gets its own `--help`/`help`.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Manage saved workspaces — opens an interactive picker; `ws ls` lists them.
    #[command(visible_aliases = ["workspace", "workspaces"])]
    Ws {
        #[command(subcommand)]
        action: Option<WsAction>,
    },
}

/// `ws` subcommands.
#[derive(Subcommand, Debug)]
enum WsAction {
    /// List saved workspaces and their folders
    #[command(visible_alias = "list")]
    Ls,
}

#[tokio::main]
async fn main() {
    let exit_code = run().await.unwrap_or_else(|err| {
        eprintln!("error: {err:#}");
        1
    });
    std::process::exit(exit_code);
}

/// Sentinel exit code from the event loop meaning "exec the new binary" (never reaches the OS:
/// `run_tui` intercepts it after restoring the terminal).
const RELOAD_EXIT: i32 = i32::MIN;

/// Spawn the worker for an accepted confirmation dialog (shared by the `y` key and the
/// clickable `[y/enter] yes` button).
fn spawn_confirm_action(app_state: &Arc<Mutex<AppState>>, action: ConfirmAction) {
    let app_state = Arc::clone(app_state);
    match action {
        ConfirmAction::DeleteBranch { repo_idx, branch, force } => {
            tokio::spawn(run_delete(app_state, repo_idx, branch, force));
        }
        ConfirmAction::DropStash { repo_idx, index } => {
            tokio::spawn(run_drop_stash(app_state, repo_idx, index));
        }
        ConfirmAction::RemoveWorktree { repo_idx, path, force } => {
            tokio::spawn(run_remove_worktree(app_state, repo_idx, path, force));
        }
        ConfirmAction::DiscardChanges { repo_idx, path } => {
            tokio::spawn(run_discard_changes(app_state, repo_idx, path));
        }
        ConfirmAction::ResetSettings => {
            let mut app = app_state.lock().unwrap();
            app.apply_settings_reset();
            app.show_toast("settings reset to defaults".to_string());
        }
        // The design-system preview: accepting just closes the dialog (already taken by the caller).
        ConfirmAction::Preview => {}
    }
}

/// Watch the running executable's path for a newer build (atomic-rename installs change the
/// file's mtime/length). On a change, raise the update notice; a fresh change re-arms a
/// dismissed one. Polling a single stat every few seconds is negligible.
async fn watch_for_new_build(app_state: Arc<Mutex<AppState>>) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Ok(meta) = tokio::fs::metadata(&exe).await else {
        return;
    };
    let mut last_seen = (meta.len(), meta.modified().ok());
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let Ok(meta) = tokio::fs::metadata(&exe).await else {
            continue; // mid-replace; the next tick sees the new file
        };
        let current = (meta.len(), meta.modified().ok());
        if current != last_seen && meta.len() > 0 {
            last_seen = current;
            let mut app = app_state.lock().unwrap();
            app.update_available = true;
            app.update_dismissed = false;
        }
    }
}

/// For the Auto theme, re-detect dark/light from the tty-safe sources every few seconds so an OS
/// light↔dark switch re-themes live (the render loop redraws every tick). Detection runs on a
/// blocking thread (it may shell out to `reg.exe`/`defaults`); the `AppState` lock is held only
/// to read `theme` and write `auto_dark`, never across `.await`.
async fn watch_theme(app_state: Arc<Mutex<AppState>>) {
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if app_state.lock().unwrap().theme != app::Theme::Auto {
            continue;
        }
        if let Ok(Some(dark)) =
            tokio::task::spawn_blocking(theme::detect_dark_background_runtime).await
        {
            app_state.lock().unwrap().auto_dark = dark;
        }
    }
}

/// Open a URL in the user's browser via the first available opener, detached.
fn open_url(url: &str) {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(browser) = std::env::var("BROWSER") {
        if !browser.is_empty() {
            candidates.push(browser);
        }
    }
    candidates.extend(["wslview", "xdg-open", "open"].map(String::from));

    for opener in candidates {
        let spawned = Command::new(&opener)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if spawned.is_ok() {
            return;
        }
    }
}

/// Copy text to the system clipboard via the first available tool, writing to its stdin.
/// `clip.exe` (Windows, reachable under WSL) is tried first and fed **UTF-16LE** — it otherwise
/// mangles non-ASCII (e.g. `•` → `ΓÇó`) because it reads stdin as the OEM code page. The Unix
/// tools take UTF-8.
fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    // (tool, args, encode_as_utf16le)
    let tools: [(&str, &[&str], bool); 4] = [
        ("clip.exe", &[], true),
        ("wl-copy", &[], false),
        ("xclip", &["-selection", "clipboard"], false),
        ("pbcopy", &[], false),
    ];
    for (tool, args, utf16le) in tools {
        let child = Command::new(tool)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(mut child) = child {
            if let Some(mut stdin) = child.stdin.take() {
                if utf16le {
                    let bytes: Vec<u8> =
                        text.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
                    let _ = stdin.write_all(&bytes);
                } else {
                    let _ = stdin.write_all(text.as_bytes());
                }
            }
            let _ = child.wait();
            return;
        }
    }
}

/// Suspend the TUI, run claude code in `path` (the `cc` alias by default, overridable via
/// `PULL_CLAUDE_CMD`), then restore the alternate screen and mouse capture.
fn launch_claude(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    path: &std::path::Path,
) -> Result<()> {
    let command = std::env::var("PULL_CLAUDE_CMD").unwrap_or_else(|_| "cc".to_string());

    pop_key_enhancement(terminal);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    // `-i` sources ~/.bashrc so the `cc` alias resolves; the path is passed as $1 to avoid quoting.
    let script = format!("cd \"$1\" && {command}");
    let _ = Command::new("bash")
        .args(["-ic", &script, "polygit"])
        .arg(path)
        .status();

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture)?;
    push_key_enhancement(terminal);
    terminal.clear()?;
    Ok(())
}

/// Whether `lazygit` is on `$PATH` (cheap `--version` probe, run only when `l` is pressed).
fn lazygit_available() -> bool {
    Command::new("lazygit")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Suspend the TUI, run `lazygit` in `path`, then restore the alternate screen and mouse capture
/// (mirrors `launch_claude`).
fn launch_lazygit(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    path: &std::path::Path,
) -> Result<()> {
    pop_key_enhancement(terminal);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    let _ = Command::new("lazygit").arg("--path").arg(path).status();

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture)?;
    push_key_enhancement(terminal);
    terminal.clear()?;
    Ok(())
}

/// Push the Kitty keyboard protocol flags when the terminal supports them, so modified keys
/// (notably Shift+Enter) are reported with their modifier instead of as a bare Enter, and bare
/// modifier presses (Shift/Ctrl/Alt/Super) arrive as their own key events for the keyboard viewer.
/// Best-effort — a no-op on terminals without support.
fn push_key_enhancement(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    if supports_keyboard_enhancement().unwrap_or(false) {
        let _ = execute!(
            terminal.backend_mut(),
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            )
        );
    }
}

/// The synthetic key event a clicked footer hint injects, so a click runs the same handler as
/// the keypress it mirrors.
fn hint_key_event(hint: app::HintKey) -> KeyEvent {
    match hint {
        app::HintKey::Char(ch) => KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
        app::HintKey::Enter => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        app::HintKey::ShiftEnter => KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
        app::HintKey::Tab => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        app::HintKey::Esc => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    }
}

/// Master-detail: while the restored repo page (panel [4]) is open and the list ([1]) holds focus,
/// keep the panel pointed at the selected repo. A no-op when the page is maximized, the panel is
/// focused (so its own keys drive it), or the selection isn't a repo.
fn maybe_follow_repo_page(app: &mut AppState) {
    if app.repo_page.is_some() && !app.repo_page_maximized && app.focus == Pane::List {
        if let Some(idx) = app.selected_repo_index() {
            app.retarget_repo_page(idx);
        }
    }
}

/// Rows the plain mouse wheel scrolls the repo list per notch (Alt+wheel moves the selection
/// instead, one step per notch).
const WHEEL_LIST_STEP: isize = 3;

/// Wheel step for a scroll event, scaled by modifier keys: Ctrl/Alt → a full `page`, Shift → 5×
/// the `base` step, otherwise `base`. (Some terminals don't report Shift on the wheel, hence Alt
/// also stands in for a fast jump.)
fn wheel_step(modifiers: KeyModifiers, base: usize, page: usize) -> usize {
    if modifiers.contains(KeyModifiers::CONTROL) || modifiers.contains(KeyModifiers::ALT) {
        page.max(1)
    } else if modifiers.contains(KeyModifiers::SHIFT) {
        base.saturating_mul(5)
    } else {
        base
    }
}

/// Pop the keyboard enhancement flags pushed by `push_key_enhancement`.
fn pop_key_enhancement(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
    if supports_keyboard_enhancement().unwrap_or(false) {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
}


/// Apply a command triggered by key OR by clicking its status-bar hint. Returns
/// `Some(exit_code)` when the command should quit the app. `pending_claude`/`pending_lazygit`
/// are the event loop's suspend-to-launch slots (picked up at the top of the next iteration).
/// A cursor-anchored dwell tooltip (a 1×1 anchor at the cursor, preferring to sit above it). Used
/// for settings rows, help links, and footer commands; column headers carry their own anchor/side.
fn cursor_tip(cursor: Option<(u16, u16)>, text: String) -> Option<app::HoverTip> {
    cursor.map(|(col, row)| app::HoverTip {
        text,
        anchor: Rect { x: col, y: row, width: 1, height: 1 },
        placement: tui_pick::Placement::top_center(),
        hide_column: None,
    })
}

fn dispatch_command(
    command: Cmd,
    app: &mut AppState,
    retry_queue: &mut Vec<usize>,
    pending_claude: &mut Option<std::path::PathBuf>,
    pending_lazygit: &mut Option<std::path::PathBuf>,
) -> Option<i32> {
    match command {
        Cmd::Retry => {
            if let Some(repos) = app.selected_header_repos() {
                // A folder/group header is selected → retry the retryable repos it covers.
                let scoped: Vec<usize> = repos
                    .into_iter()
                    .filter(|&idx| app.repos[idx].lock().unwrap().status.is_retryable())
                    .collect();
                retry_queue.extend(scoped);
            } else if let Some(idx) = app.selected_repo_index() {
                if app.repos[idx].lock().unwrap().status.is_retryable() {
                    retry_queue.push(idx);
                }
            }
        }
        Cmd::RetryAll => retry_queue.extend(app.retryable_repos()),
        Cmd::Refetch => {
            if let Some(repos) = app.selected_header_repos() {
                // A folder/group header is selected → re-pull every repo it covers (not running).
                let scoped: Vec<usize> = repos
                    .into_iter()
                    .filter(|&idx| !app.repos[idx].lock().unwrap().status.is_running())
                    .collect();
                retry_queue.extend(scoped);
            } else if let Some(idx) = app.selected_repo_index() {
                if app.repos[idx].lock().unwrap().status.is_terminal() {
                    retry_queue.push(idx);
                }
            }
        }
        Cmd::RefetchAll => retry_queue.extend(app.refetchable_repos()),
        Cmd::Info => {
            app.info_pinned = !app.info_pinned;
        }
        Cmd::ToggleResultPanel => app.toggle_result_panel(),
        Cmd::Help => app.open_help(),
        Cmd::OpenPage => app.open_repo_page(),
        Cmd::ToggleLeader => {
            app.pending_leader = if app.pending_leader == Some(Leader::Toggle) {
                None
            } else {
                Some(Leader::Toggle)
            };
        }
        Cmd::ToggleColumn(column) => {
            // Stay in toggle mode (matches the sticky `t` keyboard behavior) so several
            // columns can be clicked in a row; `t`/Esc or a non-toggle key exits.
            app.toggle_column(column);
        }
        Cmd::FilterLeader => {
            app.pending_leader = if app.pending_leader == Some(Leader::Filter) {
                None
            } else {
                Some(Leader::Filter)
            };
        }
        Cmd::SetFilter(filter) => {
            // Picking a filter applies it and closes the leader (unlike the sticky column menu).
            app.set_status_filter(filter);
            app.pending_leader = None;
        }
        Cmd::SortLeader => {
            app.pending_leader = if app.pending_leader == Some(Leader::Sort) {
                None
            } else {
                Some(Leader::Sort)
            };
        }
        Cmd::SetSort(column) => {
            app.set_sort(column);
            app.pending_leader = None;
        }
        Cmd::LeaderCancel => app.pending_leader = None,
        Cmd::FlipSort => {
            let column = app.sort_column;
            app.set_sort(column); // re-applying the active column flips direction
        }
        Cmd::NameFilter => {
            // Clicking the `/ filter` hint toggles: enter filter input, or exit it when already
            // filtering (dropping an empty filter so it leaves no dangling tag).
            if app.filter_input_mode {
                if app.filter.as_deref() == Some("") {
                    app.filter = None;
                }
                app.commit_filter_input();
            } else {
                app.begin_filter_input();
            }
        }
        Cmd::ClearNameFilter => {
            app.filter = None;
            app.filter_input_mode = false;
            app.filter_prev_selection = None;
        }
        Cmd::ResultOverlay => {
            app.result_overlay = !app.result_overlay;
        }
        Cmd::FocusToggle => app.cycle_focus(true),
        Cmd::SplitNarrow => app.adjust_split(-0.03),
        Cmd::SplitWiden => app.adjust_split(0.03),
        Cmd::GroupingToggle => app.toggle_grouping_view(),
        Cmd::FavoritesFirst => app.toggle_favorites_first(),
        Cmd::TreeToggle => app.toggle_tree_view(),
        Cmd::FoldCollapseAll => app.collapse_all(),
        Cmd::FoldExpandAll => app.expand_all(),
        Cmd::FoldExpandSubtree => app.expand_subtree(),
        Cmd::ToggleGroupCollapsed(group_idx) => app.toggle_group_collapsed(group_idx, None),
        Cmd::DiffView => app.toggle_diff_view(),
        Cmd::Claude => {
            if let Some(idx) = app.selected_repo_index() {
                *pending_claude = Some(app.repos[idx].lock().unwrap().path.clone());
            }
        }
        Cmd::Lazygit => {
            if let Some(idx) = app.selected_repo_index() {
                *pending_lazygit = Some(app.repos[idx].lock().unwrap().path.clone());
            }
        }
        Cmd::OpenRemote => {
            let url = app
                .selected_repo_index()
                .and_then(|idx| app.repos[idx].lock().unwrap().remote_url.clone());
            if let Some(url) = url {
                open_url(&url);
            }
        }
        Cmd::CopyPath => {
            if let Some(idx) = app.selected_repo_index() {
                let path = app.repos[idx].lock().unwrap().path.display().to_string();
                app.show_copy_toast(&path);
                copy_to_clipboard(&path);
            }
        }
        Cmd::CopyRemote => {
            let url = app
                .selected_repo_index()
                .and_then(|idx| app.repos[idx].lock().unwrap().remote_url.clone());
            if let Some(url) = url {
                app.show_copy_toast(&url);
                copy_to_clipboard(&url);
            }
        }
        Cmd::Settings => app.open_settings(),
        Cmd::ShowBuildInfo => app.open_build_info(),
        Cmd::ShowChangelog => app.open_changelog(false),
        Cmd::NavDown => {
            app.nav_down();
        }
        Cmd::NavUp => {
            app.nav_up();
        }
        Cmd::NavLeft => {
            app.nav_left();
        }
        Cmd::NavRight => {
            app.nav_right();
        }
        Cmd::Quit => {
            return Some(if app.all_done {
                let failed = app
                    .repos
                    .iter()
                    .any(|repo| repo.lock().unwrap().status.is_failed());
                i32::from(failed)
            } else {
                2
            });
        }
    }
    None
}

/// Build the confirm dialog for clearing/deleting a repo-page row. Returns None for the HEAD
/// branch (which can't be deleted); the danger flag scales the dialog's severity.
fn confirm_for_row(repo_idx: usize, row: &PageRow) -> Option<ConfirmDialog> {
    match row.kind {
        // Stash drops are routed through run_prepare_drop_stash (to list the stash's files).
        PageRowKind::Stash => None,
        PageRowKind::Worktree => {
            let mut message = format!("Remove worktree {}?", row.path.display());
            if row.dirty {
                message.push_str(" Uncommitted changes will be LOST.");
            }
            Some(ConfirmDialog::simple(
                message,
                ConfirmAction::RemoveWorktree {
                    repo_idx,
                    path: row.path.clone(),
                    force: row.dirty,
                },
                row.dirty,
            ))
        }
        PageRowKind::Branch if row.is_head => None,
        PageRowKind::Branch if row.deletable => Some(ConfirmDialog::simple(
            format!("Delete branch '{}'?", row.branch),
            ConfirmAction::DeleteBranch {
                repo_idx,
                branch: row.branch.clone(),
                force: false,
            },
            false,
        )),
        PageRowKind::Branch => Some(ConfirmDialog::simple(
            format!(
                "Force-delete unmerged branch '{}'? Unmerged commits will be lost.",
                row.branch
            ),
            ConfirmAction::DeleteBranch {
                repo_idx,
                branch: row.branch.clone(),
                force: true,
            },
            true,
        )),
    }
}

async fn run() -> Result<i32> {
    let cli = Cli::parse();

    let max_jobs = cli
        .jobs
        .filter(|&jobs| jobs > 0)
        .unwrap_or_else(num_cpus::get);
    // Recursive scanning is the default; `--no-recursive` (or `--depth 1`) restores the legacy
    // single-level scan. `--depth 0` is meaningless, so floor it at 1.
    let max_depth = if cli.no_recursive { 1 } else { cli.depth.max(1) };
    let use_tui = !cli.no_tui && io::stderr().is_terminal();
    let profiling = profile::profile_enabled(cli.profile);

    // Subcommands.
    if let Some(Commands::Ws { action }) = &cli.command {
        match action {
            Some(WsAction::Ls) => return list_workspaces(),
            None => {
                // The picker is interactive — fall back to a printed list without a TTY.
                if !use_tui {
                    return list_workspaces();
                }
                return match pick_workspace()? {
                    Some((name, roots)) => {
                        run_tui(
                            roots,
                            Some(name),
                            max_jobs,
                            max_depth,
                            cli.timeout,
                            cli.no_worktrees,
                            profiling,
                            cli.profile_out,
                        )
                        .await
                    }
                    None => Ok(0),
                };
            }
        }
    }

    // Default run: the CLI dirs (or the cwd), or a named workspace via `-w`.
    let (roots, active_workspace) = resolve_roots(&cli.dirs, cli.workspace.as_deref())?;

    if !use_tui {
        return plain::run_plain(
            &roots,
            max_jobs,
            max_depth,
            cli.timeout,
            cli.no_worktrees,
            profiling,
            cli.profile_out.as_deref(),
        )
        .await;
    }

    run_tui(
        roots,
        active_workspace,
        max_jobs,
        max_depth,
        cli.timeout,
        cli.no_worktrees,
        profiling,
        cli.profile_out,
    )
    .await
}

/// Resolve the launch roots and the active workspace name (canonicalized + deduped, order
/// preserved). With `-w <name>`: open that workspace's saved folders, or — when DIRS are given —
/// (re)define it as those. Without `-w`: just the CLI dirs, else the cwd; never a saved workspace.
/// A brand-new / emptied workspace seeds from the cwd so there's something to scan.
fn resolve_roots(
    cli_dirs: &[PathBuf],
    workspace: Option<&str>,
) -> Result<(Vec<PathBuf>, Option<String>)> {
    let mut out: Vec<PathBuf> = Vec::new();
    let add = |path: PathBuf, out: &mut Vec<PathBuf>| {
        let abs = std::fs::canonicalize(&path).unwrap_or(path);
        if !out.contains(&abs) {
            out.push(abs);
        }
    };
    for dir in cli_dirs {
        add(dir.clone(), &mut out);
    }
    match workspace {
        Some(name) => {
            if out.is_empty() {
                if let Some(roots) = crate::persist::load().workspaces_migrated().get(name) {
                    for root in roots {
                        add(PathBuf::from(root), &mut out);
                    }
                }
            }
            if out.is_empty() {
                out.push(std::env::current_dir()?);
            }
            Ok((out, Some(name.to_string())))
        }
        None => {
            if out.is_empty() {
                out.push(std::env::current_dir()?);
            }
            Ok((out, None))
        }
    }
}

/// Print the saved workspaces (name → folders) to stdout. Used by `ws ls` and as the no-TTY
/// fallback for the `ws` picker.
fn list_workspaces() -> Result<i32> {
    let workspaces = crate::persist::load().workspaces_migrated();
    if workspaces.is_empty() {
        println!("No saved workspaces yet.");
        println!("Create one:  polygit -w <name> <dir>...");
        return Ok(0);
    }
    let mut names: Vec<&String> = workspaces.keys().collect();
    names.sort();
    println!("Saved workspaces — open with `polygit -w <name>` or pick with `polygit ws`:\n");
    for name in names {
        let roots = &workspaces[name];
        let plural = if roots.len() == 1 { "" } else { "s" };
        println!("  {name}  ({} folder{plural})", roots.len());
        for root in roots {
            println!("      {root}");
        }
    }
    Ok(0)
}

/// Interactive workspace picker (`polygit ws`): a full-screen list of saved workspaces. Returns the
/// chosen `(name, roots)`, or `None` if cancelled / none saved.
fn pick_workspace() -> Result<Option<(String, Vec<PathBuf>)>> {
    let workspaces = crate::persist::load().workspaces_migrated();
    if workspaces.is_empty() {
        list_workspaces()?;
        return Ok(None);
    }
    let mut names: Vec<String> = workspaces.keys().cloned().collect();
    names.sort();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut selected = 0usize;
    let outcome = loop {
        terminal.draw(|frame| render_workspace_picker(frame, &names, &workspaces, selected))?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => break None,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break None,
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    if selected + 1 < names.len() {
                        selected += 1;
                    }
                }
                KeyCode::Home | KeyCode::Char('g') => selected = 0,
                KeyCode::End | KeyCode::Char('G') => selected = names.len().saturating_sub(1),
                KeyCode::Enter => {
                    let name = names[selected].clone();
                    let roots = workspaces[&name].iter().map(PathBuf::from).collect();
                    break Some((name, roots));
                }
                _ => {}
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(outcome)
}

/// Draw the `ws` picker: a bordered, centered list of workspace names with their folder counts.
fn render_workspace_picker(
    frame: &mut Frame,
    names: &[String],
    workspaces: &HashMap<String, Vec<String>>,
    selected: usize,
) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState};

    let full = frame.area();
    let width = full.width.min(80);
    let height = full.height.min(names.len() as u16 + 4).max(5);
    let area = Rect {
        x: full.x + (full.width.saturating_sub(width)) / 2,
        y: full.y + (full.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" polygit · pick a workspace ")
        .title_bottom(Line::from(Span::styled(
            " ↑↓ move · enter open · esc cancel ",
            Style::default().fg(Color::DarkGray),
        )));

    let items: Vec<ListItem> = names
        .iter()
        .map(|name| {
            let count = workspaces.get(name).map_or(0, Vec::len);
            let plural = if count == 1 { "" } else { "s" };
            ListItem::new(Line::from(vec![
                Span::styled(name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("  ({count} folder{plural})"), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(selected));
    frame.render_stateful_widget(list, area, &mut state);
}

/// TUI entry point: sets up terminal, runs the event loop, and restores on exit.
#[allow(clippy::too_many_arguments)]
async fn run_tui(
    roots: Vec<PathBuf>,
    active_workspace: Option<String>,
    max_jobs: usize,
    max_depth: usize,
    timeout_secs: u64,
    no_worktrees: bool,
    profiling: bool,
    profile_out: Option<PathBuf>,
) -> Result<i32> {
    // Repos stream in from the recursive walker (see `run_discovery` below); the list starts
    // empty and grows as the scan progresses, so there's no up-front discovery wait.

    // Detect the terminal background for Theme::Auto — must happen before raw mode /
    // the alternate screen (the OSC query reads its reply from the tty itself).
    let auto_dark = theme::detect_dark_background();

    let app_state = Arc::new(Mutex::new(AppState::new(Vec::new(), max_jobs, auto_dark)));
    // Persist the current version now so the "What's New" modal (raised when this build is newer
    // than the last-seen one) doesn't re-pop on the next launch even if nothing else is saved.
    app_state.lock().unwrap().save_state();
    // Load group definitions (optional, user-edited) + the dynamic-membership cache.
    let (groups_config, groups_config_error) = groups::load_config();
    let groups_cache = groups::load_cache();
    let icon_style = {
        let mut app = app_state.lock().unwrap();
        // The scanned roots drive the tree forest; an active workspace persists them by name.
        app.root_dirs = roots.clone();
        app.active_workspace = active_workspace;
        // Capture discovery settings so the picker can scan a runtime-added root the same way.
        app.discovery_max_depth = max_depth;
        app.discovery_timeout_secs = timeout_secs;
        app.discovery_no_worktrees = no_worktrees;
        // Persist the active workspace's folder set (a no-op for an ad-hoc cwd/CLI-dirs session).
        app.save_state();
        let group_errors = app.init_groups(groups_config, &groups_cache);
        if let Some(error) = groups_config_error.or_else(|| group_errors.into_iter().next()) {
            app.show_toast(error);
        }
        app.icon_style
    };

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    push_key_enhancement(&mut terminal);

    // Ensure terminal is restored on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            PopKeyboardEnhancementFlags,
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        original_hook(panic_info);
    }));

    // The shared concurrency gate + throttle governor (drives back-off + recovery).
    let throttle = app_state.lock().unwrap().throttle.clone();
    tokio::spawn(worker::run_governor(throttle.clone()));

    // Stream repos in from the recursive walker: each batch is appended, its pulls + remote-url
    // discovery kick off immediately, and worktree discovery runs once the walk completes.
    tokio::spawn(run_discovery(
        Arc::clone(&app_state),
        roots.clone(),
        max_depth,
        throttle,
        max_jobs,
        timeout_secs,
        icon_style,
        no_worktrees,
        true,
    ));

    // Resolve dynamic (command/url) group memberships in the background; the task no-ops when
    // every dynamic group has a fresh cached membership.
    if app_state.lock().unwrap().any_dynamic_groups() {
        tokio::spawn(groups::run_group_resolution(Arc::clone(&app_state), false));
    }

    // Watch the binary on disk for a newer build (drives the reload notice).
    tokio::spawn(watch_for_new_build(Arc::clone(&app_state)));
    tokio::spawn(watch_theme(Arc::clone(&app_state)));

    let exit_code = run_event_loop(&mut terminal, Arc::clone(&app_state)).await?;

    // Persist UI preferences (columns, info state, splitter) and the status cache for next run.
    {
        let mut app = app_state.lock().unwrap();
        app.save_state();
        app.flush_cache(now_unix());
        app.flush_pr_cache();
    }

    // Restore terminal
    pop_key_enhancement(&mut terminal);
    // Disable all-motion mouse tracking (hover effects) — DisableMouseCapture doesn't cover 1003.
    let _ = terminal.backend_mut().write_all(b"\x1b[?1003l");
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    // Reload requested: replace this process with the new build, same argv. Never returns
    // on success (the fresh process sets up its own terminal and re-runs the pulls).
    if exit_code == RELOAD_EXIT {
        // After a rename-over install, /proc/self/exe reads "<path> (deleted)" — strip the
        // suffix so we exec the NEW file now living at the original path.
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("polygit"));
        let exe_str = exe.to_string_lossy();
        let exe = PathBuf::from(exe_str.strip_suffix(" (deleted)").unwrap_or(&exe_str));
        let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        let error = Command::new(&exe).args(&args).exec();
        eprintln!("error: reload failed: {error}");
        return Ok(1);
    }

    // Emit the profile report only after the alternate screen is left so it
    // doesn't corrupt the display.
    if profiling {
        let rows = build_profile_rows(&app_state);
        let report = profile::format_report(rows);
        emit_report(&report, profile_out.as_deref())?;
    }

    Ok(exit_code)
}

/// Build profile rows from the shared repo state for the TUI run.
fn build_profile_rows(app_state: &Arc<Mutex<AppState>>) -> Vec<profile::ProfileRow> {
    let app = app_state.lock().unwrap();
    app.repos
        .iter()
        .map(|repo| {
            let state = repo.lock().unwrap();
            let status = match &state.status {
                RepoStatus::Updated => "updated",
                RepoStatus::UpToDate => "uptodate",
                RepoStatus::NoUpstream => "noupstream",
                RepoStatus::Skipped => "skipped",
                RepoStatus::Throttled => "throttled",
                RepoStatus::Failed => "failed",
                RepoStatus::Running { .. } => "running",
                RepoStatus::Queued => "queued",
            };
            let last_log_line = state
                .log
                .lines()
                .iter()
                .rev()
                .find(|line| !line.trim().is_empty())
                .cloned()
                .unwrap_or_default();
            profile::ProfileRow {
                name: state.name.clone(),
                branch: state.branch.clone().unwrap_or_else(|| "?".to_string()),
                status,
                elapsed: state.elapsed.unwrap_or_default(),
                last_log_line,
            }
        })
        .collect()
}

/// Write the profile report to the given file, or to stderr if none.
fn emit_report(report: &str, profile_out: Option<&std::path::Path>) -> Result<()> {
    match profile_out {
        Some(path) => std::fs::write(path, report)?,
        None => eprint!("{report}"),
    }
    Ok(())
}

/// Main event loop: renders UI and handles keyboard input.
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app_state: Arc<Mutex<AppState>>,
) -> Result<i32> {
    let mut tick: u64 = 0;

    // Track which repos to retry
    let mut retry_queue: Vec<usize> = Vec::new();

    // Whether the divider is currently being dragged with the mouse.
    let mut dragging_divider = false;
    let mut dragging_dock = false;
    let mut dragging_preview_split = false;
    // Tracks the list selection between frames: when it changes (keyboard / Alt+wheel nav, filter
    // preview, …) the view scrolls just enough to keep it visible. The plain wheel changes only
    // `list_scroll`, not the selection, so it never triggers this — the view scrolls freely.
    let mut last_list_selection: usize = usize::MAX;
    let mut last_branch_check = Instant::now();
    // Which scrollbar (if any) is currently being dragged.
    let mut scroll_drag: Option<app::ScrollKind> = None;

    // Set when `c` is pressed; the TUI is suspended to run claude code after event handling.
    let mut pending_claude: Option<std::path::PathBuf> = None;
    // Set when `l` is pressed; the TUI is suspended to run lazygit after event handling.
    let mut pending_lazygit: Option<std::path::PathBuf> = None;

    // Last left-click (time, selection) for synthesizing double-click → open repo page.
    let mut last_click: Option<(Instant, usize)> = None;

    // Keys injected by clicking a footer hint — drained before polling real input, so a clicked
    // hint runs through the exact same key handler as a real keypress.
    let mut synthetic_keys: std::collections::VecDeque<KeyEvent> = std::collections::VecDeque::new();

    // Whether all-motion mouse tracking (DEC 1003) is currently enabled in the terminal; kept in
    // sync with the `hover_effects` setting each render.
    let mut hover_tracking_on = false;
    // Dwell tracking for the footer-command tooltip: which command the cursor is resting on and
    // since when. The tooltip appears once it's been the same command for ~1s.
    let mut hover_dwell_text: Option<String> = None;
    let mut hover_dwell_since = Instant::now();

    loop {
        // Suspend the TUI and run claude code when requested (set by a key/click last iteration).
        if let Some(path) = pending_claude.take() {
            launch_claude(terminal, &path)?;
        }

        // Suspend the TUI and run lazygit when requested, or note that it isn't installed.
        if let Some(path) = pending_lazygit.take() {
            if lazygit_available() {
                launch_lazygit(terminal, &path)?;
            } else {
                let mut app = app_state.lock().unwrap();
                if app.repo_page.is_some() {
                    app.repo_page_message = Some("lazygit not found on PATH".to_string());
                } else if let Some(idx) = app.selected_repo_index() {
                    app.repos[idx]
                        .lock()
                        .unwrap()
                        .log
                        .push("lazygit not found on PATH".to_string());
                }
            }
        }

        // Update the "all done" edge. Selection is never moved automatically — it stays wherever
        // the user put it (no follow-the-running-repo, no jump-to-Result-when-complete).
        {
            let mut app = app_state.lock().unwrap();
            // Don't settle until the walker has finished AND found at least one repo — an empty
            // `all(...)` is vacuously true, which would otherwise freeze the timer at 0 repos.
            // When auto-pull was suppressed, idle/cached repos that were never pulled count as
            // settled (we wait only for any *manual* pull in flight), so the timer freezes and
            // the Result row renders instead of hanging on Queued repos.
            let suppressed = app.auto_pull_suppressed;
            let all_done = app.discovery_done
                && !app.repos.is_empty()
                && app.repos.iter().all(|repo| {
                    let status = &repo.lock().unwrap().status;
                    if suppressed {
                        !status.is_running()
                    } else {
                        status.is_terminal()
                    }
                });

            if all_done && !app.all_done {
                app.all_done = true;
                app.finished_elapsed = Some(app.start.elapsed());
                // Persist each repo's fresh terminal status so the next launch shows it instantly.
                app.flush_cache(now_unix());
                app.flush_pr_cache();
            }
        }

        // Pull throttled repos whose backoff has elapsed back into the retry queue.
        {
            let app = app_state.lock().unwrap();
            let due = app.throttle.take_due_retries();
            drop(app);
            retry_queue.extend(due);
        }

        // Process retry queue
        if !retry_queue.is_empty() {
            let (control, max_jobs, icon_style) = {
                let app = app_state.lock().unwrap();
                (app.throttle.clone(), app.max_jobs, app.icon_style)
            };
            let timeout_secs = 30u64;

            // A fresh batch of work is starting: restart the header timer and re-arm the
            // all-done edge so it freezes again once this batch completes.
            {
                let mut app = app_state.lock().unwrap();
                app.start = Instant::now();
                app.finished_elapsed = None;
                app.all_done = false;
            }

            let repos_to_retry: Vec<SharedRepoState> = retry_queue
                .drain(..)
                .map(|idx| {
                    let app = app_state.lock().unwrap();
                    let repo = Arc::clone(&app.repos[idx]);
                    {
                        let mut state = repo.lock().unwrap();
                        state.status = RepoStatus::Queued;
                        state.log.clear();
                        state.auto_scroll = true;
                        // Keep the cached details visible during the refresh; run_refetch_batch
                        // diffs old vs new and flashes only the cells that actually changed.
                    }
                    repo
                })
                .collect();

            let app_state_clone = Arc::clone(&app_state);
            tokio::spawn(async move {
                run_refetch_batch(
                    app_state_clone,
                    repos_to_retry,
                    control,
                    max_jobs,
                    timeout_secs,
                    icon_style,
                )
                .await;
            });
        }

        // Render
        {
            let mut app = app_state.lock().unwrap();
            // Keep the selection in view whenever it moved this frame (keyboard / Alt+wheel nav,
            // filter preview, reselect after a layout change). A plain wheel scroll leaves the
            // selection unchanged, so this is skipped and the view stays where the wheel left it.
            if app.selected != last_list_selection {
                let viewport = app.list_rows_area.height as usize;
                app.ensure_list_selection_visible(viewport);
                last_list_selection = app.selected;
            }
            // Sync all-motion mouse tracking (DEC 1003) to the hover-effects setting: on enables
            // `Moved` events for hover highlighting; off restores the terminal's own selection.
            if app.hover_effects != hover_tracking_on {
                let mut out = io::stdout();
                // Off must re-assert button/SGR tracking: some terminals drop click reporting
                // entirely when all-motion (1003) is turned off, leaving the UI unclickable.
                let seq: &[u8] = if app.hover_effects {
                    b"\x1b[?1003h"
                } else {
                    b"\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h"
                };
                let _ = out.write_all(seq);
                let _ = out.flush();
                hover_tracking_on = app.hover_effects;
                if !app.hover_effects {
                    app.hover = None;
                }
            }
            // Hover affordances (reads last frame's click regions — the layout is stable
            // frame-to-frame): a help-link URL shown bottom-left immediately (browser-style), and a
            // dwell tooltip after 1s over either a help link (its URL) or a footer command.
            let help_url = if app.show_help && app.help_tab == app::HelpTab::About {
                app.hover.and_then(|(_, row)| {
                    app.help_links.iter().find(|(link_row, _)| *link_row == row).map(|(_, url)| url.clone())
                })
            } else {
                None
            };
            app.status_hint = help_url.clone();
            // Hovering a built-command token (CLI builder tab) dwells a "click to remove" tip.
            let cli_cmd_tip = if app.show_help && app.help_tab == app::HelpTab::CliFlags {
                app.hover.and_then(|(_, row)| {
                    app.cli_command_click
                        .iter()
                        .any(|(token_row, _)| *token_row == row)
                        .then(|| "click to remove this flag from the command".to_string())
                })
            } else {
                None
            };
            let settings_tip = if app.show_settings {
                app.hover.and_then(|(col, row)| {
                    app.settings_hit_at(col, row)
                        .and_then(|(setting_row, option)| AppState::settings_tip(setting_row, option))
                        .map(str::to_string)
                })
            } else {
                None
            };
            // Cursor-anchored tips (settings rows, help links, footer commands) sit above the
            // cursor; column-header / count-tail tips carry their own anchor + side (below the
            // header). The floating engine flips/shifts each to stay on-screen.
            let cursor = app.hover;
            // Keep the active tooltip alive while the cursor is over its own popup, so an
            // interactive tooltip (e.g. a column header's `[x]` hide button) stays clickable as the
            // cursor moves off the header onto it.
            let over_popup = app.hover_tooltip.is_some()
                && cursor.is_some_and(|(col, row)| point_in(app.tooltip_rect, col, row));
            if !over_popup {
                // Per-area tooltip gating: the master switch plus each area's own toggle (the
                // Tooltips settings group). `cursor_tip` still requires `hover_effects` upstream.
                let tips = app.tooltips;
                let dwell: Option<app::HoverTip> = settings_tip
                    .filter(|_| tips.enabled && tips.settings)
                    .and_then(|tip| cursor_tip(cursor, tip))
                    .or_else(|| {
                        help_url
                            .filter(|_| tips.enabled && tips.links)
                            .and_then(|url| cursor_tip(cursor, url))
                    })
                    .or_else(|| {
                        cli_cmd_tip.filter(|_| tips.enabled).and_then(|tip| cursor_tip(cursor, tip))
                    })
                    .or_else(|| {
                        if !tips.enabled {
                            return None;
                        }
                        cursor.and_then(|(col, row)| app.tooltip_at(col, row)).and_then(
                            |(text, anchor, placement, hide_column, area)| {
                                let allowed = match area {
                                    app::TooltipArea::Header => tips.headers,
                                    app::TooltipArea::Count => tips.counts,
                                };
                                allowed.then_some(app::HoverTip {
                                    text,
                                    anchor,
                                    placement,
                                    hide_column,
                                })
                            },
                        )
                    })
                    .or_else(|| {
                        if !(tips.enabled && tips.footer) {
                            return None;
                        }
                        cursor
                            .and_then(|(col, row)| app.command_at(col, row))
                            .and_then(|cmd| cursor_tip(cursor, cmd.tooltip().to_string()))
                    });
                let dwell_text = dwell.as_ref().map(|tip| tip.text.clone());
                if dwell_text != hover_dwell_text {
                    hover_dwell_text = dwell_text;
                    hover_dwell_since = Instant::now();
                    app.hover_tooltip = None;
                } else if let Some(tip) = dwell {
                    if app.hover_tooltip.is_none()
                        && hover_dwell_since.elapsed() >= Duration::from_millis(1000)
                    {
                        app.hover_tooltip = Some(tip);
                    }
                }
            }
            // Periodic local branch/status refresh (no pull) when enabled — interval scales with
            // the repo count; held off while any pull is in flight.
            if app.branch_check == app::BranchCheck::Auto
                && app.discovery_done
                && !app.any_pull_running()
            {
                let interval =
                    Duration::from_secs(AppState::branch_check_interval_secs(app.repos.len()));
                if last_branch_check.elapsed() >= interval {
                    last_branch_check = Instant::now();
                    tokio::spawn(run_all_details(app.repos.clone(), app.max_jobs));
                }
            }
            app.divider_dragging = dragging_divider;
            app.scrollbar_dragging = scroll_drag;
            // Latch the pulled/chg columns on once a delta lands, so a retry doesn't flicker them.
            app.refresh_pulled_seen();
            // Master-detail: while the restored panel [4] is open and the list ([1]) has focus, keep
            // the panel pointed at the selected repo (cheap no-op when it's already on that repo).
            maybe_follow_repo_page(&mut app);
            terminal.draw(|frame| render::render(frame, &mut app, tick))?;
        }

        // Handle events with a short timeout for animation. A clicked footer hint queues a
        // synthetic key, drained here before polling real input so it dispatches identically.
        let poll_timeout = Duration::from_millis(50);
        let next_event = if let Some(key) = synthetic_keys.pop_front() {
            Some(Event::Key(key))
        } else if event::poll(poll_timeout)? {
            Some(event::read()?)
        } else {
            None
        };
        if let Some(next_event) = next_event {
            match next_event {
            Event::Mouse(mouse) => {
                let mut app = app_state.lock().unwrap();

                // Record the cursor for the hover highlight — only when the feature is on, so a
                // stray motion event (e.g. just after toggling off) can't resurrect a highlight.
                if app.hover_effects {
                    app.hover = Some((mouse.column, mouse.row));
                }
                // Bare cursor motion carries no action.
                if matches!(mouse.kind, MouseEventKind::Moved) {
                    continue;
                }

                // The dwell tooltip's `[x]` hides that column. The popup floats above every pane, so
                // it's hit-tested first — before the splitter/scrollbar grabs underneath it.
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    if let Some((_, _, _, column)) = app.tooltip_hide_click.filter(|&(r, s, e, _)| {
                        mouse.row == r && mouse.column >= s && mouse.column < e
                    }) {
                        app.toggle_column(column);
                        app.save_state();
                        app.hover_tooltip = None;
                        app.tooltip_hide_click = None;
                        continue;
                    }
                }

                // Dragging the restored panel's top boundary resizes it (a horizontal splitter).
                // Handled before the repo-page/modal dispatch so a grab on the boundary wins — but
                // the title-bar buttons live on that same border row, so exclude their columns or
                // the maximize/restore and `[esc back]` buttons could never be clicked.
                let on_dock_boundary = app
                    .dock_divider_row
                    .is_some_and(|row| mouse.row == row || mouse.row + 1 == row)
                    && !app.title_button_hit(mouse.column, mouse.row);
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if on_dock_boundary => {
                        dragging_dock = true;
                        app.set_dock_from_row(mouse.row);
                        continue;
                    }
                    MouseEventKind::Drag(MouseButton::Left) if dragging_dock => {
                        app.set_dock_from_row(mouse.row);
                        continue;
                    }
                    MouseEventKind::Up(MouseButton::Left) if dragging_dock => {
                        dragging_dock = false;
                        app.save_state();
                        continue;
                    }
                    _ => {}
                }

                // Dragging the info/result boundary inside the preview resizes the two panels (a
                // horizontal splitter within the right pane). Only over the preview pane's columns.
                let on_preview_split = app.preview_divider_row.is_some_and(|row| {
                    (mouse.row == row || mouse.row + 1 == row) && mouse.column >= app.divider_col
                });
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if on_preview_split => {
                        dragging_preview_split = true;
                        app.set_preview_split_from_row(mouse.row);
                        continue;
                    }
                    MouseEventKind::Drag(MouseButton::Left) if dragging_preview_split => {
                        app.set_preview_split_from_row(mouse.row);
                        continue;
                    }
                    MouseEventKind::Up(MouseButton::Left) if dragging_preview_split => {
                        dragging_preview_split = false;
                        app.save_state();
                        continue;
                    }
                    _ => {}
                }

                // Draggable scrollbars (preview, diff panels, help, repo page) are handled here,
                // before the per-view gates, so a grab works in any modal/view.
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(kind) = app.scrollbar_at(mouse.column, mouse.row) {
                            scroll_drag = Some(kind);
                            if let Some(value) = app.scroll_value_for(kind, mouse.row) {
                                if app.apply_scroll(kind, value) {
                                    drop(app);
                                    tokio::spawn(run_diff_modal_file(Arc::clone(&app_state)));
                                }
                            }
                            continue;
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        if let Some(kind) = scroll_drag {
                            if let Some(value) = app.scroll_value_for(kind, mouse.row) {
                                if app.apply_scroll(kind, value) {
                                    drop(app);
                                    tokio::spawn(run_diff_modal_file(Arc::clone(&app_state)));
                                }
                            }
                            continue;
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        if scroll_drag.take().is_some() {
                            continue;
                        }
                    }
                    _ => {}
                }

                // Folder picker: footer hints inject keys; [x]/outside cancel; a breadcrumb navigates;
                // a row click activates it (folder → open, repo → select). Other mouse events swallowed.
                if app.picker.is_some() {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        if let Some(hint) = app.hint_at(mouse.column, mouse.row) {
                            synthetic_keys.push_back(hint_key_event(hint));
                            continue;
                        }
                        if region_hit(app.picker_close_click, mouse.column, mouse.row) {
                            app.sync_picker_bookmarks();
                            app.picker = None;
                            continue;
                        }
                        if let Some(target) = app
                            .picker_crumbs_click
                            .iter()
                            .find(|(row, start, end, _)| {
                                mouse.row == *row && mouse.column >= *start && mouse.column < *end
                            })
                            .map(|(.., path)| path.clone())
                        {
                            app.picker.as_mut().unwrap().navigate_to(target);
                            continue;
                        }
                        if let Some(&(_, view_index)) =
                            app.picker_rows_click.iter().find(|(row, _)| *row == mouse.row)
                        {
                            let picker = app.picker.as_mut().unwrap();
                            picker.select_at(view_index);
                            if let tui_pick::picker::PickerOutcome::Selected(path) =
                                picker.activate_selected()
                            {
                                app.sync_picker_bookmarks();
                                app.picker = None;
                                if let Some(abs) = app.add_root(path) {
                                    let throttle = app.throttle.clone();
                                    let max_jobs = app.max_jobs;
                                    let depth = app.discovery_max_depth;
                                    let timeout = app.discovery_timeout_secs;
                                    let icons = app.icon_style;
                                    let no_wt = app.discovery_no_worktrees;
                                    drop(app);
                                    tokio::spawn(run_discovery(
                                        Arc::clone(&app_state),
                                        vec![abs],
                                        depth,
                                        throttle,
                                        max_jobs,
                                        timeout,
                                        icons,
                                        no_wt,
                                        false,
                                    ));
                                }
                            }
                            continue;
                        }
                        if !point_in(app.picker_area, mouse.column, mouse.row) {
                            app.sync_picker_bookmarks();
                            app.picker = None;
                            continue;
                        }
                    }
                    continue;
                }

                // Finder overlay: footer hints inject their key; the [x]/outside close it; a row
                // click selects + jumps. All other mouse events are swallowed while it's open.
                if app.finder.is_some() {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        if let Some(hint) = app.hint_at(mouse.column, mouse.row) {
                            synthetic_keys.push_back(hint_key_event(hint));
                            continue;
                        }
                        if region_hit(app.finder_close_click, mouse.column, mouse.row) {
                            app.finder = None;
                            continue;
                        }
                        if let Some(&(_, view_index)) =
                            app.finder_rows_click.iter().find(|(row, _)| *row == mouse.row)
                        {
                            let finder = app.finder.as_mut().unwrap();
                            finder.select_at(view_index);
                            let selected = finder.selected_row().map(|row| row.key.clone());
                            if let Some(key) = selected {
                                app.finder = None;
                                app.finder_jump(&key);
                            }
                            continue;
                        }
                        if !point_in(app.finder_area, mouse.column, mouse.row) {
                            app.finder = None;
                            continue;
                        }
                    }
                    continue;
                }

                // A clicked footer hint injects its key — works over the repo page and every modal
                // footer, since only the visible footer's regions are registered this frame.
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    if let Some(hint) = app.hint_at(mouse.column, mouse.row) {
                        // A click on the restored panel's footer acts on panel [4]: focus it first so
                        // the injected key reaches the repo-page handler (not the list).
                        if app.repo_page.is_some()
                            && !app.repo_page_maximized
                            && point_in(app.dock_rect, mouse.column, mouse.row)
                        {
                            app.focus = Pane::RepoPage;
                        }
                        synthetic_keys.push_back(hint_key_event(hint));
                        continue;
                    }
                }

                // New-build notice buttons work over any view (the notice renders above panes).
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    if region_hit(app.update_close_click, mouse.column, mouse.row) {
                        app.update_dismissed = true;
                        continue;
                    }
                    if region_hit(app.update_reload_click, mouse.column, mouse.row) {
                        drop(app);
                        return Ok(RELOAD_EXIT);
                    }
                }

                // Footer status-bar commands are clickable in every context — including over an
                // open modal, where only settings/help/quit stay live (the rest are inert via
                // `style_footer`, so they have no click region). `q` inside a modal closes it
                // (injecting Esc reuses each modal's own close handler) rather than quitting.
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    let clicked = app
                        .clickable
                        .iter()
                        .find(|region| {
                            region.row == mouse.row
                                && mouse.column >= region.col_start
                                && mouse.column < region.col_end
                        })
                        .map(|region| region.command);
                    if let Some(command) = clicked {
                        if command == Cmd::Quit && app.any_modal_open() {
                            synthetic_keys.push_back(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                            continue;
                        }
                        if let Some(code) = dispatch_command(
                            command,
                            &mut app,
                            &mut retry_queue,
                            &mut pending_claude,
                            &mut pending_lazygit,
                        ) {
                            drop(app);
                            return Ok(code);
                        }
                        continue;
                    }
                }

                // Build-info modal: footer hints (`r` restart / `esc` close) are handled by the
                // hint-click injection above; the `[x]` button or a click outside closes it (a click
                // inside is inert, so you can select/scroll the JSON preview without it vanishing).
                if app.show_build_info {
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            app.build_info_scroll = app.build_info_scroll.saturating_add(3);
                        }
                        MouseEventKind::ScrollUp => {
                            app.build_info_scroll = app.build_info_scroll.saturating_sub(3);
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            if region_hit(app.build_info_fold_all_click, mouse.column, mouse.row) {
                                app.build_info_fold_all(false);
                            } else if region_hit(app.build_info_unfold_all_click, mouse.column, mouse.row) {
                                app.build_info_fold_all(true);
                            } else if let Some(index) = app
                                .build_info_tree_click
                                .iter()
                                .find(|&&(row, start, end, _)| {
                                    mouse.row == row && mouse.column >= start && mouse.column < end
                                })
                                .map(|&(.., index)| index)
                            {
                                // Click a container row to select + fold it.
                                app.build_info_tree_selected = index;
                                app.build_info_toggle_selected();
                            } else if region_hit(app.build_info_close_click, mouse.column, mouse.row)
                                || !point_in(app.build_info_area, mouse.column, mouse.row)
                            {
                                app.show_build_info = false;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Changelog modal: click an accordion header to select + fold it, `[x]`/outside to
                // close, wheel scrolls (the scrollbar grab is handled by the generic handler above).
                if app.show_changelog {
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            app.changelog_scroll = app.changelog_scroll.saturating_add(3);
                        }
                        MouseEventKind::ScrollUp => {
                            app.changelog_scroll = app.changelog_scroll.saturating_sub(3);
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(idx) = app
                                .changelog_header_click
                                .iter()
                                .find(|&&(row, start, end, _)| {
                                    mouse.row == row && mouse.column >= start && mouse.column < end
                                })
                                .map(|&(.., idx)| idx)
                            {
                                app.changelog_selected = idx;
                                if let Some(release) = changelog::releases().get(idx) {
                                    let version = release.version.to_string();
                                    app.toggle_changelog_release(&version);
                                }
                            } else if region_hit(app.changelog_close_click, mouse.column, mouse.row)
                                || !point_in(app.changelog_area, mouse.column, mouse.row)
                            {
                                app.show_changelog = false;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Header dropdown: click an item to activate it, `[x]`/outside to close, wheel moves.
                if app.dropdown.is_some() {
                    match mouse.kind {
                        MouseEventKind::ScrollDown => app.dropdown_move(1),
                        MouseEventKind::ScrollUp => app.dropdown_move(-1),
                        MouseEventKind::Down(MouseButton::Left) => {
                            if region_hit(app.dropdown_close_click, mouse.column, mouse.row)
                                || !point_in(app.dropdown_area, mouse.column, mouse.row)
                            {
                                app.close_dropdown();
                            } else if let Some(&(_, _, _, index)) = app
                                .dropdown_item_click
                                .iter()
                                .find(|&&(row, start, end, _)| {
                                    mouse.row == row && mouse.column >= start && mouse.column < end
                                })
                            {
                                if app.dropdown_activate(index) {
                                    app.close_dropdown();
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Settings modal: click a row label to select it, a radio chip to set that
                // value, [x] or anywhere outside to close. Everything else is swallowed so
                // clicks never fall through to the view behind.
                if app.show_settings {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        let tab_click = app
                            .settings_tab_click
                            .iter()
                            .find(|&&(row, start, end, _)| {
                                mouse.row == row && mouse.column >= start && mouse.column < end
                            })
                            .map(|&(.., tab)| tab);
                        let section_click = app
                            .settings_section_click
                            .iter()
                            .find(|&&(row, start, end, _)| {
                                mouse.row == row && mouse.column >= start && mouse.column < end
                            })
                            .map(|&(.., tab)| tab);
                        if region_hit(app.settings_close_click, mouse.column, mouse.row) {
                            app.show_settings = false;
                        } else if region_hit(app.settings_search_click, mouse.column, mouse.row) {
                            app.settings_begin_search();
                        } else if let Some(tab) = tab_click {
                            app.settings_select_tab(tab);
                        } else if region_hit(
                            app.settings_collapse_all_click,
                            mouse.column,
                            mouse.row,
                        ) {
                            app.toggle_all_settings_sections();
                        } else if let Some(tab) = section_click {
                            // Click a header: focus it (it becomes the active item, no child) and
                            // expand/collapse it.
                            app.settings_on_header = Some(tab);
                            app.toggle_settings_section(tab);
                        } else if let Some((row_idx, option)) =
                            app.settings_hit_at(mouse.column, mouse.row)
                        {
                            app.settings_on_header = None;
                            app.settings_selected = row_idx;
                            app.settings_tab = AppState::settings_tab_of_row(row_idx);
                            match option {
                                // Clicking any radio chip just sets that value (clicking the
                                // already-active chip is a no-op — it never cycles).
                                Some(option_idx) => app.set_setting_option(row_idx, option_idx),
                                // Clicking the row label cycles to the next value (left→right,
                                // wrapping — e.g. the 3-radio theme).
                                None => app.toggle_selected_setting(),
                            }
                        } else if !point_in(app.settings_area, mouse.column, mouse.row) {
                            app.show_settings = false;
                        }
                    }
                    continue;
                }

                // Confirmation dialog: the yes/no footer chips inject `y`/`n` (same handler as the
                // keys); the [x] button or a click outside cancels.
                if app.confirm.is_some() {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        if let Some(hint) = app.hint_at(mouse.column, mouse.row) {
                            synthetic_keys.push_back(hint_key_event(hint));
                        } else if region_hit(app.confirm_close_click, mouse.column, mouse.row)
                            || !point_in(app.confirm_area, mouse.column, mouse.row)
                        {
                            app.confirm = None;
                        }
                    }
                    continue;
                }

                // Copy menu: click an option to copy it, [x] or outside to close.
                if app.copy_menu.is_some() {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        if region_hit(app.copy_menu_close_click, mouse.column, mouse.row)
                            || !point_in(app.copy_menu_area, mouse.column, mouse.row)
                        {
                            app.copy_menu = None;
                        } else if let Some(index) = app.copy_menu_option_at(mouse.row) {
                            app.copy_menu = Some(index);
                            let text = app.repo_page_target().map(|row| app.copy_menu_text(&row));
                            app.copy_menu = None;
                            if let Some(text) = text {
                                app.show_copy_toast(&text);
                                drop(app);
                                copy_to_clipboard(&text);
                            }
                        }
                    }
                    continue;
                }

                // Base-branch picker: click an option to set the override, [x] or outside closes.
                if app.base_picker.is_some() {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        if region_hit(app.base_picker_close_click, mouse.column, mouse.row)
                            || !point_in(app.base_picker_area, mouse.column, mouse.row)
                        {
                            app.base_picker = None;
                        } else if let Some(index) = app.base_picker_option_at(mouse.row) {
                            if let Some(picker) = app.base_picker.as_mut() {
                                picker.selected = index;
                            }
                            if let Some((repo_index, _)) = app.confirm_base_picker() {
                                let repo = Arc::clone(&app.repos[repo_index]);
                                drop(app);
                                tokio::spawn(run_branch_stats(repo));
                                continue;
                            }
                        }
                    }
                    continue;
                }

                // Diff modal: the wheel scrolls; clicks are ignored (esc/q closes it).
                // Skipped while help is open so the help overlay handles the mouse instead.
                if app.diff_modal.is_some() && !app.show_help {
                    let files_area = app.diff_files_area;
                    // Shift/Alt+wheel scrolls the file-list view (selection unchanged); a plain
                    // wheel over the file list moves the selection, and over the diff scrolls it.
                    // (Some terminals don't report Shift on the wheel, so Alt works too.)
                    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT)
                        || mouse.modifiers.contains(KeyModifiers::ALT);
                    let over_files = mouse.row >= files_area.y
                        && mouse.row < files_area.y + files_area.height;
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            if shift {
                                app.diff_files_scroll(3);
                            } else if over_files {
                                if app.diff_modal_select(1) {
                                    drop(app);
                                    tokio::spawn(run_diff_modal_file(Arc::clone(&app_state)));
                                    continue;
                                }
                            } else if let Some(modal) = app.diff_modal.as_mut() {
                                modal.scroll = modal.scroll.saturating_add(3);
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if shift {
                                app.diff_files_scroll(-3);
                            } else if over_files {
                                if app.diff_modal_select(-1) {
                                    drop(app);
                                    tokio::spawn(run_diff_modal_file(Arc::clone(&app_state)));
                                    continue;
                                }
                            } else if let Some(modal) = app.diff_modal.as_mut() {
                                modal.scroll = modal.scroll.saturating_sub(3);
                            }
                        }
                        // Click a status chip to filter, a file row to view its diff; [x] or
                        // outside the modal closes it.
                        MouseEventKind::Down(MouseButton::Left) => {
                            if region_hit(app.diff_modal_close_click, mouse.column, mouse.row)
                                || !point_in(app.diff_modal_area, mouse.column, mouse.row)
                            {
                                app.diff_modal = None;
                            } else if let Some(bucket) = app.diff_chip_at(mouse.column, mouse.row) {
                                if app.diff_modal_set_filter(bucket) {
                                    drop(app);
                                    tokio::spawn(run_diff_modal_file(Arc::clone(&app_state)));
                                    continue;
                                }
                            } else if let Some(index) = app.diff_modal_file_at(mouse.row) {
                                if app.diff_modal_select_index(index) {
                                    drop(app);
                                    tokio::spawn(run_diff_modal_file(Arc::clone(&app_state)));
                                    continue;
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Repo page: the wheel scrolls; a click selects a row, a double-click opens a
                // diff modal on a stash or a dirty branch/worktree. When restored, only events
                // inside the dock panel are handled here — events above it fall through to the
                // list/preview so the restored panel is master-detail (panel [4]).
                let in_repo_page = app.repo_page.is_some()
                    && !app.show_help
                    && (app.repo_page_maximized
                        || point_in(app.dock_rect, mouse.column, mouse.row));
                if in_repo_page {
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            let step = wheel_step(
                                mouse.modifiers,
                                3,
                                app.repo_page_inner.height as usize,
                            );
                            app.repo_page_scroll = app.repo_page_scroll.saturating_add(step);
                        }
                        MouseEventKind::ScrollUp => {
                            let step = wheel_step(
                                mouse.modifiers,
                                3,
                                app.repo_page_inner.height as usize,
                            );
                            app.repo_page_scroll = app.repo_page_scroll.saturating_sub(step);
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            // A click anywhere in the panel focuses it (panel [4]).
                            app.focus = Pane::RepoPage;
                            let tab_click = app
                                .repo_page_tab_click
                                .iter()
                                .find(|&&(row, start, end, _)| {
                                    mouse.row == row && mouse.column >= start && mouse.column < end
                                })
                                .map(|&(.., kind)| kind);
                            let pr_url = app
                                .repo_page_pr_click
                                .as_ref()
                                .filter(|&&(row, start, end, _)| {
                                    mouse.row == row && mouse.column >= start && mouse.column < end
                                })
                                .map(|(_, _, _, url)| url.clone());
                            if region_hit(app.repo_page_window_click, mouse.column, mouse.row) {
                                app.toggle_repo_page_maximized();
                            } else if region_hit(app.repo_page_back_click, mouse.column, mouse.row) {
                                app.close_repo_page();
                            } else if let Some((row, start, _)) =
                                app.page_cols_click.filter(|&(r, s, e)| {
                                    mouse.row == r && mouse.column >= s && mouse.column < e
                                })
                            {
                                app.open_dropdown(app::DropdownKind::PageColumns, start, row);
                            } else if let Some((row, start, _)) =
                                app.page_sort_click.filter(|&(r, s, e)| {
                                    mouse.row == r && mouse.column >= s && mouse.column < e
                                })
                            {
                                app.open_dropdown(app::DropdownKind::PageSort, start, row);
                            } else if let Some(url) = pr_url {
                                drop(app);
                                open_url(&url);
                                continue;
                            } else if let Some(kind) = tab_click {
                                app.repo_page_select_tab(kind);
                            } else if let Some(column) =
                                app.repo_page_toggle_at(mouse.column, mouse.row)
                            {
                                app.toggle_repo_page_column(column);
                                app.save_state();
                            } else if let Some(selection) =
                                app.base_cell_at(mouse.column, mouse.row)
                            {
                                app.repo_page_selected = selection;
                                app.open_base_picker(selection);
                            } else if let Some(sort) =
                                app.repo_page_sort_at(mouse.column, mouse.row)
                            {
                                app.set_repo_page_sort(sort);
                            } else if let Some(selection) = app.repo_page_row_at(mouse.row) {
                                app.repo_page_selected = selection;
                                let double = last_click
                                    .map(|(when, previous)| {
                                        previous == selection
                                            && when.elapsed() < Duration::from_millis(400)
                                    })
                                    .unwrap_or(false);
                                if double {
                                    last_click = None;
                                    if let Some(source) = app.diff_source_for_selected() {
                                        app.open_diff_modal(source);
                                        let app_state_clone = Arc::clone(&app_state);
                                        drop(app);
                                        tokio::spawn(run_diff_modal(app_state_clone));
                                        continue;
                                    }
                                } else {
                                    last_click = Some((Instant::now(), selection));
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Keyboard viewer: click a key to inspect it, [x]/outside closes, wheel scrolls
                // the actions panel. Sits above the help modal, so it's gated first.
                if app.show_keyboard {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if region_hit(app.keyboard_close_click, mouse.column, mouse.row)
                                || !point_in(app.keyboard_area, mouse.column, mouse.row)
                            {
                                app.show_keyboard = false;
                                app.keyboard_selected = None;
                                app.keyboard_scroll = 0;
                            } else if let Some(code) = app
                                .keyboard_key_click
                                .iter()
                                .find(|(row, start, end, _)| {
                                    *row == mouse.row
                                        && mouse.column >= *start
                                        && mouse.column < *end
                                })
                                .map(|(_, _, _, code)| *code)
                            {
                                app.keyboard_selected = Some(code);
                                app.keyboard_scroll = 0;
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            app.keyboard_scroll = app.keyboard_scroll.saturating_add(3);
                        }
                        MouseEventKind::ScrollUp => {
                            app.keyboard_scroll = app.keyboard_scroll.saturating_sub(3);
                        }
                        _ => {}
                    }
                    continue;
                }

                // Help modal: click a tab to switch, the [esc] button to close, or a link to open
                // it; the wheel scrolls.
                if app.show_help {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(tab) = app.help_tab_at(mouse.column, mouse.row) {
                                app.help_filter = None;
                                app.set_help_tab(tab);
                                app.help_scroll = 0;
                                app.save_state();
                            } else if region_hit(app.help_keyboard_click, mouse.column, mouse.row) {
                                app.show_keyboard = true;
                                app.keyboard_selected = None;
                                app.keyboard_scroll = 0;
                            } else if region_hit(app.help_maximize_click, mouse.column, mouse.row) {
                                app.help_maximized = !app.help_maximized;
                            } else if app.help_close_at(mouse.column, mouse.row)
                                || !point_in(app.help_area, mouse.column, mouse.row)
                            {
                                app.show_help = false;
                            } else if app.help_notes_toggle_row == Some(mouse.row) {
                                // Expand/collapse the Notes link group.
                                app.help_notes_expanded = !app.help_notes_expanded;
                            } else if region_hit(app.cli_copy_click, mouse.column, mouse.row) {
                                let command = app.cli_builder.command();
                                app.show_toast("command copied");
                                drop(app);
                                copy_to_clipboard(&command);
                            } else if let Some(mode) = app
                                .cli_helpmode_click
                                .iter()
                                .find(|&&(row, start, end, _)| {
                                    mouse.row == row && mouse.column >= start && mouse.column < end
                                })
                                .map(|&(.., mode)| mode)
                            {
                                // Click a help-display-mode chip (always / on hover / never).
                                app.cli_builder.help_mode = app::CliHelpMode::ALL[mode];
                                app.save_state();
                            } else if let Some(idx) = app
                                .cli_command_click
                                .iter()
                                .find(|&&(row, _)| row == mouse.row)
                                .map(|&(_, idx)| idx)
                            {
                                // Click a token in the built command to remove (uncheck) that flag.
                                app.cli_builder.set_on(idx, false);
                                app.cli_builder.selected = idx;
                            } else if let Some(idx) = app
                                .cli_flag_click
                                .iter()
                                .find(|&&(row, _)| row == mouse.row)
                                .map(|&(_, idx)| idx)
                            {
                                // Click a CLI-builder flag row: commit any edit, select, then toggle
                                // the checkbox (or edit a value flag).
                                app.cli_builder.editing = None;
                                app.cli_builder.selected = idx;
                                if app.cli_builder.enabled(idx) {
                                    match app::CLI_FLAGS[idx].kind {
                                        app::CliFlagKind::Toggle => app.cli_builder.toggle(idx),
                                        _ => {
                                            app.cli_builder.editing =
                                                Some(app.cli_builder.values[idx].clone());
                                        }
                                    }
                                }
                            } else if let Some((row_idx, option)) = app
                                .help_design_click
                                .iter()
                                .find(|&&(row, start, end, ..)| {
                                    mouse.row == row && mouse.column >= start && mouse.column < end
                                })
                                .map(|&(.., row_idx, option)| (row_idx, option))
                            {
                                // Design System radios reuse the settings dispatch: a radio chip
                                // just sets that value; only the row label cycles.
                                match option {
                                    Some(opt) => app.set_setting_option(row_idx, opt),
                                    None => {
                                        app.settings_selected = row_idx;
                                        app.toggle_selected_setting();
                                    }
                                }
                            } else if let Some(section) = app
                                .help_design_tab_click
                                .iter()
                                .find(|&&(row, start, end, _)| {
                                    mouse.row == row && mouse.column >= start && mouse.column < end
                                })
                                .map(|&(.., section)| section)
                            {
                                // Click a Design System vertical tab to switch sections.
                                app.design_section = section;
                                app.help_scroll = 0;
                            } else if region_hit(app.help_preview_click, mouse.column, mouse.row) {
                                // Open the shared confirm dialog as a live preview with its own
                                // unique copy (accepting is a no-op — it just closes).
                                app.confirm = Some(app::ConfirmDialog {
                                    message: "This is the polygit confirm dialog — one component, \
                                              reused everywhere."
                                        .to_string(),
                                    action: app::ConfirmAction::Preview,
                                    danger: false,
                                    restore_files: Vec::new(),
                                    delete_files: Vec::new(),
                                    detail_lines: vec![
                                        "Every yes/no prompt routes through it:".to_string(),
                                        "delete a branch · drop a stash · remove a worktree"
                                            .to_string(),
                                        "discard changes · reset settings".to_string(),
                                        "Its yes/no are the shared footer-chip buttons —".to_string(),
                                        "hover-highlighted, click or press y / n.".to_string(),
                                    ],
                                    detail_title: Some("Design system preview".to_string()),
                                });
                            } else if let Some(url) = app.help_link_at(mouse.row) {
                                drop(app);
                                open_url(&url);
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            app.help_scroll = app.help_scroll.saturating_add(3);
                        }
                        MouseEventKind::ScrollUp => {
                            app.help_scroll = app.help_scroll.saturating_sub(3);
                        }
                        _ => {}
                    }
                    continue;
                }

                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        // The list header's `[cols ▾]` / `[sort ▾]` chips open their dropdowns.
                        if let Some((row, start, _)) = app.list_cols_click.filter(|&(r, s, e)| {
                            mouse.row == r && mouse.column >= s && mouse.column < e
                        }) {
                            app.open_dropdown(app::DropdownKind::ListColumns, start, row);
                            continue;
                        }
                        if let Some((row, start, _)) = app.list_sort_click.filter(|&(r, s, e)| {
                            mouse.row == r && mouse.column >= s && mouse.column < e
                        }) {
                            app.open_dropdown(app::DropdownKind::ListSort, start, row);
                            continue;
                        }
                        // Click-to-focus: a click inside the panes focuses whichever panel it hit.
                        if point_in(app.list_area, mouse.column, mouse.row) {
                            app.focus_pane(Pane::List);
                        } else if point_in(app.preview_area, mouse.column, mouse.row) {
                            // The right pane stacks info (top) over result (bottom); the divider row
                            // splits them when both show, otherwise the visible one wins.
                            let above_divider = app
                                .preview_divider_row
                                .is_none_or(|divider| mouse.row < divider);
                            app.focus_pane(if above_divider { Pane::Info } else { Pane::Result });
                        }
                        // Footer status-bar commands are handled globally above (before the modal
                        // branches), so here we only handle the panes' own hits.
                        if let Some(column) = app.header_sort_at(mouse.column, mouse.row) {
                            // Click a column header to sort by it (re-click flips direction).
                            app.set_sort(column);
                        } else if let Some(action) = app.info_action_at(mouse.column, mouse.row) {
                            // Click an info-block link / copy button / expandable value.
                            match action {
                                InfoAction::OpenUrl(url) => open_url(&url),
                                InfoAction::CopyText(text) => {
                                    app.show_copy_toast(&text);
                                    copy_to_clipboard(&text);
                                }
                                InfoAction::ToggleExpand(field) => app.toggle_info_expanded(&field),
                            }
                        } else if let Some(repo_idx) = app
                            .fav_cell_click
                            .iter()
                            .find(|(row, start, end, _)| {
                                mouse.row == *row && mouse.column >= *start && mouse.column < *end
                            })
                            .map(|(_, _, _, repo_idx)| *repo_idx)
                        {
                            // Click the favorites column's star to toggle that repo's favorite.
                            app.toggle_favorite(repo_idx);
                        } else if let Some(url) = app
                            .pr_cell_click
                            .iter()
                            .find(|(row, start, end, _)| {
                                mouse.row == *row && mouse.column >= *start && mouse.column < *end
                            })
                            .map(|(_, _, _, url)| url.clone())
                        {
                            // Click the PR column's `#N` to open the pull request in the browser.
                            open_url(&url);
                        } else {
                            let on_divider = (i32::from(mouse.column)
                                - i32::from(app.divider_col))
                            .abs()
                                <= 1
                                && mouse.row >= app.main_area.y
                                && mouse.row < app.main_area.y + app.main_area.height;
                            if on_divider {
                                dragging_divider = true;
                            } else if let Some(selection) =
                                app.list_selection_at(mouse.column, mouse.row)
                            {
                                app.selected = selection;
                                app.user_navigated = true;
                                app.result_overlay = false;
                                app.right_view = RightView::Log;
                                if app.toggle_selected_header() {
                                    // Click a folder / group header: select it and toggle
                                    // collapse (no double-click semantics on headers).
                                    last_click = None;
                                } else {
                                    // Synthesize double-click → open the repo page.
                                    let double = last_click
                                        .map(|(when, previous)| {
                                            previous == selection
                                                && when.elapsed() < Duration::from_millis(400)
                                        })
                                        .unwrap_or(false);
                                    if double && app.selected_repo_index().is_some() {
                                        app.open_repo_page();
                                        last_click = None;
                                    } else {
                                        last_click = Some((Instant::now(), selection));
                                    }
                                }
                            }
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        if dragging_divider {
                            app.set_split_from_col(mouse.column);
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        dragging_divider = false;
                    }
                    MouseEventKind::ScrollUp => {
                        if mouse.column < app.divider_col {
                            // Plain wheel scrolls the list view (selection untouched, web-app
                            // style); Alt+wheel moves the selection like the Up key.
                            let viewport = app.list_rows_area.height as usize;
                            if mouse.modifiers.contains(KeyModifiers::ALT) {
                                app.nav_up();
                                app.ensure_list_selection_visible(viewport);
                            } else {
                                app.scroll_list(-WHEEL_LIST_STEP, viewport);
                            }
                        } else if let Some(repo_idx) = app.selected_repo_index() {
                            let step = wheel_step(mouse.modifiers, 3, app.preview_viewport);
                            let mut state = app.repos[repo_idx].lock().unwrap();
                            state.auto_scroll = false;
                            state.preview_scroll = state.preview_scroll.saturating_sub(step);
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if mouse.column < app.divider_col {
                            let viewport = app.list_rows_area.height as usize;
                            if mouse.modifiers.contains(KeyModifiers::ALT) {
                                app.nav_down();
                                app.ensure_list_selection_visible(viewport);
                            } else {
                                app.scroll_list(WHEEL_LIST_STEP, viewport);
                            }
                        } else if let Some(repo_idx) = app.selected_repo_index() {
                            // Clamp to the real content (works for log AND diff views) so wheel-up
                            // responds immediately instead of undoing invisible over-scroll.
                            let step = wheel_step(mouse.modifiers, 3, app.preview_viewport);
                            let max_scroll =
                                app.preview_total.saturating_sub(app.preview_viewport);
                            let mut state = app.repos[repo_idx].lock().unwrap();
                            state.auto_scroll = false;
                            state.preview_scroll = (state.preview_scroll + step).min(max_scroll);
                        }
                    }
                    _ => {}
                }
            }
            Event::Key(key) => {
                let mut app = app_state.lock().unwrap();

                // New-build notice: keyboard counterpart to the clickable `[reload]`/`[x]`. Handled
                // first so it works over any view or modal, mirroring the always-on mouse handler.
                // `Ctrl-R` reloads into the new build; `Ctrl-X` dismisses the notice.
                if app.update_available && !app.update_dismissed {
                    if key.code == KeyCode::Char('r')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(RELOAD_EXIT);
                    }
                    if key.code == KeyCode::Char('x')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        app.update_dismissed = true;
                        continue;
                    }
                }

                // Filter input mode
                if app.filter_input_mode {
                    match key.code {
                        KeyCode::Esc => app.cancel_filter_input(),
                        KeyCode::Enter => app.commit_filter_input(),
                        KeyCode::Backspace => {
                            if let Some(ref mut filter) = app.filter {
                                filter.pop();
                                if filter.is_empty() {
                                    app.filter = None;
                                }
                            }
                            app.select_first_filtered_row();
                        }
                        KeyCode::Char(ch) => {
                            app.filter.get_or_insert_with(String::new).push(ch);
                            app.select_first_filtered_row();
                        }
                        _ => {}
                    }
                    continue;
                }

                // Keyboard viewer: capture every keypress to highlight that key + list its
                // actions. Esc closes and stops listening; Ctrl-C still quits (safety, matching
                // every other modal here).
                if app.show_keyboard {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    if key.code == KeyCode::Esc {
                        app.show_keyboard = false;
                        app.keyboard_selected = None;
                        app.keyboard_scroll = 0;
                    } else if let Some(code) = keymap::keycode_to_code(key.code, key.modifiers) {
                        app.keyboard_selected = Some(code);
                        app.keyboard_scroll = 0;
                    }
                    continue;
                }

                // Confirmation dialog: y/Enter confirm, n/Esc/q cancel.
                if app.confirm.is_some() {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Enter => {
                            let action = app.confirm.take().map(|dialog| dialog.action);
                            if let Some(action) = action {
                                drop(app);
                                spawn_confirm_action(&app_state, action);
                                continue;
                            }
                        }
                        KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Esc => {
                            app.confirm = None;
                        }
                        _ => {}
                    }
                    continue;
                }

                // Build-info modal: Ctrl-C quits, `r` exec-restarts, esc/q close. The settings
                // preview is a collapsible tree — j/k move, ←/→ collapse/expand, space/enter fold,
                // -/+ fold/unfold all (falls back to plain scroll when the file isn't valid JSON).
                if app.show_build_info {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    if key.code == KeyCode::Char('r') {
                        drop(app);
                        return Ok(RELOAD_EXIT);
                    }
                    let tree = app.build_info_tree.is_some();
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.show_build_info = false,
                        KeyCode::Char('j') | KeyCode::Down if tree => app.build_info_tree_move(1),
                        KeyCode::Char('k') | KeyCode::Up if tree => app.build_info_tree_move(-1),
                        KeyCode::PageDown if tree => app.build_info_tree_move(10),
                        KeyCode::PageUp if tree => app.build_info_tree_move(-10),
                        KeyCode::Char('g') | KeyCode::Home if tree => app.build_info_tree_selected = 0,
                        KeyCode::Char('G') | KeyCode::End if tree => app.build_info_tree_move(isize::MAX),
                        KeyCode::Right | KeyCode::Char('l') if tree => app.build_info_tree_expand(),
                        KeyCode::Left | KeyCode::Char('h') if tree => {
                            app.build_info_tree_collapse_or_parent()
                        }
                        KeyCode::Char(' ') | KeyCode::Enter if tree => {
                            app.build_info_toggle_selected()
                        }
                        KeyCode::Char('-') if tree => app.build_info_fold_all(false),
                        KeyCode::Char('+') | KeyCode::Char('=') if tree => {
                            app.build_info_fold_all(true)
                        }
                        // Plain-scroll fallback (no tree).
                        KeyCode::Char('j') | KeyCode::Down => {
                            app.build_info_scroll = app.build_info_scroll.saturating_add(1)
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.build_info_scroll = app.build_info_scroll.saturating_sub(1)
                        }
                        KeyCode::PageDown => {
                            app.build_info_scroll = app.build_info_scroll.saturating_add(10)
                        }
                        KeyCode::PageUp => {
                            app.build_info_scroll = app.build_info_scroll.saturating_sub(10)
                        }
                        KeyCode::Char('g') | KeyCode::Home => app.build_info_scroll = 0,
                        _ => {}
                    }
                    continue;
                }

                // Changelog / What's New modal: Ctrl-C quits, esc/q closes. Full changelog: j/k move
                // the selected release, space/enter folds it, g/G jump. What's New: pure scroll.
                if app.show_changelog {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    let releases = changelog::releases();
                    let last = releases.len().saturating_sub(1);
                    let whats_new = app.changelog_whats_new;
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.show_changelog = false,
                        KeyCode::Char('j') | KeyCode::Down => {
                            if whats_new {
                                app.changelog_scroll = app.changelog_scroll.saturating_add(1);
                            } else {
                                app.changelog_selected = (app.changelog_selected + 1).min(last);
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            if whats_new {
                                app.changelog_scroll = app.changelog_scroll.saturating_sub(1);
                            } else {
                                app.changelog_selected = app.changelog_selected.saturating_sub(1);
                            }
                        }
                        KeyCode::PageDown => {
                            app.changelog_scroll = app.changelog_scroll.saturating_add(10);
                        }
                        KeyCode::PageUp => {
                            app.changelog_scroll = app.changelog_scroll.saturating_sub(10);
                        }
                        KeyCode::Char('g') | KeyCode::Home => {
                            if whats_new {
                                app.changelog_scroll = 0;
                            } else {
                                app.changelog_selected = 0;
                            }
                        }
                        KeyCode::Char('G') | KeyCode::End => {
                            if whats_new {
                                app.changelog_scroll = usize::MAX;
                            } else {
                                app.changelog_selected = last;
                            }
                        }
                        KeyCode::Char(' ') | KeyCode::Enter if !whats_new => {
                            if let Some(release) = releases.get(app.changelog_selected) {
                                let version = release.version.to_string();
                                app.toggle_changelog_release(&version);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Header dropdown (`[cols ▾]` / `[sort ▾]`): arrows move, space/enter activate
                // (columns stay open, sort closes), esc/q close.
                if app.dropdown.is_some() {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.close_dropdown(),
                        KeyCode::Down | KeyCode::Char('j') => app.dropdown_move(1),
                        KeyCode::Up | KeyCode::Char('k') => app.dropdown_move(-1),
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            let index = app.dropdown.map_or(0, |dropdown| dropdown.selected);
                            if app.dropdown_activate(index) {
                                app.close_dropdown();
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Settings modal (`,`): j/k move, space/enter toggle, esc/q/, close. Works over
                // both the main list and the repo page since it's gated before either.
                if app.show_settings {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    // `R` (or the footer `R reset` chip) resets all settings to defaults via a
                    // confirmation — handled before search so the chip works even while typing.
                    if let KeyCode::Char('R') = key.code {
                        if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
                            app.open_settings_reset_confirm();
                            continue;
                        }
                    }
                    // Search input focused: typing edits the query; Enter/Down jumps to the results;
                    // Esc clears + unfocuses; everything else is captured as text.
                    if app.settings_search_focused {
                        match key.code {
                            KeyCode::Esc => app.settings_clear_search(),
                            KeyCode::Enter | KeyCode::Down | KeyCode::Up => {
                                app.settings_search_focused = false;
                            }
                            KeyCode::Backspace => app.settings_search_backspace(),
                            KeyCode::Char(ch)
                                if !key.modifiers.intersects(
                                    KeyModifiers::CONTROL | KeyModifiers::ALT,
                                ) =>
                            {
                                app.settings_search_push(ch);
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match key.code {
                        // `/` focuses the search box (filters rows across all tabs).
                        KeyCode::Char('/') => app.settings_begin_search(),
                        KeyCode::Esc => {
                            if app.settings_search.is_empty() {
                                app.show_settings = false;
                            } else {
                                app.settings_clear_search();
                            }
                        }
                        KeyCode::Char('q') | KeyCode::Char(',') => {
                            app.show_settings = false;
                        }
                        KeyCode::Char('j') | KeyCode::Down => app.settings_move(1),
                        KeyCode::Char('k') | KeyCode::Up => app.settings_move(-1),
                        // In accordion mode ←/→ collapse/expand the selected section; in tabbed
                        // mode they switch tabs; in flat mode they do nothing.
                        KeyCode::Right | KeyCode::Tab => {
                            if app.settings_layout == crate::app::SettingsLayout::Accordion {
                                app.set_selected_settings_section(false);
                            } else {
                                app.settings_cycle_tab(true);
                            }
                        }
                        KeyCode::Left | KeyCode::BackTab => {
                            if app.settings_layout == crate::app::SettingsLayout::Accordion {
                                app.set_selected_settings_section(true);
                            } else {
                                app.settings_cycle_tab(false);
                            }
                        }
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            // In accordion mode, enter/space on a header expands/collapses it; on a
                            // row it toggles the setting. Other layouts always toggle the row.
                            if app.settings_layout == crate::app::SettingsLayout::Accordion
                                && app.settings_on_header.is_some()
                            {
                                app.toggle_focused_accordion_section();
                            } else {
                                app.toggle_selected_setting();
                            }
                        }
                        // `v` cycles the tabbed → accordion → flat layout (hint in the bottom border).
                        KeyCode::Char('v') => {
                            app.settings_layout = app.settings_layout.cycle();
                            app.settings_tab =
                                AppState::settings_tab_of_row(app.settings_selected);
                            // Entering accordion focuses the current row's section header; leaving
                            // clears the header focus (other layouts select rows).
                            app.settings_on_header =
                                (app.settings_layout == crate::app::SettingsLayout::Accordion)
                                    .then(|| AppState::settings_tab_of_row(app.settings_selected));
                            app.settings_scroll = 0;
                            app.save_state();
                        }
                        _ => {}
                    }
                    continue;
                }

                // Copy menu (`y` on the repo page): pick path / branch / both, then copy.
                if app.copy_menu.is_some() {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('y') => {
                            app.copy_menu = None;
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            let next = app.copy_menu.unwrap_or(0) + 1;
                            if next < AppState::COPY_MENU_ROWS {
                                app.copy_menu = Some(next);
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.copy_menu = Some(app.copy_menu.unwrap_or(0).saturating_sub(1));
                        }
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            let text =
                                app.repo_page_target().map(|row| app.copy_menu_text(&row));
                            app.copy_menu = None;
                            if let Some(text) = text {
                                app.show_copy_toast(&text);
                                drop(app);
                                copy_to_clipboard(&text);
                                continue;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Base-branch picker (`b` on the repo page): choose a base / auto-detect, then
                // recompute that branch's stats against it.
                // Folder picker (`A`): type to filter, ↑↓ move, Enter open-folder/select-repo,
                // ←/Backspace parent, ^S select the current folder, ^B bookmark, ^H home, Esc cancel.
                if app.picker.is_some() {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    let outcome = app.picker.as_mut().unwrap().on_key(key);
                    match outcome {
                        tui_pick::picker::PickerOutcome::Pending => {}
                        tui_pick::picker::PickerOutcome::Cancelled => {
                            app.sync_picker_bookmarks();
                            app.picker = None;
                        }
                        tui_pick::picker::PickerOutcome::Selected(path) => {
                            app.sync_picker_bookmarks();
                            app.picker = None;
                            if let Some(abs) = app.add_root(path) {
                                let throttle = app.throttle.clone();
                                let max_jobs = app.max_jobs;
                                let depth = app.discovery_max_depth;
                                let timeout = app.discovery_timeout_secs;
                                let icons = app.icon_style;
                                let no_wt = app.discovery_no_worktrees;
                                drop(app);
                                tokio::spawn(run_discovery(
                                    Arc::clone(&app_state),
                                    vec![abs],
                                    depth,
                                    throttle,
                                    max_jobs,
                                    timeout,
                                    icons,
                                    no_wt,
                                    false,
                                ));
                                continue;
                            }
                        }
                    }
                    continue;
                }

                // Fuzzy finder overlay (`P`): type to filter, ↑↓/PgUp/PgDn to move, ^S to cycle
                // sort, Enter to jump the list to that repo (records the visit), Esc to close.
                if app.finder.is_some() {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    // Take the finder out so on_key can borrow app.finder_history (the MutexGuard
                    // deref makes field borrows non-disjoint otherwise); put it back unless it closed.
                    let mut finder = app.finder.take().unwrap();
                    let outcome = finder.on_key(key, &app.finder_history);
                    match outcome {
                        tui_pick::finder::FinderOutcome::Cancelled => {}
                        tui_pick::finder::FinderOutcome::Accepted { key, .. } => {
                            app.finder_jump(&key);
                        }
                        tui_pick::finder::FinderOutcome::Pending => app.finder = Some(finder),
                    }
                    continue;
                }

                if app.base_picker.is_some() {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.base_picker = None,
                        KeyCode::Char('j') | KeyCode::Down => app.move_base_picker(1),
                        KeyCode::Char('k') | KeyCode::Up => app.move_base_picker(-1),
                        KeyCode::Char('g') | KeyCode::Home => app.move_base_picker(isize::MIN),
                        KeyCode::Char('G') | KeyCode::End => app.move_base_picker(isize::MAX),
                        KeyCode::Char(' ') | KeyCode::Enter => {
                            if let Some((repo_index, _)) = app.confirm_base_picker() {
                                let repo = Arc::clone(&app.repos[repo_index]);
                                drop(app);
                                tokio::spawn(run_branch_stats(repo));
                                continue;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Diff modal: scroll, toggle the dirty-diff mode, or close. Skipped while help is
                // open so the help overlay (gated below) handles keys instead.
                if app.diff_modal.is_some() && !app.show_help {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    let page = app.diff_modal_viewport.max(1);
                    let scroll_by = |app: &mut AppState, delta: isize| {
                        if let Some(modal) = app.diff_modal.as_mut() {
                            modal.scroll = if delta < 0 {
                                modal.scroll.saturating_sub((-delta) as usize)
                            } else {
                                modal.scroll.saturating_add(delta as usize)
                            };
                        }
                    };
                    // Re-fetch the selected file's diff after a selection change.
                    let refetch_file = |app_state: &Arc<Mutex<AppState>>| {
                        tokio::spawn(run_diff_modal_file(Arc::clone(app_state)));
                    };
                    let last_file = app
                        .diff_modal
                        .as_ref()
                        .map(|modal| modal.files.len().saturating_sub(1));
                    let focus = app.diff_modal.as_ref().map(|modal| modal.focus).unwrap_or_default();
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.diff_modal = None,
                        // `?` opens help (the overlay shows the diff-modal hotkeys).
                        KeyCode::Char('?') => app.open_help(),
                        // Tab switches which panel j/k/g/G drive (file list ⇄ diff).
                        KeyCode::Tab | KeyCode::BackTab => app.diff_modal_toggle_focus(),
                        // j/k/↑/↓ drive the focused panel: pick a file, or scroll the diff.
                        KeyCode::Char('j') | KeyCode::Down => {
                            if focus == DiffFocus::Files {
                                if app.diff_modal_select(1) {
                                    drop(app);
                                    refetch_file(&app_state);
                                    continue;
                                }
                            } else {
                                scroll_by(&mut app, 1);
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            if focus == DiffFocus::Files {
                                if app.diff_modal_select(-1) {
                                    drop(app);
                                    refetch_file(&app_state);
                                    continue;
                                }
                            } else {
                                scroll_by(&mut app, -1);
                            }
                        }
                        KeyCode::Char('g') | KeyCode::Home => {
                            if focus == DiffFocus::Files {
                                if app.diff_modal_select_index(0) {
                                    drop(app);
                                    refetch_file(&app_state);
                                    continue;
                                }
                            } else if let Some(modal) = app.diff_modal.as_mut() {
                                modal.scroll = 0;
                            }
                        }
                        KeyCode::Char('G') | KeyCode::End => {
                            if focus == DiffFocus::Files {
                                if let Some(last) = last_file {
                                    if app.diff_modal_select_index(last) {
                                        drop(app);
                                        refetch_file(&app_state);
                                        continue;
                                    }
                                }
                            } else if let Some(modal) = app.diff_modal.as_mut() {
                                modal.scroll = usize::MAX;
                            }
                        }
                        // Shift/Alt+Page pages the file list (selection moves a viewport at a time).
                        KeyCode::PageDown | KeyCode::PageUp
                            if key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
                        {
                            let step = app.diff_files_viewport.max(1) as isize;
                            let delta = if key.code == KeyCode::PageUp { -step } else { step };
                            if app.diff_modal_select(delta) {
                                drop(app);
                                refetch_file(&app_state);
                                continue;
                            }
                        }
                        // Plain Page keys always scroll the diff panel.
                        KeyCode::PageDown => scroll_by(&mut app, isize::try_from(page).unwrap_or(isize::MAX)),
                        KeyCode::PageUp => scroll_by(&mut app, -isize::try_from(page).unwrap_or(isize::MAX)),
                        // `f` cycles the status filter (all → each present status → all).
                        KeyCode::Char('f') => {
                            if app.diff_modal_cycle_filter() {
                                drop(app);
                                refetch_file(&app_state);
                                continue;
                            }
                        }
                        // `t` toggles the dirty-diff mode (uncommitted ⇄ base branch).
                        KeyCode::Char('t') => {
                            if app.diff_modal_toggle_mode() {
                                let app_state_clone = Arc::clone(&app_state);
                                drop(app);
                                tokio::spawn(run_diff_modal(app_state_clone));
                                continue;
                            }
                        }
                        // Clear/delete what the modal is showing: close the modal, then raise the
                        // confirm dialog over the repo page.
                        KeyCode::Char('d') => {
                            let source = app.diff_modal.as_ref().map(|modal| modal.source.clone());
                            match source {
                                // A branch diff is read-only — `d` does nothing (modal stays open).
                                Some(DiffSource::Branch { .. }) | None => {}
                                Some(source) => {
                                    app.diff_modal = None;
                                    if let Some(idx) = app.repo_page {
                                        let repo_path = app.repos[idx].lock().unwrap().path.clone();
                                        match source {
                                            DiffSource::Stash { index, .. } => {
                                                let app_state_clone = Arc::clone(&app_state);
                                                drop(app);
                                                tokio::spawn(run_prepare_drop_stash(app_state_clone, idx, index));
                                                continue;
                                            }
                                            // The checked-out branch: discard its uncommitted changes.
                                            DiffSource::Dirty { path, .. } if path == repo_path => {
                                                let app_state_clone = Arc::clone(&app_state);
                                                drop(app);
                                                tokio::spawn(run_prepare_discard(app_state_clone, idx, path));
                                                continue;
                                            }
                                            DiffSource::Dirty { path, .. } => {
                                                app.confirm = Some(ConfirmDialog::simple(
                                                    format!(
                                                        "Remove worktree {}? Uncommitted changes will be LOST.",
                                                        path.display()
                                                    ),
                                                    ConfirmAction::RemoveWorktree {
                                                        repo_idx: idx,
                                                        path,
                                                        force: true,
                                                    },
                                                    true,
                                                ));
                                            }
                                            DiffSource::Branch { .. } => {}
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Dedicated repo page: navigate branches/worktrees and act on the selected row.
                // Restored, this only takes keys when panel [4] holds focus — otherwise keys drive
                // the list/preview (master-detail). Maximized, it's the only panel, so it always wins.
                if app.repo_page.is_some()
                    && !app.show_help
                    && (app.repo_page_maximized || app.focus == Pane::RepoPage)
                {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    // Column-toggle menu: chip keys flip a column (stay in mode); esc/t close;
                    // any other key exits and falls through to normal handling.
                    if app.repo_page_toggle {
                        let column = match key.code {
                            KeyCode::Char('b') => Some(RepoPageColumn::AheadBehind),
                            KeyCode::Char('y') => Some(RepoPageColumn::Dirty),
                            KeyCode::Char('a') => Some(RepoPageColumn::Added),
                            KeyCode::Char('m') => Some(RepoPageColumn::Modified),
                            KeyCode::Char('d') => Some(RepoPageColumn::Deleted),
                            KeyCode::Char('c') => Some(RepoPageColumn::Total),
                            KeyCode::Char('u') => Some(RepoPageColumn::Upstream),
                            KeyCode::Char('f') => Some(RepoPageColumn::Base),
                            KeyCode::Char('g') => Some(RepoPageColumn::Age),
                            KeyCode::Char('r') => Some(RepoPageColumn::PullRequest),
                            KeyCode::Char('s') => Some(RepoPageColumn::Subject),
                            _ => None,
                        };
                        if let Some(column) = column {
                            if app.repo_page_column_available(column) {
                                app.toggle_repo_page_column(column);
                                app.save_state();
                            } else {
                                app.show_toast("that column is empty for this repo");
                            }
                            continue;
                        }
                        app.repo_page_toggle = false;
                        if matches!(key.code, KeyCode::Char('t') | KeyCode::Esc) {
                            continue;
                        }
                        // Fall through: arrows/other keys exit the menu and act normally.
                    }
                    let len = app.repo_page_selectable_len();
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.close_repo_page(),
                        // `m` maximizes / restores the page (Windows-style window control).
                        KeyCode::Char('m') => app.toggle_repo_page_maximized(),
                        // `?` opens help (the overlay shows the repo-page hotkeys).
                        KeyCode::Char('?') => app.open_help(),
                        // `,` opens settings (handled by the early gate next iteration).
                        KeyCode::Char(',') => app.open_settings(),
                        // `t` opens the column-toggle menu; `i` toggles the info panel.
                        KeyCode::Char('t') => app.repo_page_toggle = true,
                        KeyCode::Char('i') => {
                            app.repo_page_info = !app.repo_page_info;
                            app.save_state();
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            if app.repo_page_selected + 1 < len {
                                app.repo_page_selected += 1;
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.repo_page_selected = app.repo_page_selected.saturating_sub(1);
                        }
                        KeyCode::Char('g') | KeyCode::Home => app.repo_page_selected = 0,
                        KeyCode::Char('G') | KeyCode::End => {
                            app.repo_page_selected = len.saturating_sub(1)
                        }
                        // Tab / Shift+Tab switch repo-page tabs (when tabbed).
                        KeyCode::Tab => app.repo_page_cycle_tab(true),
                        KeyCode::BackTab => app.repo_page_cycle_tab(false),
                        KeyCode::PageDown => {
                            app.repo_page_scroll = app.repo_page_scroll.saturating_add(10);
                        }
                        KeyCode::PageUp => {
                            app.repo_page_scroll = app.repo_page_scroll.saturating_sub(10);
                        }
                        // Shift+Enter checks out the selected (clean, non-HEAD) branch.
                        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                            if let (Some(idx), Some(row)) = (app.repo_page, app.repo_page_target()) {
                                if row.kind == PageRowKind::Branch && !row.is_head {
                                    let app_state_clone = Arc::clone(&app_state);
                                    drop(app);
                                    tokio::spawn(run_checkout(app_state_clone, idx, row.branch));
                                    continue;
                                }
                            }
                        }
                        // Enter (or Space) on a stash or a dirty row opens its diff modal;
                        // Shift+Enter checks a branch out instead (handled above).
                        KeyCode::Enter | KeyCode::Char(' ') => {
                            if let Some(source) = app.diff_source_for_selected() {
                                app.open_diff_modal(source);
                                let app_state_clone = Arc::clone(&app_state);
                                drop(app);
                                tokio::spawn(run_diff_modal(app_state_clone));
                                continue;
                            }
                        }
                        // Clear/delete the selected row (stash drop / worktree remove / branch
                        // delete) after a confirmation dialog whose severity scales with danger.
                        KeyCode::Char('d') => {
                            if let (Some(idx), Some(row)) = (app.repo_page, app.repo_page_target()) {
                                // Stash drop gathers the stash's files for the confirm dialog.
                                if let (PageRowKind::Stash, Some(index)) = (row.kind, row.stash_index) {
                                    let app_state_clone = Arc::clone(&app_state);
                                    drop(app);
                                    tokio::spawn(run_prepare_drop_stash(app_state_clone, idx, index));
                                    continue;
                                }
                                if let Some(dialog) = confirm_for_row(idx, &row) {
                                    app.confirm = Some(dialog);
                                } else if row.kind == PageRowKind::Branch && row.is_head {
                                    if row.dirty {
                                        let app_state_clone = Arc::clone(&app_state);
                                        drop(app);
                                        tokio::spawn(run_prepare_discard(app_state_clone, idx, row.path));
                                        continue;
                                    }
                                    app.repo_page_message =
                                        Some("can't delete the current branch".to_string());
                                }
                            }
                        }
                        // Start claude code in the selected row's path.
                        KeyCode::Char('c') => {
                            if let Some(row) = app.repo_page_target() {
                                pending_claude = Some(row.path);
                            }
                        }
                        // Open lazygit in the selected row's path.
                        KeyCode::Char('l') => {
                            if let Some(row) = app.repo_page_target() {
                                pending_lazygit = Some(row.path);
                            }
                        }
                        // Open the copy menu (pick path / branch / both).
                        KeyCode::Char('y') => {
                            if app.repo_page_target().is_some() {
                                app.copy_menu = Some(0);
                            }
                        }
                        // Open the base-branch picker for the selected branch (override which base
                        // its diff stats compare against; no-op on non-branch rows).
                        KeyCode::Char('b') => {
                            let selection = app.repo_page_selected;
                            app.open_base_picker(selection);
                        }
                        // Open the selected branch on the remote host.
                        KeyCode::Char('o') => {
                            if let (Some(idx), Some(row)) = (app.repo_page, app.repo_page_target()) {
                                let url = app.repos[idx].lock().unwrap().remote_url.clone();
                                if let Some(url) = url {
                                    let branch_url = format!("{url}/tree/{}", row.branch);
                                    drop(app);
                                    open_url(&branch_url);
                                    continue;
                                }
                            }
                        }
                        // Fast-forward the selected branch/worktree.
                        KeyCode::Char('p') => {
                            if let (Some(idx), Some(row)) = (app.repo_page, app.repo_page_target()) {
                                let app_state_clone = Arc::clone(&app_state);
                                drop(app);
                                tokio::spawn(run_pull_branch(app_state_clone, idx, row));
                                continue;
                            }
                        }
                        // Fast-forward every fast-forwardable local branch in the repo.
                        KeyCode::Char('P') => {
                            if let Some(idx) = app.repo_page {
                                let loaded = {
                                    let state = app.repos[idx].lock().unwrap();
                                    state.page.is_some() && !state.page_loading
                                };
                                if loaded {
                                    let app_state_clone = Arc::clone(&app_state);
                                    drop(app);
                                    tokio::spawn(run_pull_all_branches(app_state_clone, idx));
                                    continue;
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Help modal: swallow keys while open (scroll or close).
                if app.show_help {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        drop(app);
                        return Ok(130);
                    }
                    // `/` search applies to every tab. While active, printable chars edit the query
                    // (Esc clears, Backspace deletes); arrows/Tab fall through to scroll/switch.
                    if app.help_filter.is_some() {
                        match key.code {
                            KeyCode::Esc => {
                                app.help_filter = None;
                                continue;
                            }
                            KeyCode::Backspace => {
                                if let Some(query) = app.help_filter.as_mut() {
                                    query.pop();
                                }
                                continue;
                            }
                            KeyCode::Char(ch)
                                if !key.modifiers.intersects(
                                    KeyModifiers::CONTROL | KeyModifiers::ALT,
                                ) =>
                            {
                                if let Some(query) = app.help_filter.as_mut() {
                                    query.push(ch);
                                }
                                app.help_scroll = 0;
                                continue;
                            }
                            _ => {}
                        }
                    } else if key.code == KeyCode::Char('/') {
                        app.help_filter = Some(String::new());
                        app.help_scroll = 0;
                        continue;
                    }
                    // Interactive CLI builder (CLI & Flags tab) intercepts its own keys — but not
                    // while a search is active (then typing edits the query, handled above).
                    if app.help_tab == app::HelpTab::CliFlags && app.help_filter.is_none() {
                        if app.cli_builder.editing.is_some() {
                            let idx = app.cli_builder.selected;
                            match key.code {
                                // Esc / Enter just leave edit mode — the value is already applied.
                                KeyCode::Esc | KeyCode::Enter => app.cli_builder.editing = None,
                                KeyCode::Backspace => {
                                    if let Some(buffer) = app.cli_builder.editing.as_mut() {
                                        buffer.pop();
                                    }
                                }
                                KeyCode::Char(ch)
                                    if !key.modifiers.intersects(
                                        KeyModifiers::CONTROL | KeyModifiers::ALT,
                                    ) =>
                                {
                                    if let Some(buffer) = app.cli_builder.editing.as_mut() {
                                        buffer.push(ch);
                                    }
                                }
                                _ => {}
                            }
                            // Auto-apply live: the command updates as you type; a non-empty value
                            // checks the flag on so it's included.
                            if let Some(buffer) = app.cli_builder.editing.clone() {
                                let value = buffer.trim().to_string();
                                let non_empty = !value.is_empty();
                                app.cli_builder.values[idx] = value;
                                if non_empty {
                                    app.cli_builder.set_on(idx, true);
                                }
                            }
                            continue;
                        }
                        let idx = app.cli_builder.selected;
                        match key.code {
                            KeyCode::Down | KeyCode::Char('j') => {
                                let last = app::CLI_FLAGS.len().saturating_sub(1);
                                app.cli_builder.selected = (idx + 1).min(last);
                                continue;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.cli_builder.selected = idx.saturating_sub(1);
                                continue;
                            }
                            // Space toggles the checkbox (cascades to children when unchecked).
                            KeyCode::Char(' ') => {
                                app.cli_builder.toggle(idx);
                                continue;
                            }
                            // Enter edits a value/positional flag; on a toggle it flips the checkbox.
                            KeyCode::Enter => {
                                if app.cli_builder.enabled(idx) {
                                    match app::CLI_FLAGS[idx].kind {
                                        app::CliFlagKind::Toggle => app.cli_builder.toggle(idx),
                                        _ => {
                                            app.cli_builder.editing =
                                                Some(app.cli_builder.values[idx].clone());
                                        }
                                    }
                                }
                                continue;
                            }
                            // `f` swaps the selected flag's short/long form.
                            KeyCode::Char('f') => {
                                app.cli_builder.toggle_short(idx);
                                continue;
                            }
                            KeyCode::Char('y') => {
                                let command = app.cli_builder.command();
                                app.show_toast("command copied");
                                drop(app);
                                copy_to_clipboard(&command);
                                continue;
                            }
                            // `h` cycles the help-display mode (always / on hover / never).
                            KeyCode::Char('h') => {
                                app.cli_builder.help_mode = app.cli_builder.help_mode.cycle();
                                app.save_state();
                                continue;
                            }
                            _ => {}
                        }
                    }
                    match key.code {
                        KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => {
                            app.show_help = false;
                        }
                        // Tab / Shift+Tab cycle help tabs; the choice is persisted so it reopens here.
                        KeyCode::Tab => {
                            app.help_filter = None;
                            let next = app.help_tab.next();
                            app.set_help_tab(next);
                            app.help_scroll = 0;
                            app.save_state();
                        }
                        KeyCode::BackTab => {
                            app.help_filter = None;
                            let prev = app.help_tab.prev();
                            app.set_help_tab(prev);
                            app.help_scroll = 0;
                            app.save_state();
                        }
                        // `K` pops the interactive keyboard viewer (same as clicking [K ⌨ keyboard]).
                        KeyCode::Char('K') => {
                            app.show_keyboard = true;
                            app.keyboard_selected = None;
                            app.keyboard_scroll = 0;
                        }
                        // `m` maximizes/restores the modal (same as clicking the button).
                        KeyCode::Char('m') => app.help_maximized = !app.help_maximized,
                        // Design System tab: `v` cycles flat/tabbed; in tabbed, `[`/`]` move sections.
                        KeyCode::Char('v') if app.help_tab == app::HelpTab::DesignSystem => {
                            app.design_layout = app.design_layout.cycle();
                            app.help_scroll = 0;
                            app.save_state();
                        }
                        KeyCode::Char(']')
                            if app.help_tab == app::HelpTab::DesignSystem
                                && app.design_layout == app::DesignLayout::Tabbed =>
                        {
                            app.design_section = (app.design_section + 1) % app::DESIGN_SECTIONS.len();
                            app.help_scroll = 0;
                        }
                        KeyCode::Char('[')
                            if app.help_tab == app::HelpTab::DesignSystem
                                && app.design_layout == app::DesignLayout::Tabbed =>
                        {
                            app.design_section =
                                (app.design_section + app::DESIGN_SECTIONS.len() - 1) % app::DESIGN_SECTIONS.len();
                            app.help_scroll = 0;
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            app.help_scroll = app.help_scroll.saturating_add(1);
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.help_scroll = app.help_scroll.saturating_sub(1);
                        }
                        KeyCode::PageDown => {
                            app.help_scroll = app.help_scroll.saturating_add(10);
                        }
                        KeyCode::PageUp => {
                            app.help_scroll = app.help_scroll.saturating_sub(10);
                        }
                        KeyCode::Char('g') | KeyCode::Home => app.help_scroll = 0,
                        KeyCode::Char('G') | KeyCode::End => app.help_scroll = usize::MAX,
                        KeyCode::Char('D') => {
                            drop(app);
                            open_url(render::DOCS_URL);
                            continue;
                        }
                        _ => {}
                    }
                    continue;
                }

                // `t` toggle mode: stays active so multiple columns can be toggled (a/d/l/w/b/s);
                // `t` again or Esc exits. Navigation keys (up/down/home/end/enter) exit the mode
                // and then run normally (fall through); any other key is swallowed.
                if app.pending_leader == Some(Leader::Toggle) {
                    // Toggle a column unless its data is trivially empty — then explain why.
                    let toggle_or_warn = |app: &mut AppState, column: Column, noun: &str| {
                        if app.column_available(column) {
                            app.toggle_column(column);
                        } else {
                            app.show_toast(format!("no repo has any {noun} — nothing to show"));
                        }
                    };
                    match key.code {
                        KeyCode::Char('u') => app.toggle_column(Column::Status),
                        KeyCode::Char('a') => app.toggle_column(Column::AheadBehind),
                        KeyCode::Char('d') => app.toggle_column(Column::Dirty),
                        KeyCode::Char('l') => app.toggle_column(Column::LastCommit),
                        KeyCode::Char('w') => toggle_or_warn(&mut app, Column::Worktrees, "worktrees"),
                        KeyCode::Char('b') => toggle_or_warn(&mut app, Column::Branches, "extra branches"),
                        KeyCode::Char('s') => toggle_or_warn(&mut app, Column::Stashes, "stashes"),
                        KeyCode::Char('p') => toggle_or_warn(&mut app, Column::PulledCommits, "pulled commits"),
                        KeyCode::Char('c') => toggle_or_warn(&mut app, Column::PulledFiles, "changed files"),
                        KeyCode::Char('r') => app.toggle_column(Column::PullRequest),
                        KeyCode::Char('f') => app.toggle_column(Column::Favorite),
                        KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End | KeyCode::Enter => {
                            // Exit toggle mode and let the key run normally below.
                            app.pending_leader = None;
                        }
                        _ => {
                            // `t` again, Esc, or any other key: exit (or stay) without acting.
                            if matches!(key.code, KeyCode::Char('t') | KeyCode::Esc) {
                                app.pending_leader = None;
                            }
                            continue;
                        }
                    }
                    // Column toggles took their action above and stay in toggle mode.
                    if app.pending_leader == Some(Leader::Toggle) {
                        continue;
                    }
                }

                // `f` filter mode: pick one status filter (a/u/c/s/f/i), then exit. Esc cancels;
                // any other key just exits without changing the filter.
                if app.pending_leader == Some(Leader::Filter) {
                    let picked = match key.code {
                        KeyCode::Char('a') => Some(StatusFilter::All),
                        KeyCode::Char('u') => Some(StatusFilter::Updated),
                        KeyCode::Char('c') => Some(StatusFilter::UpToDate),
                        KeyCode::Char('s') => Some(StatusFilter::Skipped),
                        KeyCode::Char('f') => Some(StatusFilter::Failed),
                        KeyCode::Char('i') => Some(StatusFilter::Issues),
                        _ => None,
                    };
                    if let Some(filter) = picked {
                        app.set_status_filter(filter);
                    }
                    app.pending_leader = None;
                    continue;
                }

                // `s` sort mode: pick a sort column (re-pick flips direction), then exit. Esc
                // cancels; any other key exits without changing the sort.
                if app.pending_leader == Some(Leader::Sort) {
                    let picked = match key.code {
                        KeyCode::Char('n') => Some(SortColumn::Name),
                        KeyCode::Char('c') => Some(SortColumn::Branch),
                        KeyCode::Char('s') => Some(SortColumn::Status),
                        KeyCode::Char('a') => Some(SortColumn::AheadBehind),
                        KeyCode::Char('d') => Some(SortColumn::Dirty),
                        KeyCode::Char('l') => Some(SortColumn::LastCommit),
                        KeyCode::Char('w') => Some(SortColumn::Worktrees),
                        KeyCode::Char('b') => Some(SortColumn::Branches),
                        KeyCode::Char('k') => Some(SortColumn::Stashes),
                        KeyCode::Char('p') => Some(SortColumn::PulledCommits),
                        KeyCode::Char('g') => Some(SortColumn::PulledFiles),
                        KeyCode::Char('r') => Some(SortColumn::PullRequest),
                        _ => None,
                    };
                    if let Some(column) = picked {
                        app.set_sort(column);
                    }
                    app.pending_leader = None;
                    continue;
                }

                // `v` view mode: pick grouped (`g`) or tree (`t`), then exit. Esc/any other
                // key just closes the menu.
                if app.pending_leader == Some(Leader::View) {
                    match key.code {
                        KeyCode::Char('g') => app.toggle_grouping_view(),
                        KeyCode::Char('t') => app.toggle_tree_view(),
                        _ => {}
                    }
                    app.pending_leader = None;
                    continue;
                }

                // `z` fold mode (vim-style): za toggle · zo/zc open/close selected ·
                // zO expand subtree · zM collapse all · zR expand all. Esc/other closes.
                if app.pending_leader == Some(Leader::Fold) {
                    match key.code {
                        KeyCode::Char('a') => {
                            app.toggle_selected_header();
                        }
                        KeyCode::Char('o') => app.nav_right(),
                        KeyCode::Char('c') => app.nav_left(),
                        KeyCode::Char('O') => app.expand_subtree(),
                        KeyCode::Char('M') => app.collapse_all(),
                        KeyCode::Char('R') => app.expand_all(),
                        _ => {}
                    }
                    app.pending_leader = None;
                    continue;
                }

                // Normal key handling
                match (key.code, key.modifiers) {
                    // Quit — but if a restored panel [4] is open while another pane holds focus,
                    // Esc/q backs out of the panel first (rather than surprising the user with a quit).
                    (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => {
                        if app.repo_page.is_some() {
                            app.close_repo_page();
                        } else {
                            let all_done = app.all_done;
                            drop(app);
                            if all_done {
                                return Ok(compute_exit_code(&app_state));
                            } else {
                                return Ok(2); // user quit mid-run
                            }
                        }
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        drop(app);
                        return Ok(130);
                    }

                    // Navigation
                    (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
                        app.nav_down();
                    }
                    (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                        app.nav_up();
                    }
                    // Tree-style group navigation: ← jumps to the group header / collapses,
                    // → expands a collapsed group.
                    (KeyCode::Left, _) => {
                        app.nav_left();
                    }
                    (KeyCode::Right, _) => {
                        app.nav_right();
                    }
                    (KeyCode::Char('g'), _) => {
                        app.nav_top();
                    }
                    (KeyCode::Char('G'), _) => {
                        app.nav_bottom();
                    }

                    // Tab / Shift+Tab: cycle focus across the visible panels.
                    (KeyCode::Tab, _) => app.cycle_focus(true),
                    (KeyCode::BackTab, _) => app.cycle_focus(false),
                    // `1`-`4`: focus the list / info / result / repo-page panel directly (lazygit-style).
                    (KeyCode::Char('1'), _) => app.focus_pane(Pane::List),
                    (KeyCode::Char('2'), _) => app.focus_pane(Pane::Info),
                    (KeyCode::Char('3'), _) => app.focus_pane(Pane::Result),
                    (KeyCode::Char('4'), _) => app.focus_pane(Pane::RepoPage),

                    // Space: collapse/expand a selected group header, else toggle the Result
                    // preview overlay (temporary switch).
                    (KeyCode::Char(' '), _) => {
                        if !app.toggle_selected_header() {
                            app.result_overlay = !app.result_overlay;
                        }
                    }

                    // `v` leader: arm the view-mode chord (`g` grouped · `t` tree).
                    (KeyCode::Char('v'), _) => {
                        app.pending_leader = Some(Leader::View);
                    }
                    // `z` leader: arm the fold chord (za/zo/zc/zO/zM/zR).
                    (KeyCode::Char('z'), _) => {
                        app.pending_leader = Some(Leader::Fold);
                    }
                    // Direct fold keys: `-` collapse all · `+`/`=` expand all · `*` expand subtree.
                    (KeyCode::Char('-'), _) => app.collapse_all(),
                    (KeyCode::Char('+'), _) | (KeyCode::Char('='), _) => app.expand_all(),
                    (KeyCode::Char('*'), _) => app.expand_subtree(),
                    // `Z`: re-resolve dynamic (command/url) group memberships now.
                    (KeyCode::Char('Z'), _) => {
                        if app.any_dynamic_groups() {
                            for group in &mut app.groups {
                                if group.source.is_dynamic() {
                                    group.resolving = true;
                                }
                            }
                            drop(app);
                            tokio::spawn(groups::run_group_resolution(
                                Arc::clone(&app_state),
                                true,
                            ));
                        } else if !app.groups.is_empty() {
                            app.show_toast("no dynamic groups to refresh");
                        }
                    }

                    // Help modal
                    (KeyCode::Char('?'), _) => app.open_help(),

                    // `t` leader: arm the column-toggle chord (next key picks the column).
                    (KeyCode::Char('t'), _) => {
                        app.pending_leader = Some(Leader::Toggle);
                    }

                    // `s` leader: arm the sort chord (next key picks the sort column).
                    (KeyCode::Char('s'), _) => {
                        app.pending_leader = Some(Leader::Sort);
                    }

                    // `f` leader: arm the status-filter chord (next key picks the filter).
                    (KeyCode::Char('f'), _) => {
                        app.pending_leader = Some(Leader::Filter);
                    }

                    // `,` opens the settings modal.
                    (KeyCode::Char(','), _) => app.open_settings(),

                    // Resize the split: [ narrows the left pane, ] widens it.
                    (KeyCode::Char('['), _) => {
                        app.adjust_split(-0.03);
                    }
                    (KeyCode::Char(']'), _) => {
                        app.adjust_split(0.03);
                    }

                    // Filter
                    (KeyCode::Char('/'), _) => app.begin_filter_input(),

                    // Favorite the selected repo (★); `M` toggles the favorites-first pinned section.
                    (KeyCode::Char('m'), _) => app.toggle_selected_favorite(),
                    (KeyCode::Char('M'), _) => app.toggle_favorites_first(),

                    // Open the fuzzy finder overlay (jump to any repo across all folders).
                    (KeyCode::Char('P'), _) => app.open_finder(),

                    // Open the folder picker to add a folder/repo to the workspace.
                    (KeyCode::Char('A'), _) => app.open_picker(),

                    // Remove the selected repo's (or folder header's) root from the workspace
                    // (`X` or Delete — Delete isn't reliably delivered by every terminal).
                    (KeyCode::Delete, _) | (KeyCode::Char('X'), _) => app.remove_selected_root(),

                    // Preview scroll (when preview focused)
                    (KeyCode::PageUp, _) if app.focus == Pane::Result => {
                        if let Some(repo_idx) = app.selected_repo_index() {
                            let mut state = app.repos[repo_idx].lock().unwrap();
                            state.auto_scroll = false;
                            state.preview_scroll =
                                state.preview_scroll.saturating_sub(20);
                        }
                    }
                    (KeyCode::PageDown, _) if app.focus == Pane::Result => {
                        if let Some(repo_idx) = app.selected_repo_index() {
                            let total = {
                                let state = app.repos[repo_idx].lock().unwrap();
                                state.log.lines().len()
                            };
                            let mut state = app.repos[repo_idx].lock().unwrap();
                            state.preview_scroll =
                                (state.preview_scroll + 20).min(total.saturating_sub(1));
                        }
                    }
                    (KeyCode::End, _) if app.focus == Pane::Result => {
                        if let Some(repo_idx) = app.selected_repo_index() {
                            let mut state = app.repos[repo_idx].lock().unwrap();
                            state.auto_scroll = true;
                        }
                    }

                    // List navigation: jump and page (when the preview isn't focused).
                    (KeyCode::Home, _) => app.nav_top(),
                    (KeyCode::End, _) => app.nav_bottom(),
                    (KeyCode::PageUp, _) => {
                        let step = (app.list_area.height.saturating_sub(2)) as usize;
                        app.nav_page_up(step);
                    }
                    (KeyCode::PageDown, _) => {
                        let step = (app.list_area.height.saturating_sub(2)) as usize;
                        app.nav_page_down(step);
                    }

                    // Clear log buffer for selected repo
                    (KeyCode::Char('x'), _) => {
                        if let Some(repo_idx) = app.selected_repo_index() {
                            let mut state = app.repos[repo_idx].lock().unwrap();
                            state.log.clear();
                        }
                    }

                    // Toggle the info block above the log/diff (tracks the selection).
                    (KeyCode::Char('i'), _) => {
                        app.info_pinned = !app.info_pinned;
                    }
                    // Toggle the result/log panel (bottom of the preview); hidden, info fills it.
                    (KeyCode::Char('I'), _) => app.toggle_result_panel(),
                    // Toggle the per-repo diff view in the right pane.
                    (KeyCode::Char('d'), _) => {
                        app.toggle_diff_view();
                    }
                    // Open the selected repo's remote in the browser.
                    (KeyCode::Char('o'), _) => {
                        let url = app
                            .selected_repo_index()
                            .and_then(|idx| app.repos[idx].lock().unwrap().remote_url.clone());
                        if let Some(url) = url {
                            drop(app);
                            open_url(&url);
                        }
                    }
                    // Open the documentation website in the browser.
                    (KeyCode::Char('D'), _) => {
                        drop(app);
                        open_url(render::DOCS_URL);
                    }
                    // Copy the selected repo's local path to the clipboard.
                    (KeyCode::Char('y'), _) => {
                        if let Some(idx) = app.selected_repo_index() {
                            let path = app.repos[idx].lock().unwrap().path.display().to_string();
                            app.show_copy_toast(&path);
                            drop(app);
                            copy_to_clipboard(&path);
                        }
                    }
                    // Copy the selected repo's remote URL to the clipboard.
                    (KeyCode::Char('Y'), _) => {
                        let url = app
                            .selected_repo_index()
                            .and_then(|idx| app.repos[idx].lock().unwrap().remote_url.clone());
                        if let Some(url) = url {
                            app.show_copy_toast(&url);
                            drop(app);
                            copy_to_clipboard(&url);
                        }
                    }
                    // Start claude code in the selected repo (suspends the TUI; handled below).
                    (KeyCode::Char('c'), _) => {
                        if let Some(idx) = app.selected_repo_index() {
                            pending_claude = Some(app.repos[idx].lock().unwrap().path.clone());
                        }
                    }
                    // Open lazygit in the selected repo (suspends the TUI like `c`).
                    (KeyCode::Char('l'), _) => {
                        if let Some(idx) = app.selected_repo_index() {
                            pending_lazygit = Some(app.repos[idx].lock().unwrap().path.clone());
                        }
                    }

                    // Enter / double-click: collapse/expand a selected group header, else open
                    // the dedicated repo page for the selected repo.
                    (KeyCode::Enter, _) => {
                        if !app.toggle_selected_header() {
                            app.open_repo_page();
                        }
                    }

                    // Retry selected repo if it has an issue (failed or skipped).
                    (KeyCode::Char('r'), _) => {
                        if let Some(repo_idx) = app.selected_repo_index() {
                            let retryable = {
                                let state = app.repos[repo_idx].lock().unwrap();
                                state.status.is_retryable()
                            };
                            if retryable {
                                drop(app);
                                retry_queue.push(repo_idx);
                            }
                        }
                    }
                    // Retry all repos with an issue (failed or skipped).
                    (KeyCode::Char('R'), _) => {
                        let retryable = app.retryable_repos();
                        drop(app);
                        retry_queue.extend(retryable);
                    }
                    // Refetch selected repo: re-run regardless of status, unless it's in progress.
                    (KeyCode::Char('e'), _) => {
                        if let Some(repo_idx) = app.selected_repo_index() {
                            let refetchable = {
                                let state = app.repos[repo_idx].lock().unwrap();
                                state.status.is_terminal()
                            };
                            if refetchable {
                                drop(app);
                                retry_queue.push(repo_idx);
                            }
                        }
                    }
                    // Refetch all repos not currently in progress.
                    (KeyCode::Char('E'), _) => {
                        let refetchable = app.refetchable_repos();
                        drop(app);
                        retry_queue.extend(refetchable);
                    }
                    // `u`/`U`: refresh local git facts (branch, ahead/behind, dirty, …) for the
                    // selected repo / all repos WITHOUT pulling. Cheap, network-free.
                    (KeyCode::Char('u'), _) => {
                        if let Some(repo_idx) = app.selected_repo_index() {
                            let repo = Arc::clone(&app.repos[repo_idx]);
                            drop(app);
                            tokio::spawn(run_repo_details(repo));
                        }
                    }
                    (KeyCode::Char('U'), _) => {
                        let repos = app.repos.clone();
                        let max_jobs = app.max_jobs;
                        drop(app);
                        tokio::spawn(run_all_details(repos, max_jobs));
                    }

                    _ => {}
                }
            }
            _ => {}
            }
        }

        // Lazily load the repo page (fetch + branches + worktrees) when it's open.
        {
            let mut app = app_state.lock().unwrap();
            if let Some(idx) = app.repo_page {
                let repo = Arc::clone(&app.repos[idx]);
                {
                    let mut state = repo.lock().unwrap();
                    if state.page.is_none() && !state.page_loading {
                        state.page_loading = true;
                        drop(state);
                        // Seed this repo's per-branch overrides from the persisted map so the stats
                        // worker resolves each base correctly on first paint.
                        app.seed_repo_base_overrides(idx);
                        tokio::spawn(run_repo_page(Arc::clone(&repo)));
                    } else if state.page.is_some() {
                        drop(state);
                        // Rows exist now — snap the selection to the current branch (once).
                        app.focus_head_branch_if_pending();
                    }
                }
                // The repo page's info panel shows the PR too — resolve it (cache-aware).
                if let Some(pr_repo) = app.maybe_resolve_pr(idx, now_unix()) {
                    tokio::spawn(run_pull_request(pr_repo, now_unix()));
                }
            }
        }

        // Once a git-backed column is enabled, fetch details for all repos in the background.
        {
            let mut app = app_state.lock().unwrap();
            if app.columns.any_git() && !app.details_pass_spawned {
                app.details_pass_spawned = true;
                let repos = app.repos.clone();
                let max_jobs = app.max_jobs;
                drop(app);
                tokio::spawn(run_all_details(repos, max_jobs));
            }
        }

        // When the PR column is enabled, resolve PRs for all repos in the background — bounded and
        // cache-aware (fresh entries are skipped, so a warm cache never re-hits `gh`). Network, so
        // capped low to stay gentle on the GitHub API.
        {
            let mut app = app_state.lock().unwrap();
            if app.columns.pull_request && !app.pr_pass_spawned {
                app.pr_pass_spawned = true;
                let repos = app.repos.clone();
                let max_jobs = app.max_jobs.min(8);
                let cache = app.pr_cache.clone();
                drop(app);
                tokio::spawn(run_all_prs(repos, max_jobs, now_unix(), cache));
            }
        }

        // Lazily fetch details/diff for the selected repo when those views are open.
        {
            let app = app_state.lock().unwrap();
            // The info block (`i`) needs details, regardless of the log/diff view beneath it.
            if app.info_pinned {
                if let Some(idx) = app.selected_repo_index() {
                    let repo = Arc::clone(&app.repos[idx]);
                    {
                        let mut state = repo.lock().unwrap();
                        // Fetch when never loaded, or re-fetch when a pull marked details stale so
                        // the panel reflects the new HEAD (sha, ahead/behind, last commit).
                        if (state.details.is_none() || state.details_stale) && !state.details_loading
                        {
                            state.details_loading = true;
                            drop(state);
                            tokio::spawn(run_repo_details(Arc::clone(&repo)));
                        }
                    }
                    // Resolve the open PR for the info panel, honoring the 5-min cache TTL (seeds
                    // from a fresh entry; only hits `gh` when stale/missing).
                    if let Some(pr_repo) = app.maybe_resolve_pr(idx, now_unix()) {
                        tokio::spawn(run_pull_request(pr_repo, now_unix()));
                    }
                }
            }
            if app.right_view == RightView::Diff {
                if let Some(idx) = app.selected_repo_index() {
                    let repo = Arc::clone(&app.repos[idx]);
                    let mut state = repo.lock().unwrap();
                    if state.diff.is_none() {
                        state.diff = Some(vec!["(loading…)".to_string()]);
                        drop(state);
                        tokio::spawn(run_repo_diff(repo));
                    }
                }
            }
        }

        tick = tick.wrapping_add(1);
    }
}

fn compute_exit_code(app_state: &Arc<Mutex<AppState>>) -> i32 {
    let app = app_state.lock().unwrap();
    let has_failed = app
        .repos
        .iter()
        .any(|repo| repo.lock().unwrap().status.is_failed());
    if has_failed {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::wheel_step;
    use crossterm::event::KeyModifiers;

    #[test]
    fn wheel_step_scales_with_modifiers() {
        // No modifier → base step.
        assert_eq!(wheel_step(KeyModifiers::NONE, 1, 30), 1);
        assert_eq!(wheel_step(KeyModifiers::NONE, 3, 30), 3);
        // Shift → 5× base.
        assert_eq!(wheel_step(KeyModifiers::SHIFT, 1, 30), 5);
        assert_eq!(wheel_step(KeyModifiers::SHIFT, 3, 30), 15);
        // Ctrl / Alt → a full page.
        assert_eq!(wheel_step(KeyModifiers::CONTROL, 3, 30), 30);
        assert_eq!(wheel_step(KeyModifiers::ALT, 1, 30), 30);
        // Page never collapses to zero.
        assert_eq!(wheel_step(KeyModifiers::CONTROL, 3, 0), 1);
    }
}
