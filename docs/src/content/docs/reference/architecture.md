---
title: Architecture
description: How the pull-all Rust crate is organized.
---

`pull-all` is a small Rust crate built on [ratatui](https://ratatui.rs) (TUI), [crossterm](https://github.com/crossterm-rs/crossterm)
(terminal/input), and [tokio](https://tokio.rs) (async pulls).

## Modules

| File | Responsibility |
|------|----------------|
| `src/main.rs` | CLI entry point, sibling dispatch, TUI setup, and the event loop. |
| `src/app.rs` | Application state types (`AppState`, `RepoState`, `LogBuffer`, page/diff/confirm models) and retry/refetch eligibility helpers. |
| `src/git.rs` | Git operations (`discover_repos`, `get_branch`, `is_dirty`, `diff_stat`, branch/worktree/stash mutations, `discard_changes`) plus unit tests. |
| `src/worker.rs` | Async workers with semaphore-bounded concurrency — pulls, page loads, diffs, and the branch/worktree/stash/discard mutations. |
| `src/render.rs` | Ratatui rendering — list pane, preview pane, status bar, repo page, diff modal, confirm/settings/help modals, and ANSI color support. |
| `src/plain.rs` | Non-TUI streaming output, byte-compatible with the bash reference. |
| `src/persist.rs` | UI preferences saved to `~/.config/pull-all/state.json` (columns, sort, icon style, padding, help tab, splitter). |
| `src/profile.rs` | The optional `--profile` per-repo timing report. |

## How a pull flows

1. `discover_repos` scans the target directory (and `.worktrees/*/.git`).
2. `worker` spawns a bounded set of async pulls, each streaming output into its repo's
   `LogBuffer`.
3. `render` redraws the TUI each tick from the shared `AppState`.
4. Key and mouse events flow through the event loop in `main.rs`, which mutates state and
   spawns mutation workers (checkout, fast-forward, delete, drop, remove, discard).

## Input enhancements

`main.rs` pushes the Kitty keyboard-protocol enhancement flags when the terminal supports
them, so modified keys like `Shift`+`Enter` are reported distinctly. The flags are popped
on teardown, on panic, and while a suspended external session (a `c`-launched claude or an
`l`-launched [lazygit](https://github.com/jesseduffield/lazygit)) has the terminal.

## Geometry capture & hit-testing

`render.rs` writes the exact `Rect`s it drew back onto `AppState` each frame (the repo-rows
area, the column-header cells, scrollbar tracks, diff-modal panels, the divider column). Mouse
handlers hit-test against those captured rects rather than recomputing them from borders, so
clicks stay correct regardless of panel padding or the column header.
