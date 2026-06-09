# pull-all

Interactive multi-repo git pull dashboard. Pulls every git repo in a directory in parallel with live per-repo logs, retry/refetch support, and a two-pane TUI layout. This is the canonical Rust implementation; it also fronts the Go, Bun, and bash alternatives via subcommands.

📖 **Documentation: https://steven-pribilinskiy.github.io/pull-all**

## Features

- Parallel pulls with configurable concurrency (default: nproc)
- Live log streaming per repo in a scrollable preview pane
- Status glyphs: queued / running / up-to-date / updated / no-upstream / skipped / failed
- Branches with no upstream are a distinct **no-upstream** state (`⊝`), not a failure — kept off the Errors page and counted as done
- Automatic one-shot retry of a failed pull before marking it failed
- Dynamic `Errors (N)` page (after `Result`) listing each failed repo with its error output
- Retry repos with an issue (`r` / `R`) and refetch any repo from scratch (`e` / `E`) — a refetch re-pulls **and** refreshes every cached fact (branch/dirty/stash counts, ahead/behind, worktrees)
- Action hints dim when they'd be a no-op
- Worktree discovery (`.worktrees/*/.git`)
- Sort the list (`s` leader, or click a column header) by name / status / ahead-behind / dirty / last-commit / worktrees / branches / stashes — re-pick or re-click flips `▲`/`▼` (persisted)
- Filter repos by name (`/`) or by status (`f` leader: updated / up-to-date / skipped / failed / issues)
- Clickable 2-row column header with the active sort indicator; an always-on dirty marker (`•`) with the count (`•N`) when the dirty column is toggled
- Lazygit-style panes: rounded borders, a bright border on the focused pane (`Tab` / `1` / `2`), and a draggable divider with a grip
- Open [lazygit](https://github.com/jesseduffield/lazygit) on the selected repo with `l`
- Diff modal with a clickable file list over the selected file's diff (stash, uncommitted, vs base branch, or **a branch's changes vs its base**); `Tab` switches focus between the file list and diff; "no changes" shows a toast instead of an empty modal
- Draggable scrollbars everywhere (preview, diff panels, help, repo page), highlighted while dragged
- Tabbed, **context-aware** help modal (`?`): **Hotkeys** (for the current view) · **CLI & Flags** · **About**, switched with `Tab`/click (last tab remembered)
- Settings modal (`,`): panel padding, Unicode ⇄ emoji icons, and a **theme** (auto / dark / light) — all persisted
- Non-TUI fallback (same output as bash reference) when not on a TTY or with `--no-tui`
- Exit codes: 0 (all ok), 1 (any failed), 2 (user quit mid-run), 130 (Ctrl-C)

## Building

```bash
# Requires Rust stable (cargo)
make build              # binary at: bin/pull-all
make install            # also copies to ~/bin/pull-all
```

## Running

```bash
# TUI mode (auto-detected when stderr is a TTY)
pull-all [DIR]

# Plain streaming output (matches bash reference)
pull-all --no-tui [DIR]

# Custom concurrency
pull-all -j 8 [DIR]
PULL_JOBS=8 pull-all [DIR]

# Custom timeout per pull (default: 30s)
pull-all --timeout 60 [DIR]

# Skip worktree discovery
pull-all --no-worktrees [DIR]
```

## Sibling implementations

`pull-all` forwards to the other builds when the first argument is `go`, `bun`, or `cli`; all remaining arguments are passed through verbatim:

```bash
pull-all go  [args]   # Go / bubbletea build (pull-all-tui-go)
pull-all bun [args]   # Bun / ink build, JIT (pull-all-tui-bun-jit)
pull-all cli [args]   # bash streaming version (pull-all-repos)
```

A directory literally named `go`/`bun`/`cli` is still reachable as `pull-all ./go`.

The backends live in `pull-all-siblings/` next to the `pull-all` binary (e.g. `~/bin/pull-all-siblings/`), deliberately **off `$PATH`** so they aren't top-level commands — they're reachable only through `pull-all go|bun|cli`. The dispatcher resolves them relative to its own location and falls back to `$PATH` if that directory is absent.

