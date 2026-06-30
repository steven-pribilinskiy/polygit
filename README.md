# polygit

Interactive polyrepo git dashboard. Discovers every git repo under a directory and shows their status in a two-pane TUI — pulling them in parallel with live per-repo logs, with retry/refetch support, a persisted status cache, and configurable auto-pull. Built in Rust with ratatui.

📖 **Documentation: https://steven-pribilinskiy.github.io/polygit**

## Features

- **Recursive discovery** (default): crawls the directory tree in parallel for git repos, pruning hidden / `node_modules` / `vendor` / `target` / `*.worktrees` dirs and never descending into a found repo — so `polygit ~/projects` (or even `~`) just works. Repos stream in and start pulling as they're found; `--depth N` caps it, `--no-recursive` restores a single-level scan
- **Multiple folders & named workspaces**: pass several folders (`polygit ~/work ~/oss`) — each may itself be a single repo; a no-arg launch scans the cwd. Curate a multi-folder set as a **named workspace**: `polygit -w <name> <dirs…>` defines/opens it, `polygit -w <name>` reopens it, `polygit ws` shows an interactive picker, and `polygit ws ls` lists them (stored in `~/.config/polygit/state.json` under arbitrary names). In the tree view each folder becomes its own top-level node (a forest)
- **Fuzzy finder overlay** (`Ctrl+P`): an fzf-style picker over every repo across all folders — type to fuzzy-filter, `^S` cycles sort (relevance / name / recent / **most-used**), `Enter` jumps the list to that repo. Recent/most-used are backed by the **shared `~/.config/goto-repo/history`** (the same usage file `goto-repo` uses), and each jump is recorded there
- **Folder picker** (`A`): a filesystem browser to add a folder — or a single repo — to the workspace (breadcrumbs, fuzzy search, git-repo badges, bookmarks, current path). The chosen folder is scanned and, when a **named workspace** is active (`-w`), persisted to it; `X`/`Delete` removes the selected repo's folder. (Ad-hoc cwd/CLI-dir sessions add folders live but don't persist them — open under `-w <name>` to keep changes.) Built on the reusable **`tui-pick`** crate (a workspace member other Rust CLIs can depend on)
- **Directory-tree view** (`v t`): render the repos as a collapsible folder tree, with per-folder status rollups; orthogonal to grouping, so you can have flat, grouped, tree, or **tree + groups** (groups subdivide repos inside each folder)
- Parallel pulls with configurable concurrency (default: nproc); the list title shows live concurrency (`⇄ active/cap`)
- Live log streaming per repo in a scrollable **Command log** pane (the right pane, titled `[2] Command log · <repo> · <status>`)
- Status glyphs: queued / running / up-to-date / updated / no-upstream / skipped / throttled / failed — plus an optional **status text column** (`t u`) that spells them out and names the failure kind on failed repos (`not found` / `auth` / `diverged` / `not a repo` / `timeout` / `network` / `lock`)
- Branches with no upstream are a distinct **no-upstream** state (`▽`), not a failure — kept off the Errors page and counted as done. A branch whose tracked remote ref was deleted (PR merged → "no such ref was fetched") is treated the same way, not as a red failure
- **Throttle adaptation**: detects remote rate-limiting (HTTP 429 / "rate limit" / SSH connection throttling) as a distinct **throttled** state (`↯`), shows a warning banner, automatically halves concurrency, and re-queues throttled repos with exponential backoff — restoring full concurrency once the remote is quiet
- Automatic one-shot retry of a failed pull before marking it failed — skipped for permanent errors (repository not found, auth failure, diverged branch) where retrying can't change the result
- Dynamic `Errors (N)` page (after `Result`) listing each failed repo with its error output
- **What the pull delivered**: optional **pulled** (`t p`, `⇣N` commits) and **changed** (`t c`, `±N` files) columns showing what each repo's pull landed this run; the info panel's **Pulled** row spells out the full delta — `oldsha → newsha · N commits · M files (+ins −del)`, then `N new tag(s): <names>` (truncated, click to expand) and `N new branches` — and the panel re-fetches after a pull so its numbers reflect the new HEAD
- **Optional auto-pull + status cache**: launches are useful instantly — the list is seeded from a persisted per-repo status cache (last-known status shown **dim with an age**, e.g. `up-to-date 2d`, until pulled). Auto-pull-on-launch is configurable in Settings (master on/off · a max-repos limit `50/100/250/∞` · suppress in tree view); by default it pulls small flat sets and **skips** large sets (>100) and the tree view. When auto-pull is suppressed, `E` pulls everything / `e` pulls the selected repo on demand
- Retry repos with an issue (`r` / `R`) and refetch any repo from scratch (`e` / `E`) — a refetch re-pulls **and** refreshes every cached fact (branch/dirty/stash counts, ahead/behind, worktrees). With a **folder or group header selected**, `r`/`e` act on just that section (the folder's whole subtree, recursively, or the group's members)
- **PR detection** (via the `gh` CLI): when a repo's current branch has a pull request, the info panel shows a clickable **Pull Request** row (`#N title`, with a colored lifecycle badge — green `open` / magenta `merged` / gray `closed` — and a "checked … ago" age) below the branch, and the optional **pr** column (`t r`) shows a clickable `#N` per repo, colored by that same state. Clicking the PR (info-panel value, repo-page `#N`, or list cell) opens the **[PR viewer modal](#pr-viewer)**; the info-panel row also has a trailing **↗** button that opens the PR on GitHub instead. The **info panel (and repo page) always show the PR when available**, in any state. The **Settings → Pull requests → Merged PRs** toggle gates only the **dense list column**: off (default) the column shows only **open** PRs, on it also shows merged & closed (a merged PR is exactly why a branch's upstream goes "ref gone"). Detection always finds every state — the toggle just gates the column, so flipping it is instant. Results are **cached 5 minutes** per repo+branch (in `pr-cache.json`, each entry timestamped) so the column fills instantly on relaunch without re-hitting the network; the selected repo refreshes on focus, and a pull re-checks. Resolved in the background with bounded concurrency when the column is on; silently absent when `gh` isn't installed or there's no PR. The **Branch** field links to the branch on the remote **only when it's actually there** — a no-upstream / "ref gone" branch renders as plain text, not a link that 404s
- Footer command hints **dim and go inert when not applicable** (never hidden) — e.g. info/diff/page/claude/lazygit/open/copy when no repo is selected, the fold + group/tree hints when there's nothing to fold. While a **modal is open** the background recedes — all pane borders + titles dim — and the whole footer goes inert except `, settings` · `? help` · `q` (which stay clickable **and hover-highlight** from inside any modal), with `q` reading **`close`** (it closes the modal instead of quitting). Only **one modal is ever open at a time** — clicking `, settings` while help is up closes help and opens settings (no stacking)
- Worktree discovery (`.worktrees/*/.git`)
- Sort the list (`s` opens the sort dropdown, or click a column header) by name / branch / status / ahead-behind / dirty / last-commit / worktrees / branches / stashes / pulled / changed — re-pick or re-click flips `▲`/`▼` (persisted; the list is always sorted, Name asc by default, ties broken alphabetically by name). The active sort + direction rides on the `s sort ⟪…⟫ ▾` trigger
- **Fuzzy**-filter repos by name (`/`) — subsequence matching ranked by relevance, with matched characters underlined (powered by the `nucleo` matcher) — or **by status/attribute** by prepending `@` (e.g. `@failed`, `@dirty`, `@ahead`, `@behind`, `@updated`); the prompt hints at it. While typing, the selection follows the **first match** so you can `Enter` to jump straight to it; `Esc` clears the filter and restores the repo you were on. Also a quick status filter via the `f` dropdown (all / updated / up-to-date / skipped / failed / issues)
- **Favorites**: mark a repo with `b` (or click the star in the optional **favorite column** `t f`); `B` pins a **★ Favorites** section to the top of the list (footer `B ★favs` toggle, shown once anything is favorited). Favorites persist in `~/.config/polygit/state.json`
- **Repo groups** (`z`): named list sections from `~/.config/polygit/groups.json` — membership by `*`-pattern, static list, shell command, or a fetched JSON document; sort/filter apply within each group, big groups collapse (`Enter`/`Space`/click on the header), dynamic memberships are cached and refreshed with `Z`
- Clickable 2-row column header with the active sort indicator; an always-on dirty marker (`•`) with the count (`•N`) when the dirty column is toggled. Count columns render a **dim zero** in the Unicode set (not a blank) — but in **emoji** mode a zero cell is hidden (a colorful emoji beside `0` is clutter) — and a column every repo lacks (no worktrees/stashes, ≤1 branch) auto-hides — its columns-dropdown row goes dim and inert
- Lazygit-style panels: rounded borders, a bright border on the focused panel, `Tab`/`Shift-Tab` to cycle focus across the visible panels (`[1]` list · `[2]` info · `[3]` result · `[4]` repo page) or `1`-`4` to jump, and a draggable divider with a grip
- **Every panel is maximizeable** — `m` (or the `m▢`/`m▣` button on each pane's top border) maximizes/restores the focused pane to full-screen; `1`-`4` swap which pane is maximized. Only the repo page's maximize is sticky (persisted); list/info/result maximize is session-only
- **Pane splitter modes** (Settings → Layout → **Pane splitter**): **on hover** (default) keeps the panes flush and shows a thin grip only under the cursor at a splitter hotspot; **dedicated** reserves a visible 1-cell lane between panes with a persistent grip
- Open [lazygit](https://github.com/jesseduffield/lazygit) on the selected repo with `l`
- **File explorer** (`Ctrl-E`, or the `Explore files…` kebab item): a two-pane browser rooted at the selected repo — a directory **list pane** beside a **syntax-highlighted preview pane** (powered by `two-face`/`syntect`, high-contrast themes that track the app's dark/light), split by a **draggable divider** (`[`/`]` or drag, highlights on hover).
  - **Sortable columns** — `name` always shown; **size / permissions / modified / created / kind** via the `t` column-picker dropdown (off by default, persisted). Click a header (or the `s` sort dropdown) to sort; the active header shows `▲`/`▼`. Folders show **recursive sizes** (computed in the background); size is **colored by magnitude** (KB normal, MB/GB bold).
  - **Tree view** — **`v`** toggles between a recursive tree (directories expand/collapse in place with `←`/`→`/`Enter`, indent + ▾/▸ glyphs) and the flat folder view (persisted). **`+`/`-`** step the tree's expansion one level deeper/shallower (expand-to-level).
  - **Fuzzy file finder** (`/`) filters the listing as you type; **`d`** toggles relative ("2d ago") vs absolute date stamps.
  - `↑↓`/`jk` navigate, `←`/`h` up a directory, `⏎`/`→` open a directory or focus the preview, `Tab` swaps panes; in the preview, `←`/`→` scroll horizontally. Click a row to select (again to open), wheel scrolls the pane under the cursor
- Diff modal with a clickable file list over the selected file's diff (stash, uncommitted, vs base branch, or **a branch's changes vs its base**); `Tab` switches focus between the file list and diff, with a footer that adapts to the focused pane; **status-filter chips** (`f` / click) with count badges when a change set has >10 files across ≥2 statuses; `Shift`/`Alt`+`PgUp`/`PgDn` page the file list; "no changes" shows a toast instead of an empty modal
- <a name="pr-viewer"></a>**PR viewer modal** — click any PR link (info-panel **Pull Request** value, repo-page HEAD `#N`, or a list **PRs** cell), or press `p` on a repo, to read the full pull request without leaving the terminal — as a **GitHub-style tabbed view**. A **pinned meta header** (state, `head → base`, author, **relative age** — "6 days ago", **hover** for the exact date/time — `+adds −dels`, labels) sits above a **tab bar** with count badges: **Description · Conversation · Commits · Checks · Files**. Switch tabs with `Tab`/`Shift+Tab`, `1`–`5`, or by clicking a chip. **Description** renders the body as markdown (headings, bullets, blockquotes, rules, links; raw HTML stripped). **Conversation** lists every review/comment as **collapsible sections** (click a `▾`/`▸` header; `z` or a `[− collapse all]`/`[+ expand all]` control folds them all when there's >1) and **`/` searches** them (filters to matches, highlights hits). **Commits** lists each commit (sha · subject · author · date). **Checks** shows each CI check colored by outcome (✓ pass / ✗ fail / ● pending / – skip), with a trailing **↗** you click to open that run on GitHub. **Files** shows a changed-files summary then the whole-PR diff, with a `raw`/`unified`/`split` toggle (`d` or the chips) — syntax-highlighted, the same renderer as the diff modal. One `gh pr view` call fills every tab and the badge counts (a loading skeleton shows while it runs); the Files diff loads lazily via `gh pr diff` the first time you open that tab. `j`/`k`·`↑`/`↓`/`g`/`G`/`PgUp`/`PgDn` + wheel + draggable scrollbar scroll the active tab; `o` opens the PR on GitHub; `esc`/`q`/`[x]`/outside-click closes
- Draggable scrollbars everywhere (the repo list, the info pane, preview, diff panels, help, repo page), highlighted while dragged — the list's grab takes priority over the splitter that sits right beside it
- Tabbed, **context-aware** help modal (`?`): **Hotkeys** (for the current view) · **CLI** · **Legend** (every glyph, both icon sets) · **About** · **Design** (Theme/Background/Contrast/Selection radios + a live palette swatch showcase + a **component showcase** — buttons, list rows, and radios drawn in every interaction state: normal / hover / selected / active / focused / disabled, under both hover and selection effects, plus a **[preview confirm dialog]** button that opens the shared confirm dialog live; the Design tab toggles between a **flat** scroll and a **tabbed** layout with vertical section tabs via `v`), switched with `Tab`/click (last tab remembered). The Hotkeys tab's `[K ⌨ keyboard]` (or `K`) pops a **responsive interactive keyboard viewer** — a full-blown bordered keyboard when there's room (compact strip when not), nav cluster on the right, every bound key highlighted; press/click any key (physical `Shift`/`Ctrl` too) to list what it does — and holding **modifiers filters to the exact chord** (e.g. `Shift+G` lists only the `Shift+G` binding). The Hotkeys list itself is rendered from the **same `keymap.json`** as the docs + this viewer, so it always reflects the real bindings (no hand-maintained copy to drift) — laid out as **grouped two columns that fill the modal**, with any `…`-clipped description showing its full text on hover, and inline markdown (`` `code` ``, **bold**, *italic*) rendered in the help and tooltips
- Settings modal (`,`) with a top **search box** (`/` focuses it — type to filter rows across every tab into a flat list with matched chars highlighted; `Esc` clears), sections: **Lists** (grouping · tree view · hide folder-header lines), **Theming** (Unicode ⇄ emoji icons · **hide zeros** · **theme** auto/dark/light · independent **background** normal/soft/**terminal** · **contrast** normal/soft · **list selection** blue/subtle · **button hover** inverted/subtle), **Sync** (auto-pull on launch · auto-pull limit `50/100/250/∞` · auto-pull in tree view), **Interaction** (**hover effects** · **changed-row effect** off/flash/highlight), **Layout** (panel padding · borders · pane splitter · repo-page tabs · auto branch-check), **Tooltips**, **Agent** (AI agent · skip permissions), and **Pull requests** (**Merged PRs** — show merged & closed PRs in the list column, off by default; detail views always show the PR) — all persisted; rows and radio chips are mouse-clickable, and every modal's hint footer lives on the bottom border (clickable, hover-highlighted). The `auto` theme **re-detects** dark/light at runtime, so an OS light↔dark switch re-themes live (no restart)
- **Hover effects** (on by default): when on, the actionable element under the mouse cursor (status-bar commands, footer hints, table-sort headers, the `t cols ▾`/`s sort ▾` triggers and their dropdown rows, info links/copy buttons, scrollbars, the splitter, list rows, repo-page rows) gets a subtle highlight. A status-bar command's key and its label highlight together; sortable column headers highlight too; and dwelling ~1s shows a tooltip — on a status-bar command (what it does), a column title (`wt`/`st`/`pr`/… meaning), or a group/folder header's right-corner count (its breakdown). A column-title tooltip for an optional column also carries a clickable red **`[x]`** that hides that column outright (the tooltip stays alive while the cursor moves onto it, so the `[x]` is reachable). Tooltips are placed by a floating-ui-style engine: a column-title tooltip **drops below the header** (anchored bottom-left), footer tooltips sit above, and each **flips** to the opposite side and **shifts** along its edge to stay on-screen. It enables all-motion mouse tracking, which takes over the terminal's own text selection / URL hover — so it's opt-in
- Web-like mouse support everywhere: full status-bar hints + the active `[filter]` tag clickable, the list pane's top-border triggers `f status ⟪…⟫ ▾ · s sort ⟪…⟫ ▾ · t cols ▾` (each opens a dropdown — filter / sort / columns; the filter and sort triggers carry their active `⟪…⟫` tag), every modal gets an `[x]` and closes on outside click, and every pane has window-control buttons on its top border — `m▢`/`m▣` maximize/restore (all panes) plus an `esc✕` close on the repo page (Unicode glyphs, or emoji in emoji mode)
- **Kebab (`⋮`) row menu** (`.`, or click the **`⋮`** that appears in the rightmost column when a row is hovered): state-aware per-repo actions. **Checkout branch…** opens a filterable picker of local + remote branches (after a `git fetch`); selecting one switches immediately on a clean tree, or pops a confirmation describing the uncommitted changes + risk when dirty. The headline item is **Copy cleanup prompt** — a prompt with every repo fact already embedded (branch, ahead/behind, working-tree, stash/branch/worktree counts, open PR) and a tailored cleanup checklist, so an AI agent doesn't have to re-run `git`/`gh` to discover the situation. A **`[ ] include cd … && claude '…'`** checkbox wraps it as a ready-to-run session command, and hovering the item shows a **live preview** of exactly what will be copied. The menu also surfaces Run agent · lazygit · diff · refetch · open remote (each with its hotkey)
- **New-build reload notice**: detects a newer binary installed over the running one and offers a one-key `Ctrl-R` reload / `Ctrl-X` dismiss (also clickable; exec-restart in the same terminal)
- **Version picker** (Build info → `p pin version`, or `p` from the Changelog/What's New dialog): pin/switch to any published release without leaving the terminal. The picker lists every release (fetched live from GitHub), downloads the chosen one for your platform, installs it over the running binary, and reloads into it. By default it only offers versions that themselves have the picker (v2.72.0+) so you can always switch again from inside the app; `a` reveals older builds (dimmed, tagged "no in-app switch"), and pinning one shows a warning plus a copyable command to reinstall the latest (Linux/macOS; hidden on native Windows). Release notes render markdown, and `m` maximizes the modal
- **Changelog / What's New / version-picker modals** render release-note markdown (`**bold**`, `` `code` ``), wrap long lines, and maximize ⇄ restore with `m` (or the `[m maximize]` title-bar button)
- Non-TUI fallback (same output as bash reference) when not on a TTY or with `--no-tui`
- Exit codes: 0 (all ok), 1 (any failed), 2 (user quit mid-run), 130 (Ctrl-C)

## Installing

polygit is a single binary for **Linux, macOS, and Windows** (native `x86_64-pc-windows-msvc` — WSL works too, but isn't required).

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

# Self-update to the latest published release (alias: upgrade)
polygit update

# Plain streaming output (matches bash reference for a flat dir; lists nested repos too)
polygit --no-tui [DIR]

# Custom concurrency
polygit -j 8 [DIR]
PULL_JOBS=8 polygit [DIR]

# Cap scan depth (1 = immediate subdirs only — the legacy single-level scan)
polygit --depth 3 [DIR]
polygit --no-recursive [DIR]

# Custom timeout per pull (default: 10s)
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
| `1` / `2` / `3` / `4` | Focus the list / info / result / repo-page panel directly — or, when a pane is maximized, swap which pane fills the screen |
| `m` | **Maximize / restore** the focused pane (every pane — list, info, result, repo page). Also the `m▢`/`m▣` button on each pane's top border. Only the repo page's maximize is sticky; the others are session-only |
| `PgUp` / `PgDn` | Scroll the result/log panel (when panel `[3]` is focused) |
| `End` | Resume auto-scroll in preview |
| `Enter` / double-click | Open the dedicated repo page for the selected repo (on the repo list); on a folder/collapsible group header: collapse/expand |
| `r` | Retry selected repo if it has an issue — or, on a folder/group header, every repo it covers |
| `R` | Retry all repos with an issue (failed or skipped) |
| `e` | Refetch selected repo (re-pull regardless of status) — or, on a folder/group header, every repo it covers |
| `E` | Refetch all repos that aren't currently in progress |
| `u` | Refresh the selected repo's local git facts — branch, ahead/behind, dirty, stash (no pull) |
| `U` | Refresh all repos' local git facts (no pull) |
| `i` | Toggle the info panel — an additive block above the log/diff, grouped into sections: **Remote** (remote, branch) · **Status** (status, pull request, ahead/behind) · **Activity** (pulled delta, last commit, changes, worktrees) · **Path** |
| `L` | Cycle the info panel's **grouping layout** — titled sections → spaced (blank lines) → flat — also in **Settings → Layout → Info layout**. A **maximized** info pane always shows titled sections. Persisted |
| `I` | Toggle the result/log panel (the bottom of the preview). Info + result are independent panels split by a **draggable boundary**; hide the result panel and the info panel fills the pane (reads like the repo list). Clickable `I log` footer hint; persisted |
| `d` | Cycle the result view: **log → raw → unified → split** (or click a chip in the pane's footer switcher). `log` = the pull/command output; `raw` = git's colored diff; `unified`/`split` = syntax-highlighted, GitHub-PR-style. Diff = working-tree changes, else the last pull's diff. The choice persists |
| `t` | Open the **columns dropdown** (or click `t cols ▾`): pick with the mouse, `↑↓`+`Enter`, or a row's mnemonic `u`/`a`/`d`/`l`/`w`/`b`/`s`/`p`/`c`/`r`/`f`. Toggling keeps it open (multi-toggle); `Esc`/`q`/`[x]`/outside-click closes |
| `s` | Open the **sort dropdown** (or click `s sort ⟪…⟫ ▾`): pick with the mouse, `↑↓`+`Enter`, or a row's mnemonic `n`/`c`/`s`/`a`/`d`/`l`/`w`/`b`/`k`/`p`/`g` → name / branch / status / ahead-behind / dirty / last-commit / worktrees / branches / stashes / pulled / changed. Re-pick (or click a column header) flips `▲`/`▼`; the list is always sorted (Name asc by default) |
| `f` | Open the status-filter dropdown — `a`/`u`/`c`/`s`/`f`/`i` (or hover+click) filter the list by all / updated / up-to-date / skipped / failed / issues (applies on top of `/`) |
| `o` | Open the selected repo's remote in the browser |
| `p` | Open the selected repo's **pull request** in polygit's PR viewer (uses the detected open PR; toasts when none). **Note:** the fuzzy finder moved to `Ctrl+P` |
| `P` | Open the PR **on GitHub** — the existing PR, else the **compare page** for this branch vs its base (`…/compare/<base>...<branch>?expand=1`); no-op on the base branch |
| `y` | Copy the selected repo's **absolute path** to the clipboard (every copy confirms with a toast previewing the copied text) |
| `Y` | Copy the selected repo's **remote (origin) URL** to the clipboard |
| `c` | Launch the selected AI coding agent in the selected repo (suspends the TUI, returns on exit). Which agent — claude / codex / gemini — and whether to skip its approval prompts are set in Settings → **Agent** |
| `l` | Open **lazygit** in the selected repo (suspends the TUI, returns on exit) |
| `Ctrl-E` | Open the **file explorer** for the selected repo (also the `Explore files…` kebab item): a two-pane browser — a directory **list** (name + toggleable `1`-`5` size/permissions/modified/created/kind columns, off by default) beside a **syntax-highlighted preview**, split by a draggable divider (`[`/`]`). `↑↓`/`jk` move · `←`/`h` up a dir · `⏎`/`→` open dir or focus preview · `tab` swap panes · `esc`/`q` close |
| `b` | Toggle the selected repo as a **favorite** (★) |
| `B` | Toggle the **★ Favorites** section pinned to the top of the list |
| `x` | Clear **this repo's log buffer** (empties the streamed pull output) |
| `D` | Open the [documentation website](https://steven-pribilinskiy.github.io/polygit/) in the browser |
| click `built … ago` | Open **Build info** (binary age/size, build duration, settings preview) — and `p pin version` to pin a release |
| `p` (in Build info) | Open the **version picker**: pin/switch to any published release (downloads + installs it, then reloads). Defaults to picker-capable versions (v2.72.0+); `a` shows older builds (with a copyable reinstall command); `Enter`/`p` pins the selected one |
| `m` (Changelog / What's New / picker) | Maximize ⇄ restore the modal (also the `[m maximize]`/`[m restore]` title-bar button) |
| `,` | Open the settings modal (panel padding, grouping, tree view, icon style, theme, background, contrast, list selection, button hover, **auto-update**) |
| `Ctrl+K` | Open the **keybindings editor** — remap any shortcut (see [Remapping shortcuts](#remapping-shortcuts)). Also the `[^K remap]` button on the help **Hotkeys** tab |
| `?` | Open the help modal (docs/GitHub/notes links, all keys, flags & env) |
| `/` | Filter repos by name |
| `Esc` | Clear filter (or quit when no filter) |
| `q` | Quit |
| `Ctrl-C` | Quit (exit 130) |

**Retry vs refetch:** retry only re-runs repos that need it (failed/skipped); refetch re-runs any repo even if it was already up to date. In the status bar, `r`/`R` dim when no repo has an issue, and `e`/`E` dim when there's nothing eligible (the selected repo is still in progress).

The repo list, the log/diff preview, the help modal, and the repo page all show a scrollbar when their content overflows. **Clickable commands:** the action hints in the status bar (and the `t` column menu) are mouse-clickable — clicking one runs the same command as the key.

### Remapping shortcuts

Every shortcut above (plus the repo-page keys) is **remappable**. Press **`Ctrl+K`** (or the **`[^K remap]`** button on the help **Hotkeys** tab) to open the **Keybindings editor**: actions grouped by area, each showing its current key(s).

- **`↑↓`/`j`/`k`** move · **`Enter`** (or click `[set]`) rebinds the selected action — **press the new key** · **`c`** (or click `[x]`) clears it · **`d`** resets that action to its default · **`R`** resets everything (with a `y`/`n` confirm) · **`Esc`** closes.
- Rebinding to a key another action already uses prompts to **reassign** (move the key to the new action) or cancel.
- Overrides persist to **`~/.config/polygit/keybindings.json`** — a plain, hand-editable `{ "<action>": ["<key>", …] }` map. Only changed actions are written, so default keys added in later versions still reach older config files. Keys are written like `ctrl+f`, `G`, `shift+enter`, `space`, `/`. Example — move the fuzzy finder to `Ctrl+F`:

  ```json
  { "open_finder": ["ctrl+f"] }
  ```

- Resolution is **context-aware**: the same letter can do different things on the repo list vs the repo page without colliding. Unbinding a key (clearing it) genuinely frees it — it no longer triggers the old action.
- **`Esc`**, **`q`**, **`Ctrl+C`**, and the new-build **`Ctrl+R`** / **`Ctrl+X`** stay fixed (not remappable), so you can always escape and quit.

### Repo page (`Enter` / double-click)

Opens the repo page (**panel `[4]`**) for the selected repo that runs `git fetch` and lists every local branch (with HEAD marker, fresh ahead/behind vs upstream, upstream name, last-commit date, subject), every worktree (branch + path), and every stash. By default the page is **restored** — a docked panel across the bottom, so the list above stays live and selecting another repo retargets the page (master-detail); press `m` (or the `m▢` title-bar button, next to the `esc✕` close button) to **maximize** it full-screen, `m▣`/`m` to restore. **When maximized the page is a single view** — every section (branches/worktrees/stashes/commits) stacked under its header, with no tab bar; the tabbed layout only applies while restored. The window state is sticky (persisted) and also set in Settings → Layout → **Repo page**. While restored, the page is panel `[4]` — `1`/`2`/`3`/`4` jump focus to the list/info/result/repo-page panels without leaving it (from a maximized page, `1`/`2`/`3` restore the layout first), and the footer is **single and focus-aware**: repo-page keys while `[4]` is focused, the main-view keys while a list panel is focused — only the active panel's keys, never both at once. A header row labels the branch columns, which are toggled with the page-local `t` menu (then `b`/`y`/`a`/`m`/`d`/`c`/`u`/`f`/`g`/`r`/`s` for ahead-behind / dirty / added / modified / deleted / total / upstream / base / age / **pr** / subject — clickable chips, persisted). The **pr** column shows the current branch's pull request — open, merged, or closed (a clickable `#N`, via `gh`) on the HEAD row, blank elsewhere; the info panel labels its state. **Stash rows** flow through the same change-stat columns (added/modified/deleted/total), loaded lazily from `git stash show`. The **added/modified/deleted** counts are each branch's changes vs the merge-base with its **base branch** — the auto-detected fork parent (the branch it most directly diverged from, weighing both local heads and remote-tracking branches, so a branch cut from a non-`main` integration branch resolves correctly), or a per-branch **override** you set — loaded in the background (cells show `…` until ready); a column every branch leaves empty auto-hides and its chip goes dim. The **base** column shows that resolved base per branch (blue when auto-detected, magenta with a trailing `*` when overridden). Count cells show a dim zero rather than a blank. The bottom **info panel** (`i`, persisted) details the selected row: branch, upstream, base branch + merge-base sha, ahead/behind, change stats, and the tip commit (sha · author · date · subject). Sections are prefixed with type icons; worktrees/stashes sections only appear when non-empty. The selection starts on the current (HEAD) branch. Navigate rows with `j`/`k`/`g`/`G`/`Home`/`End` (or the wheel / click); `Enter` (or double-click) opens the diff modal on a stash, a dirty row, or **a branch (its changes vs the base branch)**; `Shift+Enter` checks out the selected branch (clean, non-current); `p`/`P` fast-forward; `d` performs the row-appropriate action (delete branch / drop stash / remove worktree / discard) — the footer hint is dynamic; `b` (or clicking the **base** cell) opens the **base-branch picker** to override which branch this branch's stats diff against — pick *auto-detect* to clear the override; the choice is persisted per repo+branch; `c` starts claude code; `l` opens lazygit; `o` opens the branch on the remote (e.g. GitHub) in the browser; `y` opens a copy menu (absolute path / branch name / both); `m` maximizes/restores the page; `?` shows the page's hotkeys; `Esc`/`q` returns. An action result (e.g. "Dropped stash@{0}") shows in a banner at the bottom.

`Enter` or a double-click opens a 90%-of-screen **diff modal**, two bordered sub-panels: a scrollable **file-list panel** (top, ≤40% height) over the **selected file's diff** (bottom). The footer adapts to the focused pane. `Tab` switches focus between the panels (the focused one gets a bright border); `j`/`k`/`g`/`G` then drive that panel. Pick a file with `↑↓`/`j`/`k` or by clicking it; its diff loads beneath. `PgUp`/`PgDn` page the diff; `Shift`/`Alt`+`PgUp`/`PgDn` page the file list; `Shift`/`Alt`+wheel scrolls the file list. When a change set has more than 10 files across at least two statuses, a **status-filter chip row** appears (`[ all N ] [ M … ] [ A … ] …` with count badges) — click a chip or press `f` to cycle, and the list groups by status. The diff-panel title shows the full path, left-truncating only when it doesn't fit. For a dirty row, `t` toggles the file set between *uncommitted* (vs HEAD) and *vs base branch*; a stash lists its files; a clean branch shows its changes vs the base branch. **`v` cycles the diff render style — raw / unified / split** (persisted, shown in the footer): **raw** is git's own colored output; **unified** and **split** are structured, line-numbered, **syntax-highlighted** GitHub-PR-style views (split shows old left / new right) with a faint green/red wash on added/removed lines. `d` discards/removes/drops (with confirm); `Esc` closes. When there's nothing to show, a "no changes" toast appears instead of an empty modal.

### Columns (`t` / `t cols ▾`)

The list always shows the status glyph + name + branch + a dirty marker (an amber `•` for any repo with uncommitted changes — amber, not red, since it's a "modified" state, not an error). Press `t` (or click the **`t cols ▾`** trigger on the list header) to open the **columns dropdown**; pick with the mouse, the `↑↓` arrows + `Enter`, or each row's mnemonic letter (toggling keeps the dropdown open so you can flip several in a row). The columns: `u` status text (a short label per state — `queued`/`running`/`up-to-date`/`updated`/`no upstream`/`dirty`/`throttled` — with the specific failure kind on failed repos: `not found`, `auth`, `diverged`, `not a repo`, `timeout`, `network`, `lock`, and `ref gone` for a deleted upstream ref), `a` ahead/behind, `d` adds the dirty **count** (`•N`) to the always-on marker, `l` last-commit age, `w` worktree count (`⑃N`, cyan), `b` feature-branch count (`⑂N`, green — local branches excluding `main`/`dev`), `s` stash count (`≡N`), `p` commits the last pull landed (`⇣N`, green), `c` files the last pull changed (`±N`, cyan), `r` the **pull request** for the current branch (a clickable `#N`, via `gh`, colored by state — green open / magenta merged / gray closed). Count columns render a **dim zero** rather than a blank in the Unicode set, so the column shape stays recognizable (in **emoji** mode a zero cell is hidden — a colorful emoji next to `0` reads as clutter). A column every repo leaves empty (no worktrees, no stashes, or ≤1 branch everywhere) auto-hides once its data has loaded, and its dropdown row renders dim + inert; the **pulled**/**changed** columns stay hidden until a pull lands a delta, then remain for the session (so a retry doesn't flicker them in and out). The git-derived columns fetch per-repo details in the background the first time one is enabled (cells show `…` until ready); `w` is free from worktree discovery, and pulled/changed come straight from each pull. Enabled columns persist across runs.

### Info panel (`i`)

`i` toggles an info block above the right pane's content (the pull log or the diff) for the selected repo. Fields are **grouped into sections** — **Remote** (remote URL, branch) · **Status** (status with how long the pull took, pull request, ahead/behind) · **Activity** (the **Pulled** delta, last commit, changes, worktrees) · **Path**. The **Pulled** delta (shown when the repo updated this run) is `oldsha → newsha`, then `N commits · M files (+ins −del)`, then — when the fetch brought tags — `N new tag(s): <comma-separated names>` **truncated with a click to expand** (same as the last-commit subject), plus `N new branches`. The **`L`** key (or **Settings → Layout → Info layout**) switches the grouping between **titled sections** (dim UPPERCASE headers + blank lines), **spaced** (blank lines only), and **flat** (dense); a **maximized** info pane (`2` then `m`) always shows titled sections. The block is additive — the log/diff stays beneath it — and tracks the selection as you move. Layout choice and the extra git facts (fetched lazily per selected repo, re-fetched after a pull) persist / reflect the new HEAD.

The panel is interactive (it's a web app in a terminal):

- **Bold field labels**; rows that would carry nothing are hidden — no `↑0 ↓0`, no all-zero Changes line, no empty Worktrees.
- **Clickable links** (when the remote is a browsable https host): the **branch** opens `…/tree/<branch>`, the **commit hash** opens `…/commit/<sha>`, and **Remote** opens the repo — all in your browser.
- **Truncated values expand on click.** A long commit subject is truncated from the right; click the underlined value to expand it — the full text wraps starting at the value column, never under the label. Click again to collapse.
- **Click-to-copy**: on the **Path** and a non-link **Branch** rows the value + a trailing standout `⧉` copy the value (hover highlights that region; the field label is *not* part of the click target). **Worktrees** lists one branch per line, each its own copyable line (so you copy a single worktree branch, not all of them concatenated). When the branch is a clickable link, clicking the name opens it and a separate, dim 2-char `⧉` copies it (copy is the secondary operation there). A `⧉` on the log pane's top border copies the whole pull log.

`c` launches an AI coding agent in the repo dir. Pick the agent in Settings → **Agent**: `claude` (default), `codex`, or `gemini`; the optional **Skip permissions** toggle appends that agent's bypass-all-prompts flag (`--dangerously-skip-permissions` / `--dangerously-bypass-approvals-and-sandbox` / `--yolo`). On Unix the command runs via an interactive `bash` (so a shell alias resolves); on Windows via `pwsh` in the repo dir. `PULL_CLAUDE_CMD`, when set, overrides the whole command verbatim.

### Settings modal (`,`)

`,` opens a small settings modal (from the list or the repo page), organized into **Lists** (grouping, tree view, hide folder-header lines), **Theming** (icons, hide zeros, theme, background, contrast, list selection, button hover), **Sync** (auto-pull policy), **Interaction** (hover effects, changed-row effect), **Layout** (panel padding, borders, pane splitter, repo-page tabs, auto branch-check), **Tooltips** (an "All tooltips" bulk toggle plus per-area switches — footer commands, column headers, group counts, settings rows, help links; "All tooltips" sets every area on/off, and shows **neither** radio when the areas are mixed), **Agent** (AI agent + skip permissions), and **Pull requests** (**Merged PRs** — surface merged & closed PRs in the list column, off by default; detail views always show the PR) sections. By default it's a **tabbed** (JetBrains-style) layout — the sections are vertical tabs on the left; switch tabs with `Tab`/`Shift+Tab` (or `Shift+↑`/`Shift+↓`), move between rows with `j`/`k` (or `↑`/`↓`), change the selected setting's value with `←`/`→` (or `Space`/`Enter` to toggle/cycle), and close with `Esc`/`q`/`,`/`[x]`/a click outside. Press `v` (hint in the bottom border) to cycle the layout **tabbed → accordion → flat**. The **accordion** layout stacks every section under a collapsible header — `j`/`k` (or `↑`/`↓`) move through both the headers and the rows of expanded sections, `Space`/`Enter` (or `←`/`→`) on a focused header folds/unfolds it (a focused header has no active child row — it's the header itself that's selected), clicking a header focuses **and** toggles it, the top `[- collapse all]` / `[+ expand all]` button folds them all, and the content scrolls when it overflows the modal. The folded set persists. The **flat** layout stacks every section expanded in one scroll. Press **`R`** (or the `R reset` footer chip) to **reset every setting to its default** — a confirmation modal lists each setting that will change as an aligned `current → default` column (the new value highlighted green); favorites, workspaces, and caches are left untouched. With the mouse, click a tab to switch, a radio chip to set **that** value (clicking the already-active chip does nothing), or the row label to cycle to the next value (3-radio settings cycle left→right and wrap). With hover effects on, dwelling ~1s over a setting shows a tooltip explaining it (e.g. the **unicode** icon option notes that Unicode glyphs can be colorized per type, unlike emoji). All settings persist across runs (in `~/.config/polygit/state.json`):

- **Panel padding** — adds a 1-cell inner padding inside every bordered panel and modal.
- **Icons** — switches the status / column / marker glyphs **everywhere** (list, columns, repo page, Result/Errors pages, log markers) between the default Unicode set (`◌ ✓ ◇ ✗ ⑂ ≡ •`) and an emoji set (`✅ ✨ 🚫 ❌ 🌿 📦 📝`). Columns stay aligned in either mode — only single-codepoint, reliably-2-cell emoji are used (no variation-selector glyphs), and the tight ahead/behind column keeps compact `↑↓` arrows.
- **Hide zeros** — hides zero-count column cells (a dim `0` becomes blank) so only repos with real ahead/behind/dirty/worktree/branch/stash counts show a number. Emoji mode always hides zeros, so this row renders **force-selected and inert** while emoji icons are active.
- **Theme** — `dark` / `light` paint a full explicit palette (background, text, and every accent color) so the app looks identical regardless of the terminal's own color scheme. `auto` detects whether the terminal background is dark or light at startup and applies the matching palette — via an OSC 11 query of the terminal, falling back to `COLORFGBG`, the Windows light/dark setting under WSL (covers terminals that follow the system theme but don't answer OSC 11, e.g. Tabby), and the macOS appearance setting.
- **Background** and **Contrast** — two independent `normal` / `soft` axes. **Background** softens the surface tones (background, selection, shadow); **Contrast** narrows the text/background distance and desaturates the accent + semantic colors. They compose, so you can soften the surface while keeping vivid text, or vice versa. (Pre-split state files, which had only `contrast`, load with both axes set from the old value.)
- **Changed-row flash / highlight** — how a repo row signals that a pull/refresh changed one of its cells. **flash** pulses the changed cells; **highlight** holds them steady for the attention window. Independent toggles (turn both off to rely on the status text column instead).
- **Auto branch-check** — `off` / `auto`. When `auto`, polygit periodically refreshes each repo's local git facts (branch, ahead/behind, dirty, stash — **no pull**) so the list stays current without manual `u`/`U`. The interval scales with the repo count (~`repos/10` seconds, clamped 1–60s; 10 repos → ~1s, 100 → ~10s) and pauses while any pull is in flight.
- **Borders** — draw the rounded borders around the two main panes. Off reclaims the border rows/columns (the pane titles float on the top row and content fills the freed space).
- **Pane splitter** — how the draggable splitters are shown. **Dedicated** reserves a real 1-cell lane — a column between the list and the right pane, a row between the dock / info-result panes — filled with a persistent `▒` grip (clearly visible, costs one cell). **On hover** (default) keeps the panes flush (zero-width boundary) and shows only a thin grip (`▏` vertical, `▁` horizontal) under the cursor when it crosses a splitter hotspot. The two are mutually exclusive; the drag hotspots work in both, and the grip brightens to cyan on hover / `█` while dragging.
- **Repo page** — the window state the repo page (`[4]`) opens in: **restored** (a docked bottom panel — the list/info/result panels stay visible above it, master-detail) or **maximized** (full-screen). Toggle live with `m` or the `m▢`/`m▣` title-bar button (next to the `esc✕` close button); this setting picks the default and the choice is sticky (persisted). Maximized renders as a single view with no tab bar. When restored, **drag the panel's top edge** to resize it (the height is persisted) — a second splitter alongside the panes' divider.
- **Repo page tabs** — `auto` (default) splits the repo page into **Branches / Worktrees / Stashes / Commits tabs** when at least two sections have content (switch with `Tab`/`Shift+Tab` or click a tab chip); `off` keeps it as one scrolling list. Pressing **`v`** flips the current view tabbed⇄flat as a session override — it does **not** change the `auto` preference. The **Commits** tab lists recent commits on the current branch with a GitKraken-style **commit graph** (truecolor branch lanes, `●`/`◆` nodes) to the left, then sha · date · author · subject, read-only.
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

### Sorting (`s` / `s sort ⟪…⟫ ▾` / column headers)

The list is always sorted — **Name ascending** is the default. Press `s` (or click the **`s sort ⟪…⟫ ▾`** header trigger, which shows the current sort + direction) to open the **sort dropdown**, then pick with the mouse, the `↑↓` arrows + `Enter`, or a row's mnemonic key (`n` name, `c` branch, `s` status, `a` ahead/behind, `d` dirty, `l` last-commit, `w` worktrees, `b` branches, `k` stashes, `p` pulled commits, `g` changed files) — or just click a column header (including the **branch** header). Picking a sort closes the dropdown; re-picking the active sort (or re-clicking the header) flips the direction, shown as `▲`/`▼` on both the active column header and the `s sort ⟪…⟫ ▾` trigger. **Ties break by name** (A→Z) — repos sharing a column value (e.g. the same branch) list alphabetically within that group, and the name tiebreak stays ascending even under a descending column. The order persists across runs.

### Help modal (`?`)

`?` opens an in-app reference with five tabs — **Hotkeys** (contextual, with short sections laid out side by side), **CLI** (an **interactive command builder**, source-driven from the real clap flags — every flag (toggles *and* value flags) is a **checkbox** row: `Space` toggles it, `Enter` edits a value flag (typed values **auto-apply** live and check the flag on), `f` swaps a flag's **short ⇄ long** form (`-j` ⇄ `--jobs`), and `h` cycles the help-text display (`always` / `on hover` / `never`, persisted — also a clickable chip group). Child flags (e.g. `--no-recursive` under `--depth`, `--profile-out` under `--profile`) indent under their parent and go **disabled + dimmed** when the parent is unchecked (unchecking a parent removes its children). The assembled command renders as an **aligned, multiline `polygit … \`** preview where every token is **clickable to remove** that flag (hover highlights its row above, with a "click to remove" tooltip); the whole command is **clickable to copy** (also `y` / **[ copy ]**); plus exit codes), **Legend** (every glyph in both icon sets with its meaning), **Design** (the Theme/Background/Contrast/Selection radios — click a chip to set that value, click the row label to cycle; the autodetected theme is underlined when `auto` is active — above a live swatch showcase of the palette's semantic colors that re-themes as you change them, then a **component showcase** rendering the reusable `tui-pick` button / list-row / radio primitives in every interaction state (normal · hover · selected · active · focused · disabled) under both hover and selection effects, with the active choice marked, plus a clickable **[preview confirm dialog]** button that opens the shared confirm dialog (the same one every destructive action uses — its yes/no are hover-highlighted footer-style chips); icon set lives in Legend. The Design tab itself has a **flat ⇄ tabbed** layout toggle (`v`, like the settings modal): **flat** stacks every section in one scroll; **tabbed** shows the sections (Theming · Palette · Buttons · List rows · Radios · Dialogs) as clickable **vertical tabs** with `[`/`]` to move between them), and **About** — switched with `Tab` (the last tab is remembered across opens, except About). The About tab groups its links (**polygit** docs + GitHub, **lazygit**, and a collapsible **Notes** group you expand with a click) and shows **titles only** — hover a link to see its URL in the bottom-left corner (browser-style), or dwell ~1s for a tooltip. The links are clickable (open in your browser via `$BROWSER`/`wslview`/`xdg-open`). Scroll with `j`/`k`, `g`/`G`, `PgUp`/`PgDn`, or the wheel; **maximize/restore** the modal to ~90% of the viewport with `m` (or the `[m maximize]` button); close with `?`/`Esc`/`q`/`[esc]`/a click outside.

Press `/` on **any tab** to **search** it (type to filter, `Esc` clears) — the Hotkeys tab matches lazygit-style against the **key column AND the description** (prepend `@` to match the keys column specifically), the other tabs are a plain text filter over their content. `[K ⌨ keyboard]` (or pressing `K`) opens an interactive keyboard viewer (the same one on the [docs site](https://steven-pribilinskiy.github.io/polygit/guides/keybindings/), built from the same keymap): an OS-aware on-screen keyboard with the bound keys highlighted, and the standard nav/arrow cluster (`ins`/`home`/`pgup` over `del`/`end`/`pgdn`, then the inverted-T arrows) to its right. It's responsive — on a large terminal the keys grow into a full-blown keyboard of tall bordered keys; on a small one it falls back to a compact strip with the cluster below. Press or click any key — including physical `Shift`/`Ctrl` (on terminals with the Kitty keyboard protocol) — to highlight it and list everything it does in a scrollable panel below, whose keys column auto-sizes to the longest combo shown. **Held modifiers filter the panel to the exact chord**: pressing `Shift+G` lists only the `Shift+G` binding, `Ctrl+R` only the `Ctrl+R` one, while a plain key shows every variant (`g`, `G`, `^G`, …). `Esc` closes the viewer.

### Mouse

Click a repo row to select it (a click also **focuses** whichever panel it lands
in, like the `1`-`4`/`Tab` keys). Over the **left pane** the plain wheel **scrolls
the list view** (web-app style — the selection stays put and may scroll out of
view), and **`Alt`+wheel moves the selection** (like `↑`/`↓`); keyboard nav always
scrolls just enough to keep the selection on screen. Over the **right pane** the
wheel scrolls whichever sub-pane is under the cursor — the info pane ([2]) or the
preview/log ([3]). Click or drag the list, info, or preview scrollbar to jump/scroll, and
drag the divider between the panes to resize — a grab on the list scrollbar (which sits right
beside the divider) scrolls the list rather than starting a splitter drag. On the preview/repo-page, hold a
modifier while scrolling to go faster: **Shift** jumps 5× the normal step, **Ctrl**
scrolls a full page. While the TUI is running it captures the mouse, so native
terminal text-selection is suspended until you quit (same tradeoff as lazygit/htop).

Everything actionable is clickable like a web page:

- **Status-bar hints** — the whole hint ("/ filter", "vg groups", …), not just the key. (Filter/columns/sort moved to the `f status ▾` / `t cols ▾` / `s sort ▾` header triggers, so they're no longer in the footer.) While a leader menu is armed (e.g. `v`/`z`) the rest of the footer dims and goes inert, and the armed leader's own trigger gets a highlight pill so it's clear which menu is open. The active tags sit next to their hints and are clickable too: `[needle]` clears the name filter, `{failed}` resets the status filter. In "[ ] resize", `[` and `]` nudge the split directly. The right side shows a clickable `vX.Y.Z` tag (opens the **Changelog**), a clickable `built … ago` tag (opens the Build info modal), and clickable `, settings · ? help · q quit`.
- **Modals** (settings, copy menu, confirm, diff, help) — every modal has an `[x]` close button on its top border and closes/cancels when you click anywhere outside it. Clicks inside a modal never fall through to the view behind.
- **Settings** — click a row label to select it, or click a radio chip (`● dark`, `○ off`, …) to set that exact value.
- **Confirm dialogs** — `[y/enter] yes` and `[n] no` are clickable.
- **Copy menu** — click an option to copy it immediately.
- **Repo page** — a clickable `[esc back]` button on the top border returns to the list.

### New-build reload

While running, polygit watches its own binary on disk. When a newer build is installed (e.g. `make install`'s atomic rename), a persistent notice appears in the top-right (inset with the panel-padding setting, with a glint sweeping its border): `↺ new build installed · [^R reload] [^X]`. It rides on top of every screen — the repo list, the repo page, and any open modal — so it's never hidden. **`Ctrl-R`** (or clicking `[^R reload]`) restores the terminal and `exec`s the new binary with the same arguments — the fresh process re-scans and re-pulls (instant when everything is already up to date). **`Ctrl-X`** (or clicking `[^X]`) dismisses the notice; it re-arms if the binary changes again. Because the notice floats over every view, both keys work from anywhere (including inside a modal).

Clicking the **`built … ago`** tag in the status bar opens a **Build info** modal: the running version, when it was built **and how long the build took**, the **binary size** + watched path, the **settings file location** + how many files live in the config dir, whether a newer build is waiting, and a **collapsible tree view of `state.json`** — a format-agnostic structural-data viewer (not JSON-specific). Objects and arrays start **collapsed**, each showing its child count in a faint `{N}` / `[N]`; `j`/`k` move, `←`/`→` collapse/expand, `Space`/`Enter` fold the selected node, `-` / `+` fold/unfold all (also the clickable `[- fold all]` / `[+ unfold all]` buttons on the card header), and clicking a node folds it. The plain mouse wheel scrolls the tree (selection untouched, web-app style) and `Alt`+wheel moves the selection — same model as the main list. `r` (a clickable hint in the bottom border, alongside `esc close`) exec-restarts into the latest build; `esc`, the `[x]`, or a click **outside** closes it (a click inside is inert).

### Auto-update

Separately from the local new-build watcher above (which reloads into a binary you installed yourself, e.g. via `make build`), polygit can keep itself current from **published GitHub releases**. Set it in **Settings → Updates**:

- **Auto-update**: `off` (never check) · `notify` (toast when a newer release exists, and show an `↑ vX.Y.Z available` line in Build info — install it with `p`) · `install` (download + stage the newer release over the binary automatically, then the same `↺ new build installed` notice prompts a `Ctrl-R` reload — your session is never interrupted).
- **Update check**: how often the check polls GitHub — `daily` or `weekly`. It also runs once at launch when the last check is older than the interval, and the last-check time persists, so it never hammers the API.

Self-install covers Linux and macOS (the same platforms the version picker supports); on native Windows the check is inert.

### Changelog & What's New

Clicking the **`vX.Y.Z`** tag in the status bar opens the **Changelog** modal — every release as a collapsible accordion whose header is the version plus a **time-ago** (`▸ v2.60.0 · 2d ago`), the **latest two expanded**. `j`/`k` select a release, `Space`/`Enter` (or a click on the header) folds/unfolds it, `g`/`G` jump, and `esc`/`[x]`/outside-click close. After you **reload** into a newer build, a **What's New** modal pops automatically, listing every release since the version you last ran (all expanded). The notes come from an embedded `CHANGELOG.md`, so the binary carries its own history wherever it's installed.

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
