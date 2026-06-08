---
title: CLI flags & env
description: Every pull-all command-line flag, positional argument, and environment variable.
---

```
pull-all [OPTIONS] [DIR]
```

## Positional argument

| Argument | Default | Description |
|----------|---------|-------------|
| `DIR` | current directory | Directory to scan for git repos to pull. |

A directory literally named `go`, `bun`, or `cli` is reachable as `pull-all ./go` —
see [Sibling builds](../siblings/).

## Flags

| Flag | Env | Default | Description |
|------|-----|---------|-------------|
| `-j`, `--jobs <N>` | `PULL_JOBS` | `nproc` | Maximum concurrent pulls. |
| `--timeout <SECS>` | `PULL_TIMEOUT` | `30` | Per-pull timeout in seconds. |
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
| `PULL_CLAUDE_CMD` | Command run by the `c` key (default `cc`, i.e. `claude --dangerously-skip-permissions`). |
| `BROWSER` | Preferred opener for the `o` key (falls back to `wslview`, `xdg-open`, `open`). |

## Examples

```bash
pull-all                              # pull the current directory, TUI
pull-all ~/projects -j 16             # 16 parallel pulls
PULL_JOBS=8 pull-all ~/projects       # same, via env
pull-all --no-tui ~/projects          # plain output for scripts/CI
pull-all --timeout 60 ~/work          # allow slow remotes 60s each
pull-all --profile --profile-out /tmp/pull.prof ~/projects
```
