---
title: CLI flags & env
description: Every polygit command-line flag, positional argument, and environment variable.
---

```
polygit [OPTIONS] [DIR]
```

## Positional argument

| Argument | Default | Description |
|----------|---------|-------------|
| `DIR` | current directory | Directory to scan **recursively** for git repos to pull. |

The scan is recursive by default — it crawls the tree in parallel, pruning hidden dirs,
`node_modules`/`vendor`/`target`/`dist`/… and `*.worktrees`, and never descending into a
found repo. Use `--depth 1` (or `--no-recursive`) for the legacy single-level scan.

## Subcommands

| Command | Aliases | Description |
|---------|---------|-------------|
| `list` | `ls` | List every repo with its current branch. |
| `status` | | Show uncommitted changes (`git status --short`) for each dirty repo; `All repos clean.` when none. |
| `dirty` | | Print just the names of repos with uncommitted changes (grep-style filter). |
| `branches` | | Show each repo's branch and ahead/behind vs its upstream (`↑N ↓N`, `✓` in sync, `no upstream`). |
| `sizes` | | Show disk usage per repo, largest first, plus a total. |
| `ws` | `workspace`, `workspaces` | Interactive workspace picker; `ws ls` lists saved workspaces. |
| `update` | `upgrade` | Self-update to the latest published release. |

The five report commands are headless: they print to stdout and exit (colors only when
stdout is a TTY, so piped output stays plain). Each accepts its own scan args — `[DIR...]`,
`-w <name>`, `--depth <N>`, `--no-recursive` — which must come **after** the subcommand
(`polygit list -w work`, not `polygit -w work list`).

```bash
polygit list ~/projects              # every repo + current branch
polygit status                       # what's uncommitted, per repo
polygit dirty | head                 # pipe-friendly dirty-repo names
polygit branches -w work             # ahead/behind across a saved workspace
polygit sizes --no-recursive         # disk usage, immediate subdirs only
```

## Flags

| Flag | Env | Default | Description |
|------|-----|---------|-------------|
| `-j`, `--jobs <N>` | `PULL_JOBS` | `nproc` | Maximum concurrent pulls. Overrides the persisted **Settings → Workers → Parallel pulls** cap at launch (shown there as an exact pick). Reduced automatically when a remote throttles, restored when it's quiet; changeable live in Settings. |
| `--depth <N>` | | `16` | Maximum directory depth to scan (`1` = immediate subdirs only). |
| `--no-recursive` | | off | Scan only the immediate subdirectories (same as `--depth 1`). |
| `--timeout <SECS>` | `PULL_TIMEOUT` | `10` | Per-pull timeout in seconds. |
| `--no-tui` | | off | Force plain streaming output (no TUI). |
| `--no-worktrees` | | off | Skip `.worktrees/*/.git` discovery. |
| `--profile` | | off | Emit a per-repo timing report (slowest first) after the run. |
| `--profile-out <FILE>` | | stderr | Write the profile report to a file instead of stderr. |
| `--version` | | | Print the version and exit. |
| `--help` | | | Print help and exit. |

## Environment variables

| Variable | Description |
|----------|-------------|
| `PULL_JOBS` | Same as `-j`/`--jobs`. |
| `PULL_TIMEOUT` | Same as `--timeout`. |
| `PULL_CLAUDE_CMD` | Overrides the command run by the `c` key verbatim. Unset, `c` runs the agent chosen in Settings → Agent (claude / codex / gemini), plus its skip-permissions flag when that toggle is on. |
| `BROWSER` | Preferred opener for the `o` key (falls back to `wslview`/`xdg-open`/`open` on Unix, `cmd /C start` on Windows). |

## Examples

```bash
polygit                              # pull the current directory tree, TUI
polygit ~/projects -j 16             # recursive scan, 16 parallel pulls
polygit ~ --depth 4                  # crawl home, capped at 4 levels deep
polygit --no-recursive ~/projects    # legacy single-level scan
PULL_JOBS=8 polygit ~/projects       # concurrency via env
polygit --no-tui ~/projects          # plain output for scripts/CI
polygit --timeout 60 ~/work          # allow slow remotes 60s each
polygit --profile --profile-out /tmp/pull.prof ~/projects
```

## Build a command interactively

You don't have to memorize the flags: open the help modal (`?`) and switch to the
**CLI & Flags** tab for an interactive builder. Each flag is a row — `↑`/`↓` to move,
`Space`/`Enter` to toggle a boolean flag or start editing a value (type it, `Enter` to
set), or click a row directly. The constructed `polygit …` command updates live below the
flag list; press `y` or click **[ copy ]** to copy it to the clipboard.
