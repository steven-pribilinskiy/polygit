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
| `coverage` | `missing`, `cov` | Per GitHub owner, which of its repos aren't cloned locally. |
| `select` | `sel` | Resolve a [selector expression](#selector-expressions) to the repos it picks. |
| `plan` | | Preview the directory layout a selector + template produce. Touches nothing. |
| `clone` | | Clone the repos a selector picks, into that layout. |
| `orgs` | `owners` | Your account, orgs and enterprises, with how much of each is cloned. |
| `ws` | `workspace`, `workspaces` | Interactive workspace picker; `ws ls` lists saved workspaces. |
| `update` | `upgrade` | Self-update to the latest published release. |

Every command above is headless: it prints to stdout and exits (colors only when stdout is a TTY,
so piped output stays plain). Each accepts its own scan args — `[DIR...]`, `-w <name>`,
`--depth <N>`, `--no-recursive` — which must come **after** the subcommand
(`polygit list -w work`, not `polygit -w work list`).

```bash
polygit list ~/projects              # every repo + current branch
polygit status                       # what's uncommitted, per repo
polygit dirty | head                 # pipe-friendly dirty-repo names
polygit branches -w work             # ahead/behind across a saved workspace
polygit sizes --no-recursive         # disk usage, immediate subdirs only
polygit coverage ~/projects          # which repos of each owner you're missing
```

## Selector expressions

`select` and `plan` take an expression over the repos of every owner found under the scan roots
(plus any named with `--owner`). An empty expression selects everything.

| Term | Matches |
|------|---------|
| `foo` | name contains `foo` |
| `tf-*` | name matches the `*` glob (same matcher as a `groups.json` `pattern`) |
| `re:^tf-` | name matches the regex |
| `prefix:tf` | the name's leading hyphen-tokens are `tf` |
| `suffix:service` | the name's last token is `service` — plurals fold, so `-services` matches too |
| `token:billing` | any hyphen-token of the name is `billing` |
| `topic:x` · `lang:rust` · `owner:acme` | GitHub topic, primary language, owner |
| `is:cloned` · `is:missing` · `is:archived` · `is:fork` · `is:private` | boolean state |
| `list:a,b,c` | an explicit set, by `name` or `owner/name` |

Compose with `AND`, `OR`, `NOT` (or `&`, `|`, `!`) and parentheses; adjacent terms are an implicit
AND; a leading `-` negates one term. `--with-siblings` then widens the result to every repo sharing
a project stem with it — so selecting one service pulls in its infrastructure and deploy repos.

```bash
polygit select 'tf-* NOT is:archived' ~/projects
polygit select 'topic:cli AND is:missing' --owner acme
polygit select 'billing' --with-siblings          # the service, its IaC, its manifests
```

Both commands print how much signal each axis actually carries for the owners in scope — topic
coverage, prefix families, language spread — because a topic filter is a strong primary axis in a
well-tagged org and close to useless in one where the topics are machine-generated markers.

## Layout templates

`plan` renders each selected repo to a destination path built from a template. A placeholder that
resolves to nothing **drops its whole path segment**, so repos that belong to no cluster sit flat
rather than each getting a folder of their own.

| Placeholder | Resolves to |
|-------------|-------------|
| `{repo}` | the repo name (required — a template without it is rejected) |
| `{owner}` | the GitHub owner |
| `{project}` | the project stem, when the repo belongs to a multi-repo cluster |
| `{group}` | the name-prefix family, when that family has more than one member |
| `{language}` | the primary language |
| `{topic:<t>}` | `<t>`, when the repo carries that topic |

```bash
polygit plan '' ~/projects --layout '{project}/{repo}'
polygit plan 'tf-*' ~/projects --layout '{group}/{repo}' -o /tmp/preview --json
```

`polygit clone` runs the same resolution and then acts on the **clone** rows only — a repo already
present is left alone, wherever it sits. Clones run concurrently (`-j`, default `nproc`) through the
same cap as pulls, so a throttling remote slows both together. Full history by default; `--blobless`
defers file contents (`--filter=blob:none`) which is much faster at org scale and, unlike
`--depth-limit`, truncates nothing. `--max-size 2GB` skips large repos and says which. `--dry-run`
prints the destinations and exits.

```bash
polygit clone 'topic:cli AND is:missing' --owner acme -o ~/projects/acme
polygit clone '' --owner acme --layout '{project}/{repo}' --blobless -y
```

Each row comes out as **clone** (not present locally), **keep** (already exactly there), **move**
(present somewhere else) or **skip**. A skip is usually a collision: two owners with the same repo
name both wanting one destination, which `plan` refuses to resolve silently — add `{owner}` to the
template and it goes away.

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
| `--perf` | | off | Collect frame/input timings from launch and print the report on exit. `Ctrl+T` toggles the live overlay at any time; this flag only preloads collection so the first frames are covered, and measures the terminal's own round-trip once at startup. |
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