The `cli` backend (`pull-all-repos`, the original parallel-pull bash script that `src/plain.rs` was ported from) is tracked in this repo under [`pull-all-siblings/`](pull-all-siblings/) and deployed by `make install`. The `go` and `bun` backends are built from their own source trees.

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Next repo |
| `k` / `↑` | Previous repo |
| `g` | Jump to top |
| `G` | Jump to bottom (Result item) |
| `Space` | Toggle the Result summary in the preview without moving selection (any navigation clears it) |
| `[` / `]` | Narrow / widen the left pane (or drag the divider — it shows a grip, and highlights while dragging) |
| `Tab` | Toggle focus: list `[1]` ↔ preview `[2]` (the active pane gets a bright rounded border) |
| `1` / `2` | Focus the list / preview pane directly |
| `PgUp` / `PgDn` | Scroll preview (when focused) |
| `End` | Resume auto-scroll in preview |
| `Enter` / double-click | Open the dedicated repo page for the selected repo (on the repo list) |
| `r` | Retry selected repo if it has an issue (failed or skipped) |
| `R` | Retry all repos with an issue (failed or skipped) |
| `e` | Refetch selected repo (re-pull regardless of status, unless it's in progress) |
| `E` | Refetch all repos that aren't currently in progress |
| `i` | Toggle the info panel — an additive block above the log/diff (status, branch, ahead/behind, remote, last commit, worktrees, changes, path) |
| `d` | Toggle the per-repo diff view (working-tree changes, or the last pull's diff) |
| `t` | Column-toggle leader: press `t` then `a`/`d`/`l`/`w`/`b`/`s` to show/hide a column (mode stays active until `Esc`) |
| `s` | Sort leader: press `s` then `n`/`s`/`a`/`d`/`l`/`w`/`b`/`k`/`o` to sort by name / status / ahead-behind / dirty / last-commit / worktrees / branches / stashes / none — re-pick flips `▲`/`▼` (or click a column header) |
| `f` | Status-filter leader: press `f` then `a`/`u`/`c`/`s`/`f`/`i` to filter the list by all / updated / up-to-date / skipped / failed / issues (applies on top of `/`) |
| `o` | Open the selected repo's remote in the browser |
| `y` | Copy the selected repo's **absolute path** to the clipboard |
| `Y` | Copy the selected repo's **remote (origin) URL** to the clipboard |
| `c` | Start claude code in the selected repo (suspends the TUI, returns on exit) |
| `l` | Open **lazygit** in the selected repo (suspends the TUI, returns on exit) |
| `x` | Clear **this repo's log buffer** (empties the streamed pull output) |
| `D` | Open the [documentation website](https://steven-pribilinskiy.github.io/pull-all/) in the browser |
| `,` | Open the settings modal (panel padding, icon style) |
| `?` | Open the help modal (docs/GitHub/notes links, all keys, flags & env) |
| `/` | Filter repos by name |
| `Esc` | Clear filter (or quit when no filter) |
| `q` | Quit |
| `Ctrl-C` | Quit (exit 130) |

**Retry vs refetch:** retry only re-runs repos that need it (failed/skipped); refetch re-runs any repo even if it was already up to date. In the status bar, `r`/`R` dim when no repo has an issue, and `e`/`E` dim when there's nothing eligible (the selected repo is still in progress).

The repo list, the log/diff preview, the help modal, and the repo page all show a scrollbar when their content overflows. **Clickable commands:** the action hints in the status bar (and the `t` column menu) are mouse-clickable — clicking one runs the same command as the key.

### Repo page (`Enter` / double-click)

Opens a full-screen page for the selected repo that runs `git fetch` and lists every local branch (with HEAD marker, fresh ahead/behind vs upstream, upstream name, last-commit date, subject), every worktree (branch + path), and every stash. Sections are prefixed with type icons and the worktrees/stashes sections only appear when non-empty; a `•N` column shows uncommitted-change counts. The selection starts on the current (HEAD) branch. Navigate rows with `j`/`k`/`g`/`G`/`Home`/`End` (or the wheel / click); `Enter` (or double-click) opens the diff modal on a stash, a dirty row, or **a branch (its changes vs the base branch)**; `Shift+Enter` checks out the selected branch (clean, non-current); `p`/`P` fast-forward; `d` performs the row-appropriate action (delete branch / drop stash / remove worktree / discard) — the footer hint is dynamic; `c` starts claude code; `l` opens lazygit; `o` opens the branch on the remote (e.g. GitHub) in the browser; `y` opens a copy menu (absolute path / branch name / both); `?` shows the page's hotkeys; `Esc`/`q` returns. An action result (e.g. "Dropped stash@{0}") shows in a banner at the bottom.

`Enter` or a double-click opens a 90%-of-screen **diff modal**, two bordered sub-panels: a scrollable **file-list panel** (top, ≤40% height) over the **selected file's diff** (bottom). `Tab` switches focus between the panels (the focused one gets a bright border); `j`/`k`/`g`/`G` then drive that panel. Pick a file with `↑↓`/`j`/`k` or by clicking it; its diff loads beneath. `PgUp`/`PgDn` page the diff; `Shift`/`Alt`+wheel scrolls the file list. For a dirty row, `t` toggles the file set between *uncommitted* (vs HEAD) and *vs base branch*; a stash lists its files; a clean branch shows its changes vs the base branch. `d` discards/removes/drops (with confirm); `Esc` closes. When there's nothing to show, a "no changes" toast appears instead of an empty modal.

### Columns (`t` leader)

The list always shows the status glyph + name + branch + a dirty marker (`•` for any repo with uncommitted changes). Press `t` then a column key to toggle extra columns: `a` ahead/behind, `d` adds the dirty **count** (`•N`) to the always-on marker, `l` last-commit age, `w` worktree count (`⑂N`, cyan), `b` feature-branch count (`⑂N`, green — local branches excluding `main`/`dev`), `s` stash count (`≡N`). The git-derived columns fetch per-repo details in the background the first time one is enabled (cells show `…` until ready); `w` is free from worktree discovery. Enabled columns persist across runs.

### Info panel (`i`)

`i` toggles an info block above the right pane's content (the pull log or the diff) for the selected repo: status + elapsed, branch, ahead/behind vs upstream, remote, last commit (hash · subject · author · relative date), worktrees, uncommitted/stash counts, and the local path. The block is additive — the log/diff stays beneath it — and tracks the selection as you move. The extra git facts are fetched lazily for the selected repo only. `c` starts claude code (`cc`, i.e. `claude --dangerously-skip-permissions`, in the repo dir; override with `PULL_CLAUDE_CMD`).

### Settings modal (`,`)

`,` opens a small settings modal (from the list or the repo page). Move between rows with `j`/`k` (or `↑`/`↓`), toggle/cycle the selected setting with `Space`/`Enter`, and close with `Esc`/`q`/`,`. All settings persist across runs (in `~/.config/pull-all/state.json`):

- **Panel padding** — adds a 1-cell inner padding inside every bordered panel and modal.
- **Icons** — switches the status / column / marker glyphs **everywhere** (list, columns, repo page, Result/Errors pages, log markers) between the default Unicode set (`◌ ✓ ⊘ ✗ ⑂ ≡ •`) and an emoji set (`✅ ✨ 🚫 ❌ 🌿 📦 📝`). Columns stay aligned in either mode — only single-codepoint, reliably-2-cell emoji are used (no variation-selector glyphs), and the tight ahead/behind column keeps compact `↑↓` arrows.
- **Theme** — `auto` inherits the terminal's own colors; `dark` / `light` paint an explicit base background + foreground so the app looks consistent regardless of terminal (truecolor terminals get exact colors).

### Sorting (`s` leader / column headers)

The list can be sorted by any column. Press `s` then a column key (`n` name, `s` status, `a` ahead/behind, `d` dirty, `l` last-commit, `w` worktrees, `b` branches, `k` stashes, `o` none) — or click a column header. Re-picking the same column (or re-clicking the header) flips the direction; the header shows `▲`/`▼` on the active column and the footer shows a `⟪column ▲⟫` tag. The order persists across runs.

### Help modal (`?`)

`?` opens an in-app reference with two tabs — **Hotkeys** and **CLI & Flags** — switched with `Tab` (the last tab is remembered across opens). It links to this repo on GitHub and the design notes on `notes.lvh.me`, lists the `go`/`bun`/`cli` subcommands, every flag and environment variable, the hotkeys grouped by purpose, and exit codes. The links are clickable (open in your browser via `$BROWSER`/`wslview`/`xdg-open`). Scroll with `j`/`k`, `g`/`G`, `PgUp`/`PgDn`, or the wheel; close with `?`/`Esc`/`q`.

### Mouse

Click a repo row to select it, scroll the wheel over the left pane to move the
selection or over the right pane to scroll the preview, click or drag the preview
scrollbar to jump/scroll, and drag the divider between the panes to resize. While
the TUI is running it captures the mouse, so native terminal text-selection is
suspended until you quit (same tradeoff as lazygit/htop).

## Testing

```bash
make test
```

## Benchmark

```bash
make bench
```

## Architecture

- `src/main.rs` — CLI entry point, sibling dispatch, TUI setup, event loop
- `src/app.rs` — Application state types (`AppState`, `RepoState`, `LogBuffer`) + retry/refetch eligibility helpers
- `src/git.rs` — Git operations (`discover_repos`, `get_branch`, `is_dirty`, `diff_stat`, `classify_pull_output`) + unit tests
- `src/worker.rs` — Async pull workers with semaphore concurrency control
- `src/render.rs` — Ratatui rendering (list pane, preview pane, status bar, ANSI color support)
- `src/plain.rs` — Non-TUI streaming output (byte-compatible with bash reference)
