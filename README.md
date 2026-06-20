# polygit

Interactive polyrepo git dashboard. Discovers every git repo under a directory and shows their status in a two-pane TUI — pulling them in parallel with live per-repo logs, with retry/refetch support, a persisted status cache, and configurable auto-pull. Built in Rust with ratatui.

📖 **Documentation: https://steven-pribilinskiy.github.io/polygit**

## Features

- **Recursive discovery** (default): crawls the directory tree in parallel for git repos, pruning hidden / `node_modules` / `vendor` / `target` / `*.worktrees` dirs and never descending into a found repo — so `polygit ~/projects` (or even `~`) just works. Repos stream in and start pulling as they're found; `--depth N` caps it, `--no-recursive` restores a single-level scan
- **Multiple folders & named workspaces**: pass several folders (`polygit ~/work ~/oss`) — each may itself be a single repo; a no-arg launch scans the cwd. Curate a multi-folder set as a **named workspace**: `polygit -w <name> <dirs…>` defines/opens it, `polygit -w <name>` reopens it, `polygit ws` shows an interactive picker, and `polygit ws ls` lists them (stored in `~/.config/polygit/state.json` under arbitrary names). In the tree view each folder becomes its own top-level node (a forest)
- **Fuzzy finder overlay** (`P`): an fzf-style picker over every repo across all folders — type to fuzzy-filter, `^S` cycles sort (relevance / name / recent / **most-used**), `Enter` jumps the list to that repo. Recent/most-used are backed by the **shared `~/.config/goto-repo/history`** (the same usage file `goto-repo` uses), and each jump is recorded there
- **Folder picker** (`A`): a filesystem browser to add a folder — or a single repo — to the workspace (breadcrumbs, fuzzy search, git-repo badges, bookmarks, current path). The chosen folder is scanned and, when a **named workspace** is active (`-w`), persisted to it; `X`/`Delete` removes the selected repo's folder. (Ad-hoc cwd/CLI-dir sessions add folders live but don't persist them — open under `-w <name>` to keep changes.) Built on the reusable **`tui-pick`** crate (a workspace member other Rust CLIs can depend on)
- **Directory-tree view** (`v t`): render the repos as a collapsible folder tree, with per-folder status rollups; orthogonal to grouping, so you can have flat, grouped, tree, or **tree + groups** (groups subdivide repos inside each folder)
- Parallel pulls with configurable concurrency (default: nproc); the list title shows live concurrency (`⇄ active/cap`)
- Live log streaming per repo in a scrollable **Command log** pane (the right pane, titled `[2] Command log · <repo> · <status>`)
- Status glyphs: queued / running / up-to-date / updated / no-upstream / skipped / throttled / failed — plus an optional **status text column** (`t u`) that spells them out and names the failure kind on failed repos (`not found` / `auth` / `diverged` / `not a repo` / `timeout` / `network` / `lock`)
- Branches with no upstream are a distinct **no-upstream** state (`▽`), not a failure — kept off the Errors page and counted as done. A branch whose tracked remote ref was deleted (PR merged → "no such ref was fetched") is treated the same way, not as a red failure
- **Throttle adaptation**: detects remote rate-limiting (HTTP 429 / "rate limit" / SSH connection throttling) as a distinct **throttled** state (`↯`), shows a warning banner, automatically halves concurrency, and re-queues throttled repos with exponential backoff — restoring full concurrency once the remote is quiet
- Automatic one-shot retry of a failed pull before marking it failed — skipped for permanent errors (repository not found, auth failure, diverged branch) where retrying can't change the result
- Dynamic `Errors (N)` page (after `Result`) listing each failed repo with its error output
- **What the pull delivered**: optional **pulled** (`t p`, `⇣N` commits) and **changed** (`t c`, `±N` files) columns showing what each repo's pull landed this run; the info panel's **Pulled** row spells out the full delta — `oldsha → newsha · N commits · M files (+ins −del) · N new tags · N new branches` — and the panel re-fetches after a pull so its numbers reflect the new HEAD
- **Optional auto-pull + status cache**: launches are useful instantly — the list is seeded from a persisted per-repo status cache (last-known status shown **dim with an age**, e.g. `up-to-date 2d`, until pulled). Auto-pull-on-launch is configurable in Settings (master on/off · a max-repos limit `50/100/250/∞` · suppress in tree view); by default it pulls small flat sets and **skips** large sets (>100) and the tree view. When auto-pull is suppressed, `E` pulls everything / `e` pulls the selected repo on demand
- Retry repos with an issue (`r` / `R`) and refetch any repo from scratch (`e` / `E`) — a refetch re-pulls **and** refreshes every cached fact (branch/dirty/stash counts, ahead/behind, worktrees). With a **folder or group header selected**, `r`/`e` act on just that section (the folder's whole subtree, recursively, or the group's members)
- **Open PR detection** (via the `gh` CLI): when a repo's current branch has an open pull request, the info panel shows a clickable **Pull Request** row (`#N title`, with a "checked … ago" age) below the branch, and the optional **pr** column (`t r`) shows a clickable `#N` per repo. Results are **cached 5 minutes** per repo+branch (in `pr-cache.json`, each entry timestamped) so the column fills instantly on relaunch without re-hitting the network; the selected repo refreshes on focus, and a pull re-checks. Resolved in the background with bounded concurrency when the column is on; silently absent when `gh` isn't installed or there's no PR
- Footer command hints **dim and go inert when not applicable** (never hidden) — e.g. info/diff/page/claude/lazygit/open/copy when no repo is selected, the fold + group/tree hints when there's nothing to fold. While a **modal is open** the background recedes — all pane borders + titles dim — and the whole footer goes inert except `, settings` · `? help` · `q` (which stay clickable **and hover-highlight** from inside any modal), with `q` reading **`close`** (it closes the modal instead of quitting). Only **one modal is ever open at a time** — clicking `, settings` while help is up closes help and opens settings (no stacking)
- Worktree discovery (`.worktrees/*/.git`)
- Sort the list (`s` leader, or click a column header) by name / branch / status / ahead-behind / dirty / last-commit / worktrees / branches / stashes / pulled / changed — re-pick or re-click flips `▲`/`▼` (persisted; the list is always sorted, Name asc by default, ties broken alphabetically by name). The sort menu lists only the columns currently visible
- **Fuzzy**-filter repos by name (`/`) — subsequence matching ranked by relevance, with matched characters underlined (powered by the `nucleo` matcher) — or **by status/attribute** by prepending `@` (e.g. `@failed`, `@dirty`, `@ahead`, `@behind`, `@updated`); the prompt hints at it. While typing, the selection follows the **first match** so you can `Enter` to jump straight to it; `Esc` clears the filter and restores the repo you were on. Also a quick status filter via the `f` leader (updated / up-to-date / skipped / failed / issues)
- **Favorites**: mark a repo with `m` (or click the star in the optional **favorite column** `t f`); `M` pins a **★ Favorites** section to the top of the list (footer `M ★favs` toggle, shown once anything is favorited). Favorites persist in `~/.config/polygit/state.json`
- **Repo groups** (`z`): named list sections from `~/.config/polygit/groups.json` — membership by `*`-pattern, static list, shell command, or a fetched JSON document; sort/filter apply within each group, big groups collapse (`Enter`/`Space`/click on the header), dynamic memberships are cached and refreshed with `Z`
- Clickable 2-row column header with the active sort indicator; an always-on dirty marker (`•`) with the count (`•N`) when the dirty column is toggled. Count columns render a **dim zero** in the Unicode set (not a blank) — but in **emoji** mode a zero cell is hidden (a colorful emoji beside `0` is clutter) — and a column every repo lacks (no worktrees/stashes, ≤1 branch) auto-hides — its `t`-menu chip goes dim and inert
- Lazygit-style panels: rounded borders, a bright border on the focused panel, `Tab`/`Shift-Tab` to cycle focus across the visible panels (`[1]` list · `[2]` info · `[3]` result · `[4]` repo page) or `1`-`4` to jump, and a draggable divider with a grip
- Open [lazygit](https://github.com/jesseduffield/lazygit) on the selected repo with `l`
- Diff modal with a clickable file list over the selected file's diff (stash, uncommitted, vs base branch, or **a branch's changes vs its base**); `Tab` switches focus between the file list and diff, with a footer that adapts to the focused pane; **status-filter chips** (`f` / click) with count badges when a change set has >10 files across ≥2 statuses; `Shift`/`Alt`+`PgUp`/`PgDn` page the file list; "no changes" shows a toast instead of an empty modal
- Draggable scrollbars everywhere (preview, diff panels, help, repo page), highlighted while dragged
- Tabbed, **context-aware** help modal (`?`): **Hotkeys** (for the current view) · **CLI** · **Legend** (every glyph, both icon sets) · **About** · **Design** (Theme/Background/Contrast/Selection radios + a live palette swatch showcase + a **component showcase** — buttons, list rows, and radios drawn in every interaction state: normal / hover / selected / active / focused / disabled, under both hover and selection effects, plus a **[preview confirm dialog]** button that opens the shared confirm dialog live; the Design tab toggles between a **flat** scroll and a **tabbed** layout with vertical section tabs via `v`), switched with `Tab`/click (last tab remembered). The Hotkeys tab's `[K ⌨ keyboard]` (or `K`) pops a **responsive interactive keyboard viewer** — a full-blown bordered keyboard when there's room (compact strip when not), nav cluster on the right, every bound key highlighted; press/click any key (physical `Shift`/`Ctrl` too) to list what it does
- Settings modal (`,`) with a top **search box** (`/` focuses it — type to filter rows across every tab into a flat list with matched chars highlighted; `Esc` clears), five sections: **Lists** (grouping · tree view · hide folder-header lines), **Theming** (Unicode ⇄ emoji icons · **hide zeros** · **theme** auto/dark/light · independent **background** normal/soft/**terminal** · **contrast** normal/soft · **list selection** blue/subtle · **button hover** inverted/subtle), **Sync** (auto-pull on launch · auto-pull limit `50/100/250/∞` · auto-pull in tree view), **Interaction** (**hover effects** · changed-row flash/highlight), and **Layout** (panel padding · borders · splitter · repo-page tabs · repo page restored/maximized · auto branch-check) — all persisted; rows and radio chips are mouse-clickable, and every modal's hint footer lives on the bottom border (clickable, hover-highlighted). The `auto` theme **re-detects** dark/light at runtime, so an OS light↔dark switch re-themes live (no restart)
- **Hover effects** (off by default): when on, the actionable element under the mouse cursor (status-bar commands, footer hints, table-sort headers, column chips, info links/copy buttons, scrollbars, the splitter, list rows) gets a subtle highlight. A status-bar command's key and its label highlight together; sortable column headers highlight too; and dwelling ~1s shows a tooltip — on a status-bar command (what it does), a column title (`wt`/`st`/`pr`/… meaning), or a group/folder header's right-corner count (its breakdown). A column-title tooltip for an optional column also carries a clickable red **`[x]`** that hides that column outright (the tooltip stays alive while the cursor moves onto it, so the `[x]` is reachable). Tooltips are placed by a floating-ui-style engine: a column-title tooltip **drops below the header** (anchored bottom-left), footer tooltips sit above, and each **flips** to the opposite side and **shifts** along its edge to stay on-screen. It enables all-motion mouse tracking, which takes over the terminal's own text selection / URL hover — so it's opt-in
- Web-like mouse support everywhere: full status-bar hints + active `⟪sort⟫`/`[filter]` tags clickable, every modal gets an `[x]` and closes on outside click, repo page has an `[esc back]` button
- **New-build reload notice**: detects a newer binary installed over the running one and offers a one-key `Ctrl-R` reload / `Ctrl-X` dismiss (also clickable; exec-restart in the same terminal)
- Non-TUI fallback (same output as bash reference) when not on a TTY or with `--no-tui`
- Exit codes: 0 (all ok), 1 (any failed), 2 (user quit mid-run), 130 (Ctrl-C)

## Installing

polygit is a single binary for **Linux and macOS** (on Windows, use WSL).

```bash
# Install script — grabs the prebuilt binary for your platform
curl -fsSL https://steven-pribilinskiy.github.io/polygit/install.sh | bash

# Or with cargo (no clone needed)
cargo install --git https://github.com/steven-pribilinskiy/polygit

# Or from source
make build              # release build → ~/bin/polygit
```

See the [installation docs](https://steven-pribilinskiy.github.io/polygit/start/installation/) for details.

## Running

```bash
# TUI mode (auto-detected when stderr is a TTY)
polygit [DIR...]

# Recursive by default — scan a whole tree of projects
polygit ~/projects

# Multiple folders (each may itself be a single repo)
polygit ~/work ~/oss ~/some-repo

# Named workspaces: define/open, reopen, pick interactively, list
polygit -w work ~/work ~/oss   # define & open workspace "work"
polygit -w work                # reopen it
polygit ws                     # interactive picker
polygit ws ls                  # list saved workspaces

# Plain streaming output (matches bash reference for a flat dir; lists nested repos too)
polygit --no-tui [DIR]

# Custom concurrency
polygit -j 8 [DIR]
PULL_JOBS=8 polygit [DIR]

# Cap scan depth (1 = immediate subdirs only — the legacy single-level scan)
polygit --depth 3 [DIR]
polygit --no-recursive [DIR]

# Custom timeout per pull (default: 30s)
polygit --timeout 60 [DIR]

# Skip worktree discovery
polygit --no-worktrees [DIR]
```

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Next repo |
| `k` / `↑` | Previous repo |
| `g` | Jump to top |
| `G` | Jump to bottom (Result item) |
| `Space` | Toggle the Result summary in the preview without moving selection (any navigation clears it); on a folder/group header: collapse/expand |
| `v` `g` | Toggle the **grouped list view** (groups from `~/.config/polygit/groups.json`; persisted) |
| `v` `t` | Toggle the **directory-tree view** (folders from recursive discovery; persisted) |
| `z` `…` | **Fold leader** (vim-style): `za` toggle · `zo`/`zc` open/close · `zO` expand subtree · `zM` collapse all · `zR` expand all (on the selected folder/group) |
| `-` / `+` `=` / `*` | Collapse all / expand all / expand the selected subtree |
| `Z` | Refresh dynamic group memberships (`command`/`url` sources) now, bypassing the cache TTL |
| `←` / `→` | Tree-style fold nav: `←` collapses the selected header or jumps to its enclosing folder/group; `→` expands a collapsed header |
| `[` / `]` | Narrow / widen the left pane — clickable in the status bar (or drag the divider — its grip fills solid and brightens while dragging) |
| `Tab` / `Shift-Tab` | Cycle focus across the visible panels: `[1]` list · `[2]` info · `[3]` result · `[4]` repo page (the focused panel gets a bright rounded border; hidden panels are skipped) |
| `1` / `2` / `3` / `4` | Focus the list / info / result / repo-page panel directly |
| `PgUp` / `PgDn` | Scroll the result/log panel (when panel `[3]` is focused) |
| `End` | Resume auto-scroll in preview |
| `Enter` / double-click | Open the dedicated repo page for the selected repo (on the repo list); on a folder/collapsible group header: collapse/expand |
| `r` | Retry selected repo if it has an issue — or, on a folder/group header, every repo it covers |
| `R` | Retry all repos with an issue (failed or skipped) |
| `e` | Refetch selected repo (re-pull regardless of status) — or, on a folder/group header, every repo it covers |
| `E` | Refetch all repos that aren't currently in progress |
| `u` | Refresh the selected repo's local git facts — branch, ahead/behind, dirty, stash (no pull) |
| `U` | Refresh all repos' local git facts (no pull) |
| `i` | Toggle the info panel — an additive block above the log/diff (status, branch, pull request, pulled delta, ahead/behind, remote, last commit, worktrees, changes, path) |
| `I` | Toggle the result/log panel (the bottom of the preview). Info + result are independent panels split by a **draggable boundary**; hide the result panel and the info panel fills the pane (reads like the repo list). Clickable `I log` footer hint; persisted |
| `d` | Toggle the per-repo diff view (working-tree changes, or the last pull's diff) |
| `t` | Column-toggle leader: press `t` then `u`/`a`/`d`/`l`/`w`/`b`/`s`/`p`/`c` to show/hide a column (mode stays active until `Esc`) |
| `s` | Sort leader: press `s` then `n`/`c`/`s`/`a`/`d`/`l`/`w`/`b`/`k` to sort by name / branch / status / ahead-behind / dirty / last-commit / worktrees / branches / stashes — re-pick flips `▲`/`▼` (or click a column header); the list is always sorted (Name asc by default) |
| `f` | Status-filter leader: press `f` then `a`/`u`/`c`/`s`/`f`/`i` to filter the list by all / updated / up-to-date / skipped / failed / issues (applies on top of `/`) |
| `o` | Open the selected repo's remote in the browser |
| `y` | Copy the selected repo's **absolute path** to the clipboard (every copy confirms with a toast previewing the copied text) |
| `Y` | Copy the selected repo's **remote (origin) URL** to the clipboard |
| `c` | Start claude code in the selected repo (suspends the TUI, returns on exit) |
| `l` | Open **lazygit** in the selected repo (suspends the TUI, returns on exit) |
| `x` | Clear **this repo's log buffer** (empties the streamed pull output) |
| `D` | Open the [documentation website](https://steven-pribilinskiy.github.io/polygit/) in the browser |
| `,` | Open the settings modal (panel padding, grouping, tree view, icon style, theme, background, contrast, list selection, button hover) |
| `?` | Open the help modal (docs/GitHub/notes links, all keys, flags & env) |
| `/` | Filter repos by name |
| `Esc` | Clear filter (or quit when no filter) |
| `q` | Quit |
| `Ctrl-C` | Quit (exit 130) |

**Retry vs refetch:** retry only re-runs repos that need it (failed/skipped); refetch re-runs any repo even if it was already up to date. In the status bar, `r`/`R` dim when no repo has an issue, and `e`/`E` dim when there's nothing eligible (the selected repo is still in progress).

The repo list, the log/diff preview, the help modal, and the repo page all show a scrollbar when their content overflows. **Clickable commands:** the action hints in the status bar (and the `t` column menu) are mouse-clickable — clicking one runs the same command as the key.

### Repo page (`Enter` / double-click)

Opens the repo page (**panel `[4]`**) for the selected repo that runs `git fetch` and lists every local branch (with HEAD marker, fresh ahead/behind vs upstream, upstream name, last-commit date, subject), every worktree (branch + path), and every stash. By default the page is **restored** — a docked panel across the bottom, so the list above stays live and selecting another repo retargets the page (master-detail); press `m` (or the `▢` title-bar button, left of `[esc back]`) to **maximize** it full-screen, `▣`/`m` to restore. The window state is sticky (persisted) and also set in Settings → Layout → **Repo page**. A header row labels the branch columns, which are toggled with the page-local `t` menu (then `b`/`y`/`a`/`m`/`d`/`c`/`u`/`f`/`g`/`r`/`s` for ahead-behind / dirty / added / modified / deleted / total / upstream / base / age / **pr** / subject — clickable chips, persisted). The **pr** column shows the current branch's open pull request (a clickable `#N`, via `gh`) on the HEAD row, blank elsewhere. **Stash rows** flow through the same change-stat columns (added/modified/deleted/total), loaded lazily from `git stash show`. The **added/modified/deleted** counts are each branch's changes vs the merge-base with its **base branch** — the auto-detected fork parent (the branch it most directly diverged from, weighing both local heads and remote-tracking branches, so a branch cut from a non-`main` integration branch resolves correctly), or a per-branch **override** you set — loaded in the background (cells show `…` until ready); a column every branch leaves empty auto-hides and its chip goes dim. The **base** column shows that resolved base per branch (blue when auto-detected, magenta with a trailing `*` when overridden). Count cells show a dim zero rather than a blank. The bottom **info panel** (`i`, persisted) details the selected row: branch, upstream, base branch + merge-base sha, ahead/behind, change stats, and the tip commit (sha · author · date · subject). Sections are prefixed with type icons; worktrees/stashes sections only appear when non-empty. The selection starts on the current (HEAD) branch. Navigate rows with `j`/`k`/`g`/`G`/`Home`/`End` (or the wheel / click); `Enter` (or double-click) opens the diff modal on a stash, a dirty row, or **a branch (its changes vs the base branch)**; `Shift+Enter` checks out the selected branch (clean, non-current); `p`/`P` fast-forward; `d` performs the row-appropriate action (delete branch / drop stash / remove worktree / discard) — the footer hint is dynamic; `b` (or clicking the **base** cell) opens the **base-branch picker** to override which branch this branch's stats diff against — pick *auto-detect* to clear the override; the choice is persisted per repo+branch; `c` starts claude code; `l` opens lazygit; `o` opens the branch on the remote (e.g. GitHub) in the browser; `y` opens a copy menu (absolute path / branch name / both); `m` maximizes/restores the page; `?` shows the page's hotkeys; `Esc`/`q` returns. An action result (e.g. "Dropped stash@{0}") shows in a banner at the bottom.

`Enter` or a double-click opens a 90%-of-screen **diff modal**, two bordered sub-panels: a scrollable **file-list panel** (top, ≤40% height) over the **selected file's diff** (bottom). The footer adapts to the focused pane. `Tab` switches focus between the panels (the focused one gets a bright border); `j`/`k`/`g`/`G` then drive that panel. Pick a file with `↑↓`/`j`/`k` or by clicking it; its diff loads beneath. `PgUp`/`PgDn` page the diff; `Shift`/`Alt`+`PgUp`/`PgDn` page the file list; `Shift`/`Alt`+wheel scrolls the file list. When a change set has more than 10 files across at least two statuses, a **status-filter chip row** appears (`[ all N ] [ M … ] [ A … ] …` with count badges) — click a chip or press `f` to cycle, and the list groups by status. The diff-panel title shows the full path, left-truncating only when it doesn't fit. For a dirty row, `t` toggles the file set between *uncommitted* (vs HEAD) and *vs base branch*; a stash lists its files; a clean branch shows its changes vs the base branch. `d` discards/removes/drops (with confirm); `Esc` closes. When there's nothing to show, a "no changes" toast appears instead of an empty modal.

### Columns (`t` leader)

The list always shows the status glyph + name + branch + a dirty marker (an amber `•` for any repo with uncommitted changes — amber, not red, since it's a "modified" state, not an error). Click the **`[cols ▾]`** / **`[sort ▾]`** chips on the list header for a dropdown menu (a mouse-friendly companion to the `t`/`s` leaders below). Press `t` then a column key to toggle extra columns: `u` status text (a short label per state — `queued`/`running`/`up-to-date`/`updated`/`no upstream`/`dirty`/`throttled` — with the specific failure kind on failed repos: `not found`, `auth`, `diverged`, `not a repo`, `timeout`, `network`, `lock`, and `ref gone` for a deleted upstream ref), `a` ahead/behind, `d` adds the dirty **count** (`•N`) to the always-on marker, `l` last-commit age, `w` worktree count (`⑃N`, cyan), `b` feature-branch count (`⑂N`, green — local branches excluding `main`/`dev`), `s` stash count (`≡N`), `p` commits the last pull landed (`⇣N`, green), `c` files the last pull changed (`±N`, cyan), `r` the open **pull request** for the current branch (a clickable `#N`, via `gh`). Count columns render a **dim zero** rather than a blank in the Unicode set, so the column shape stays recognizable (in **emoji** mode a zero cell is hidden — a colorful emoji next to `0` reads as clutter). A column every repo leaves empty (no worktrees, no stashes, or ≤1 branch everywhere) auto-hides once its data has loaded, and its `t`-menu chip goes dim and inert; the **pulled**/**changed** columns stay hidden until a pull lands a delta, then remain for the session (so a retry doesn't flicker them in and out). The git-derived columns fetch per-repo details in the background the first time one is enabled (cells show `…` until ready); `w` is free from worktree discovery, and pulled/changed come straight from each pull. Enabled columns persist across runs.

### Info panel (`i`)

`i` toggles an info block above the right pane's content (the pull log or the diff) for the selected repo: status (with how long the pull took), branch, the **Pulled** delta when the repo updated this run (`oldsha → newsha` on one line, then `N commits · M files (+ins −del)`, then best-effort `N new tags · N new branches`), ahead/behind, remote, last commit (hash · subject · author · relative date), worktrees, uncommitted/stash counts, and the local path. The block is additive — the log/diff stays beneath it — and tracks the selection as you move. The extra git facts are fetched lazily for the selected repo only, and re-fetched after a pull so the panel reflects the new HEAD.

The panel is interactive (it's a web app in a terminal):

- **Bold field labels**; rows that would carry nothing are hidden — no `↑0 ↓0`, no all-zero Changes line, no empty Worktrees.
- **Clickable links** (when the remote is a browsable https host): the **branch** opens `…/tree/<branch>`, the **commit hash** opens `…/commit/<sha>`, and **Remote** opens the repo — all in your browser.
- **Truncated values expand on click.** The path is truncated from the *left* (keeping the filename tail); a long commit subject from the right. Click the underlined value to expand it — the full text wraps starting at the value column, never under the label. Click again to collapse.
- **Copy buttons**: a `⧉` next to **Path** copies the absolute path; a `⧉` on the log pane's top border copies the whole pull log.

`c` starts claude code (`cc`, i.e. `claude --dangerously-skip-permissions`, in the repo dir; override with `PULL_CLAUDE_CMD`).

### Settings modal (`,`)

`,` opens a small settings modal (from the list or the repo page), organized into **Lists** (grouping, tree view, hide folder-header lines), **Theming** (icons, hide zeros, theme, background, contrast, list selection, button hover), **Sync** (auto-pull policy), **Interaction** (hover effects, changed-row flash/highlight), **Layout** (panel padding, borders, splitter, repo-page tabs, repo page restored/maximized, auto branch-check), and **Tooltips** (a master "All tooltips" toggle plus per-area switches — footer commands, column headers, group counts, settings rows, help links) sections. By default it's a **tabbed** (JetBrains-style) layout — the sections are vertical tabs on the left; switch tabs with `←`/`→` or `Tab`, move between rows with `j`/`k` (or `↑`/`↓`), toggle/cycle the selected setting with `Space`/`Enter`, and close with `Esc`/`q`/`,`/`[x]`/a click outside. Press `v` (hint in the bottom border) to cycle the layout **tabbed → accordion → flat**. The **accordion** layout stacks every section under a collapsible header — `j`/`k` (or `↑`/`↓`) move through both the headers and the rows of expanded sections, `Space`/`Enter` (or `←`/`→`) on a focused header folds/unfolds it (a focused header has no active child row — it's the header itself that's selected), clicking a header focuses **and** toggles it, the top `[- collapse all]` / `[+ expand all]` button folds them all, and the content scrolls when it overflows the modal. The folded set persists. The **flat** layout stacks every section expanded in one scroll. Press **`R`** (or the `R reset` footer chip) to **reset every setting to its default** — a confirmation modal lists each setting that will change with its `current → default` value (favorites, workspaces, and caches are left untouched). With the mouse, click a tab to switch, a radio chip to set **that** value (clicking the already-active chip does nothing), or the row label to cycle to the next value (3-radio settings cycle left→right and wrap). With hover effects on, dwelling ~1s over a setting shows a tooltip explaining it (e.g. the **unicode** icon option notes that Unicode glyphs can be colorized per type, unlike emoji). All settings persist across runs (in `~/.config/polygit/state.json`):

- **Panel padding** — adds a 1-cell inner padding inside every bordered panel and modal.
- **Icons** — switches the status / column / marker glyphs **everywhere** (list, columns, repo page, Result/Errors pages, log markers) between the default Unicode set (`◌ ✓ ◇ ✗ ⑂ ≡ •`) and an emoji set (`✅ ✨ 🚫 ❌ 🌿 📦 📝`). Columns stay aligned in either mode — only single-codepoint, reliably-2-cell emoji are used (no variation-selector glyphs), and the tight ahead/behind column keeps compact `↑↓` arrows.
- **Hide zeros** — hides zero-count column cells (a dim `0` becomes blank) so only repos with real ahead/behind/dirty/worktree/branch/stash counts show a number. Emoji mode always hides zeros, so this row renders **force-selected and inert** while emoji icons are active.
- **Theme** — `dark` / `light` paint a full explicit palette (background, text, and every accent color) so the app looks identical regardless of the terminal's own color scheme. `auto` detects whether the terminal background is dark or light at startup and applies the matching palette — via an OSC 11 query of the terminal, falling back to `COLORFGBG`, the Windows light/dark setting under WSL (covers terminals that follow the system theme but don't answer OSC 11, e.g. Tabby), and the macOS appearance setting.
- **Background** and **Contrast** — two independent `normal` / `soft` axes. **Background** softens the surface tones (background, selection, shadow); **Contrast** narrows the text/background distance and desaturates the accent + semantic colors. They compose, so you can soften the surface while keeping vivid text, or vice versa. (Pre-split state files, which had only `contrast`, load with both axes set from the old value.)
- **Changed-row flash / highlight** — how a repo row signals that a pull/refresh changed one of its cells. **flash** pulses the changed cells; **highlight** holds them steady for the attention window. Independent toggles (turn both off to rely on the status text column instead).
- **Auto branch-check** — `off` / `auto`. When `auto`, polygit periodically refreshes each repo's local git facts (branch, ahead/behind, dirty, stash — **no pull**) so the list stays current without manual `u`/`U`. The interval scales with the repo count (~`repos/10` seconds, clamped 1–60s; 10 repos → ~1s, 100 → ~10s) and pauses while any pull is in flight.
- **Borders** — draw the rounded borders around the two main panes. Off reclaims the border rows/columns (the pane titles float on the top row and content fills the freed space).
- **Splitter** — draw the draggable grip on the divider between the panes. Off hides the grip (the panes sit flush); you can still resize by dragging the boundary.
- **Repo page** — the window state the repo page (`[4]`) opens in: **restored** (a docked bottom panel — the list/info/result panels stay visible above it, master-detail) or **maximized** (full-screen). Toggle live with `m` or the `▢`/`▣` title-bar button (left of `[esc back]`); this setting picks the default and the choice is sticky (persisted). When restored, **drag the panel's top edge** to resize it (the height is persisted) — a second splitter alongside the panes' divider.
- **Repo page tabs** — `off` keeps the repo page as one scrolling list; `auto` splits it into **Branches / Worktrees / Stashes / Commits tabs** when at least two sections have content (switch with `Tab`/`Shift+Tab` or click a tab chip). The **Commits** tab lists recent commits on the current branch (sha · date · author · subject), read-only.
- **List selection** — how the selected row is highlighted, applied everywhere a row can be selected (the repo list, the repo page, the diff file list). `blue` is a solid blue bar with white text (high contrast, but it overrides each column's own color); `subtle` is a soft blue tint that keeps the per-column colors (status hue, branch accent, …) readable — better for these color-coded tables. Hovering the selected row deepens the tint in either mode, so it stays distinct from a plain hover.
- **Button hover** (Theming, below List selection) — how *buttons* (footer/modal hint chips, tabs, radio chips, close buttons, keyboard keys) look under the cursor. `subtle` (default) is the soft tint; `inverted` is reverse-video — fg/bg swap, so the hovered button reads as a solid chip. Independent of List selection.
- **Grouping** — render the list as named group sections (same as `v g`). Shows a hint when no `groups.json` exists.
- **Tree view** — render the repos as a collapsible directory tree (same as `v t`). Inert when every repo is at the scan root (a flat directory).
- **Hide folder lines** — drop the dim dash-fill leader lines connecting a group / folder header name to its right-corner count, for a cleaner header row.

### Repo groups (`v g`)

`v g` renders the list as named **group sections** defined in `~/.config/polygit/groups.json` (hand-edited, optional — never written by the app). When groups are configured, a clickable `vg groups` hint appears in the status bar (its label brightens while the grouped view is active). Each group header shows per-status counts and the member total; repos inside a group keep the global sort and filters; repos matching no group land in a dim `ungrouped` section at the bottom. Groups with more members than `collapse_threshold` (default: 5) get a collapsible header — selectable, with `▾`/`▸`, toggled by `Enter`/`Space`/click; smaller groups get static headers navigation skips. The grouping toggle and collapsed groups persist across runs.

Each group has a `name` and exactly one membership source:

```json
{
  "collapse_threshold": 5,
  "cache_ttl_minutes": 1440,
  "groups": [
    { "name": "frontend", "pattern": "mfe-*" },
    { "name": "tooling", "repos": ["polygit", "dotfiles"] },
    { "name": "team", "command": "curl -fsSL https://example.com/repos.txt" },
    { "name": "platform", "url": "https://example.com/remote-entries.json",
      "extract": { "pointer": "/entries", "kind": "keys" } }
  ]
}
```

`pattern` is a case-insensitive `*`-wildcard on repo names — **or, when it contains a `/`, on the repo's path relative to the scan root** (e.g. `work/*`); `repos` is a static list; `command` runs a shell command whose stdout lines are repo names; `url` fetches a JSON document and extracts names per `extract` (a JSON pointer + `keys`/`values`). Dynamic (`command`/`url`) sources resolve in the background — never blocking startup — and are cached in `~/.config/polygit/groups-cache.json` for `cache_ttl_minutes` (default: daily). `Z` forces a refresh; a failed resolve keeps the cached membership and marks the header with `⚠`. Selecting a group header shows a group summary (source, counts, cache age, errors) in the preview pane. Full reference: [Repo groups guide](https://steven-pribilinskiy.github.io/polygit/guides/groups/).

### Directory tree (`v t`)

Recursive discovery is the default: `polygit` crawls the target directory in parallel for git repos (pruning hidden dirs, `node_modules`/`vendor`/`target`/`dist`/… and `*.worktrees`, and never descending into a found repo), streaming each repo in and starting its pull as soon as it's found. `--depth N` caps the descent (`--depth 1` / `--no-recursive` is the legacy single-level scan). In flat and grouped views, each repo shows its path relative to the scan root (e.g. `personal/polygit`).

`v t` renders that result as a **collapsible directory tree**: folders become headers (`▾`/`▸`) with their subtree's status rollup and repo count, repos nest beneath by basename. Tree and grouping are **two independent toggles**, so four views are reachable:

- **flat** — every repo in one list (default)
- **grouped** (`v g`) — repos in `groups.json` sections, regardless of folder
- **tree** (`v t`) — the folder hierarchy
- **tree + groups** (both on) — groups subdivide the repos *inside each folder*; a group collapses independently per folder

Fold the tree with the mouse (click a folder header), `←`/`→` (collapse/expand or jump to the parent), `Enter`/`Space` (toggle the selected header), the direct keys `-` (collapse all) / `+` (expand all) / `*` (expand the selected subtree), or the vim-style `z` chord (`za`/`zo`/`zc`/`zO`/`zM`/`zR`). The tree toggle and collapsed-folder set persist across runs. Full reference: [Tree view guide](https://steven-pribilinskiy.github.io/polygit/guides/tree-view/).

### Sorting (`s` leader / column headers)

The list is always sorted — **Name ascending** is the default. Press `s` then a column key (`n` name, `c` branch, `s` status, `a` ahead/behind, `d` dirty, `l` last-commit, `w` worktrees, `b` branches, `k` stashes, `p` pulled commits, `g` changed files) — or open the **`[sort ▾]`** header dropdown, or click a column header (including the **branch** header). The sort menu only lists columns currently visible on screen. Re-picking the same column (or re-clicking the header) flips the direction; the header shows `▲`/`▼` on the active column and the footer shows a clickable `⟪column ▲⟫` tag. **Ties break by name** (A→Z) — repos sharing a column value (e.g. the same branch) list alphabetically within that group, and the name tiebreak stays ascending even under a descending column. The order persists across runs.

### Help modal (`?`)

`?` opens an in-app reference with five tabs — **Hotkeys** (contextual, with short sections laid out side by side), **CLI** (an **interactive command builder** — every flag is a checkbox row (value flags check on once you type a value, which **auto-applies** live; related flags like `--no-recursive`/`--profile-out` indent under their parent), the help comments align in a column you can hide with `h`, the assembled `polygit …` command updates live and is **clickable to copy** (also `y` / **[ copy ]**); plus exit codes), **Legend** (every glyph in both icon sets with its meaning), **Design** (the Theme/Background/Contrast/Selection radios — click a chip to set that value, click the row label to cycle; the autodetected theme is underlined when `auto` is active — above a live swatch showcase of the palette's semantic colors that re-themes as you change them, then a **component showcase** rendering the reusable `tui-pick` button / list-row / radio primitives in every interaction state (normal · hover · selected · active · focused · disabled) under both hover and selection effects, with the active choice marked, plus a clickable **[preview confirm dialog]** button that opens the shared confirm dialog (the same one every destructive action uses — its yes/no are hover-highlighted footer-style chips); icon set lives in Legend. The Design tab itself has a **flat ⇄ tabbed** layout toggle (`v`, like the settings modal): **flat** stacks every section in one scroll; **tabbed** shows the sections (Theming · Palette · Buttons · List rows · Radios · Dialogs) as clickable **vertical tabs** with `[`/`]` to move between them), and **About** — switched with `Tab` (the last tab is remembered across opens, except About). The About tab groups its links (**polygit** docs + GitHub, **lazygit**, and a collapsible **Notes** group you expand with a click) and shows **titles only** — hover a link to see its URL in the bottom-left corner (browser-style), or dwell ~1s for a tooltip. The links are clickable (open in your browser via `$BROWSER`/`wslview`/`xdg-open`). Scroll with `j`/`k`, `g`/`G`, `PgUp`/`PgDn`, or the wheel; **maximize/restore** the modal to ~90% of the viewport with `m` (or the `[m maximize]` button); close with `?`/`Esc`/`q`/`[esc]`/a click outside.

Press `/` on **any tab** to **search** it (type to filter, `Esc` clears) — the Hotkeys tab matches lazygit-style against the **key column AND the description** (prepend `@` to match the keys column specifically), the other tabs are a plain text filter over their content. `[K ⌨ keyboard]` (or pressing `K`) opens an interactive keyboard viewer (the same one on the [docs site](https://steven-pribilinskiy.github.io/polygit/guides/keybindings/), built from the same keymap): an OS-aware on-screen keyboard with the bound keys highlighted, and the nav/arrow cluster (home/end/pgup/pgdn + arrows) to its right. It's responsive — on a large terminal the keys grow into a full-blown keyboard of tall bordered keys; on a small one it falls back to a compact strip with the cluster below. Press or click any key — including physical `Shift`/`Ctrl` (on terminals with the Kitty keyboard protocol) — to highlight it and list everything it does in a scrollable panel below, whose keys column auto-sizes to the longest combo shown; `Esc` closes the viewer.

### Mouse

Click a repo row to select it (a click also **focuses** whichever panel it lands
in, like the `1`-`4`/`Tab` keys). Over the **left pane** the plain wheel **scrolls
the list view** (web-app style — the selection stays put and may scroll out of
view), and **`Alt`+wheel moves the selection** (like `↑`/`↓`); keyboard nav always
scrolls just enough to keep the selection on screen. Over the **right pane** the
wheel scrolls the preview. Click or drag the preview scrollbar to jump/scroll, and
drag the divider between the panes to resize. On the preview/repo-page, hold a
modifier while scrolling to go faster: **Shift** jumps 5× the normal step, **Ctrl**
scrolls a full page. While the TUI is running it captures the mouse, so native
terminal text-selection is suspended until you quit (same tradeoff as lazygit/htop).

Everything actionable is clickable like a web page:

- **Status-bar hints** — the whole hint ("s sort", "f by-status", "/ filter", …), not just the key. While a leader menu is armed (e.g. `f`/`s`/`t`) the rest of the footer dims and goes inert, and the armed leader's own trigger gets a highlight pill so it's clear which menu is open. The active tags sit next to their hints and are clickable too: `⟪name ▲⟫` flips the sort direction, `[needle]` clears the name filter, `{failed}` resets the status filter. In "[ ] resize", `[` and `]` nudge the split directly. The right side shows the version, a clickable `built … ago` tag (opens the Build info modal), and clickable `, settings · ? help · q quit`.
- **Modals** (settings, copy menu, confirm, diff, help) — every modal has an `[x]` close button on its top border and closes/cancels when you click anywhere outside it. Clicks inside a modal never fall through to the view behind.
- **Settings** — click a row label to select it, or click a radio chip (`● dark`, `○ off`, …) to set that exact value.
- **Confirm dialogs** — `[y/enter] yes` and `[n] no` are clickable.
- **Copy menu** — click an option to copy it immediately.
- **Repo page** — a clickable `[esc back]` button on the top border returns to the list.

### New-build reload

While running, polygit watches its own binary on disk. When a newer build is installed (e.g. `make install`'s atomic rename), a persistent notice appears in the top-right (inset with the panel-padding setting, with a glint sweeping its border): `↺ new build installed · [^R reload] [^X]`. It rides on top of every screen — the repo list, the repo page, and any open modal — so it's never hidden. **`Ctrl-R`** (or clicking `[^R reload]`) restores the terminal and `exec`s the new binary with the same arguments — the fresh process re-scans and re-pulls (instant when everything is already up to date). **`Ctrl-X`** (or clicking `[^X]`) dismisses the notice; it re-arms if the binary changes again. Because the notice floats over every view, both keys work from anywhere (including inside a modal).

Clicking the **`built … ago`** tag in the status bar opens a **Build info** modal: the running version, when it was built, the **binary size** + watched path, the **settings file location** + how many files live in the config dir, whether a newer build is waiting, and a **scrollable, syntax-highlighted preview of `state.json`** (`j`/`k`/`PgUp`/`PgDn`/wheel scroll). `r` (a clickable hint in the bottom border, alongside `esc close`) exec-restarts into the latest build; any other key or click closes it.

## Testing

```bash
make test
```

## Benchmark

```bash
make bench
```

## Architecture

- `src/main.rs` — CLI entry point, TUI setup, event loop
- `src/app.rs` — Application state types (`AppState`, `RepoState`, `LogBuffer`) + retry/refetch eligibility helpers
- `src/git.rs` — Git operations + the recursive repo walker (`spawn_repo_walker`, `should_descend`) and pull-output classification (`classify_pull_output`, incl. throttle detection) + unit tests
- `src/worker.rs` — Async pull workers + streaming discovery (`run_discovery`) and the throttle governor (`run_governor`), bounded by the shared `ThrottleControl` semaphore
- `src/groups.rs` — Repo-grouping config, membership resolution, and cache
- `src/theme.rs` — Color palettes + terminal-background detection
- `src/render.rs` — Ratatui rendering (list pane, preview pane, status bar, ANSI color support)
- `src/plain.rs` — Non-TUI streaming output (byte-compatible with bash reference)
