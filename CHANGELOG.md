# Changelog

Release notes shown in-app (the `vX.Y.Z` status-bar tag opens this; a What's New modal
pops after reloading into a newer build). Format: `## vX.Y.Z — YYYY-MM-DD` then notes.

## v2.101.1 — 2026-06-29
Restore the grouped two-column Hotkeys layout (still keymap-driven)
- the **`?` help → Hotkeys** tab is back to the **grouped, two-column** look — each `group` from
  `keymap.json` (Navigate, Panes & views, Find & sort, …) renders as a cyan subhead with its
  `keys  action` rows, short groups paired side-by-side — instead of the flat list from 2.101.0
- still generated entirely from `keymap.json`, so the list can't drift from the real bindings; long
  actions are truncated with `…` to keep the columns narrow (full text stays in the `K` keyboard
  viewer + the docs)
- new `help_covers_every_binding` test asserts **every** `keymap.json` binding is rendered in the
  help for its view, so a new/renamed hotkey can never silently fall out of the Hotkeys list

## v2.101.0 — 2026-06-29
Help Hotkeys list is generated from the keymap; keyboard viewer filters by modifier
- the **`?` help → Hotkeys** list is now rendered from the **same `keymap.json`** as the docs + the
  keyboard viewer, instead of a hand-curated copy — so it always reflects the real bindings (adding,
  changing, or removing a hotkey updates it everywhere at once; no drift). Each row shows its keys,
  action, and the clarifying note.
- in the **keyboard viewer** (`K`), **held modifiers now filter** the actions panel to the exact
  chord: `Shift+G` lists only the `Shift+G` binding, `Ctrl+R` only `Ctrl+R`; a plain key still shows
  every variant (`g`, `G`, `^G`, …). Modifier-ness is computed per key, so a mixed binding like
  `g G Home End` doesn't pollute the Shift filter.

## v2.100.0 — 2026-06-29
Repo page: smarter base, flexible upstream/base columns, ref-gone marker, leaner defaults
- **base resolution fixed** — the `base` column now only ever resolves to a **conventional
  integration branch** (`dev` / `stage` / `main` / …), never a sibling feature branch the branch
  happened to fork from. Priority: explicit override → the **open PR's target** branch (so it matches
  where the branch actually merges) → the closest conventional branch → the repo's default branch.
- the **upstream** and **base** columns now **grow to fit** their longest value (up to 40 cols)
  instead of truncating at a fixed 28 — long refs are readable, and leftover width still flows to the
  subject column.
- a branch whose tracked upstream was deleted now shows a **`✗` "ref gone"** marker (red) in the
  `upstream` column and an explicit note in the info panel.
- the repo page's **default columns are leaner** — the per-status `+a ~m -d` breakdown is **off** by
  default (the `Σ` total covers it; enable the split via `t cols ▾`).

## v2.99.0 — 2026-06-28
Kebab menu looks like a dropdown; help lists favorites + the kebab menu
- the kebab (`⋮`) menu now renders like the header dropdowns: **anchored under the `⋮` button**
  (right-aligned, opening leftward), **dividers** between groups (repo actions · the cleanup-prompt
  group · launch/git actions), a `▸` selection marker, and it **clamps to the viewport** so it's
  never cropped off the bottom.
- the **`?` help → Hotkeys** list (a curated list separate from the keymap data) now includes the
  **Favorites & menu** section — `.` open the kebab menu, `b` favorite, `B` favorites-first — and the
  Find & sort entries note the new **favorites** filter + **favorite** sort.

## v2.98.1 — 2026-06-28
Hover-reveal kebab button + favorite star
- the repo list's **rightmost column** now shows a **`⋮` kebab button on the hovered row** — click
  it to open that repo's menu (right-click is eaten by some terminals, so this is the mouse way in).
- an **un-favorited** repo's **☆** now appears **only while its row is hovered** (a favorited repo
  still always shows **★**), so the column isn't a wall of empty stars.

## v2.98.0 — 2026-06-28
Columns dropdowns: select/deselect-all + reset buttons
- every **columns** dropdown (list, repo-page, stashes) now has a footer below its items: a divider,
  a dynamic **`* select all` / `* deselect all`** button (label follows the current selection), and a
  **`0 reset`** button (back to defaults). Keyboard (`*` / `0`), clickable, and hover-highlighted.
- note: the **pulled** / **changed** columns stay dim + inert until a pull actually lands a delta
  this session (they'd be empty otherwise — the same auto-hide behavior as worktrees/branches/
  stashes). **`* select all`** force-enables them anyway if you want the columns shown regardless.

## v2.97.0 — 2026-06-28
Favorites fixed + sortable + filterable; "Last pull" summary pinned to the list footer
- **fixed** the favorite (★) column — it was keyed by `rel_path` while favorites are stored by
  absolute path, so the star never reflected a toggle. It now toggles correctly (click the star,
  `b`, or the kebab).
- **sort by favorite** — favorited repos first (the `s sort ▾` dropdown's *favorite* row, or click
  the ★ column header).
- **filter to favorites** — a *favorites* row in the `f status ▾` dropdown.
- **kebab menu** gains a **★ Favorite / Unfavorite** item.
- the list's **Result** summary row is now **"Last pull"** — a compact one-liner
  (`✓ Last pull · N · …` with per-status counts) **pinned to the bottom of the list pane** (with its
  divider), so it stays visible while the repo list scrolls instead of scrolling away.

## v2.96.0 — 2026-06-28
Kebab menu: "Checkout branch…" with a filterable branch picker
- the kebab (`⋮`) menu's top item is now **Checkout branch…**, opening a picker of local + remote
  branches (after a best-effort `git fetch`), with a **live substring filter** (type to narrow),
  `↑↓` to move, `Enter`/click to check out.
- a **clean** working tree switches immediately; a **dirty** one pops a confirmation describing the
  uncommitted-change count + the risk (non-conflicting changes carry over; git refuses overwrites).

## v2.95.0 — 2026-06-28
Kebab (`⋮`) row menu with a state-aware cleanup prompt
- press **`.`** (or right-click a repo) to open a per-repo kebab menu of state-aware actions.
- **Copy cleanup prompt** copies a prompt with every repo fact already embedded (branch,
  ahead/behind, working tree, stash/branch/worktree counts, open PR) plus a tailored cleanup
  checklist — so an agent doesn't re-run `git`/`gh` to discover the situation. A **`[ ] include
  cd … && claude '…'`** checkbox wraps it as a ready-to-run session command; hovering the item
  shows a **live preview** of exactly what will be copied.
- the menu also surfaces Run agent · lazygit · diff · refetch · open remote (each with its hotkey).
- (next: a "Checkout branch…" item with a filterable fetch+local branch picker.)

## v2.94.0 — 2026-06-28
Commit graph on the Commits tab (GitKraken-style, truecolor lanes)
- the Commits tab now draws a **commit graph** left of each row: 24-bit truecolor branch lanes with
  `●` nodes (`◆` for merge commits) and `│` continuations, so branches and merges read as parallel
  colored lanes. One row per commit (still selectable/clickable; `Enter` opens its diff).
- `git log` now also reads each commit's parents (`%p`) to lay out the lanes.

## v2.93.0 — 2026-06-28
Stashes tab gets its own column selector
- the **Stashes** tab now has a **`t cols ▾`** dropdown to toggle its **age** and **stats** columns
  (the `stash@{N}` ref and message always show); hiding one gives the message its width. Persisted.

## v2.92.0 — 2026-06-28
Settings: changed-row effect is one radio; dropped the "Repo page" row
- **Changed-row flash** + **Changed-row highlight** (two booleans that looked identical and could
  both be on) are now a single **Changed-row effect** radio: **off · flash · highlight**.
- removed the **Layout → Repo page** (restored/maximized) settings row — the repo page is still
  maximized/restored with `m`, the pane buttons, or `b`; it didn't need a settings toggle.

## v2.91.1 — 2026-06-28
Fix: changelog / What's New bullets re-flow and hang-indent correctly
- the changelog is hard-wrapped in the source, so a bullet that spanned several lines rendered each
  continuation as its own dim, flush-left line (a mid-sentence break, e.g. "stats," then "and" on a
  new line, misaligned under the `-`). Continuation lines are now coalesced into their bullet and
  re-flowed to the modal width, with continuations hang-indented under the bullet text.

## v2.91.0 — 2026-06-28
Stash creation time on the repo page
- the **Stashes** tab now has an **age** column ("3 days ago") between the ref and the change stats,
  and the bottom info panel shows a **created** row for the selected stash — both in relative
  "time ago" format, read from the stash commit's committer date (`git stash list --format=%cr`).

## v2.90.1 — 2026-06-28
Fix: reset-to-defaults matches the new defaults
- the settings **Reset** had its own hardcoded default table that wasn't updated for v2.90.0, so it
  left Panel padding / Hover / Grouping / splitter / tabs off-default and the confirmation wrongly
  said "already at defaults". Reset and its diff now mirror the real field defaults; a new test
  asserts a reset leaves nothing differing.

## v2.90.0 — 2026-06-28
New defaults + `v` overrides auto repo-page tabs
- changed defaults (fresh installs / unset fields): **Hover effects on**, **Panel padding on**,
  **Grouping on**, **Pane splitter → on-hover** (no dedicated lane), **Repo page tabs → auto**.
- with **Repo page tabs: auto**, pressing **`v`** now flips the current view tabbed⇄flat as a
  session override — it no longer clobbers the persisted `auto` preference to `off`.

## v2.89.1 — 2026-06-28
Fix: a no-op refetch no longer flashes the changed-row attention indicator
- the refetch re-queues each repo (transient `Queued`) before diffing, so the "status changed"
  flash compared `Queued` → `UpToDate` and fired on **every** refetch even when nothing changed.
  Now the real pre-refetch status is captured first and only a genuine terminal→terminal change
  flashes; a transient baseline never counts.

## v2.89.0 — 2026-06-28
PR viewer modal — structured, collapsible, searchable; loading skeleton
- the description and every review/comment are now **collapsible sections** (click the `▾`/`▸`
  header or any header to fold). With more than one comment a **`[− collapse all]` / `[+ expand
  all]`** control appears (also `z`).
- **`/` searches** across the description and all comments — filters to matching sections, shows a
  live match count, and highlights hits.
- a **loading skeleton** (spinner + shimmer bars) shows while `gh pr view` is in flight.
- the title/number now live only in the modal's title bar — no longer duplicated in the body, which
  starts with a clean meta header (state · `head → base` · @author · date · `+adds −dels` · labels).
- **raw HTML tags are stripped** (`<details>`/`<summary>`/… in bot comments) so bodies read cleanly.
- fixed: hovering inside the PR modal no longer highlights panes behind it.

## v2.88.0 — 2026-06-28
PR viewer modal — read a pull request in full without leaving the terminal
- click the **Pull Request** link (info panel), the repo page's HEAD-branch PR, or a list **PRs**
  cell to open a centered modal (diff-modal dimensions) that loads the full PR via `gh pr view`.
- shows title, state, base ← head, author, date, `+adds −dels`, labels, the description, and every
  **review and comment** — all rendered as markdown (headings, bullets, blockquotes, rules, links).
- `j`/`k`·`↑`/`↓` scroll · `g`/`G` top/bottom · `PgUp`/`PgDn` page · `o` open in browser · `esc`/`q`
  close · mouse wheel scrolls · drag the scrollbar · click `[x]` or outside to close.
- the info panel's **Pull Request** value opens the modal; a trailing **↗** button beside it opens
  the PR on GitHub in your browser.

## v2.87.0 — 2026-06-28
Pull timeout default 30s → 10s; the retry queue now honors it too
- the per-pull timeout default is now **10s** (was 30s) — override with `--timeout S` / `PULL_TIMEOUT`.
- fixed: the **retry queue** was hardcoded to a 30s timeout regardless of `--timeout`; it now uses the configured value like the initial pull and refetch.

## v2.86.3 — 2026-06-28
Pane top-border buttons are clickable again (the splitter no longer steals the click)
- clicking a pane's top-border button — the copy `📋`, the maximize `m▢`, or the repo page's `t cols ▾` / `s sort ▾` — started a **splitter drag** instead: those borders double as resize handles, and only the repo-page `esc`/maximize were excluded from the grab. The splitter grab (the dock boundary, the info/result split, and the list│preview divider) now yields to **every** top-border button, so the buttons sit above the splitter as expected.

## v2.86.2 — 2026-06-28
Opening a merge commit's diff no longer flashes shut
- a **merge commit** showed nothing for `git show --name-status` (the default combined `--cc` diff is empty for a clean merge), so the diff modal opened with an empty file list and immediately closed with a "no changes" toast — a flash. Commit diffs now use **`--first-parent`**, so a merge shows the files it brought in vs its first parent (and normal commits are unaffected).

## v2.86.1 — 2026-06-28
Stashes get their own independent columns (no PR / branch columns)
- the Stashes section/tab rendered through the branch column system, so it showed columns a stash can't have — upstream (`no-up`), ahead/behind, base, and **pr**. Stashes now have their **own independent layout** like commits: `stash@{N}` · change stats (`+A ~M -D  Σtotal`) · message. The `t cols ▾` / `s sort ▾` triggers are hidden on the Stashes tab too (they only apply to the branch-column Branches/Worktrees views).

## v2.86.0 — 2026-06-28
Repo page: collapsible accordion sections in the flat view
- in the flat (stacked) repo page, each section header (Branches / Worktrees / Stashes / Commits) is now a **collapsible accordion** — a `▾`/`▸` chevron shows its state. **Click the header (or press `z`) to collapse/expand** the selected row's section, and **`Z`** expands/collapses them all (the keyboard way back once a collapsed section's rows are hidden). The collapsed set persists; headers hover-highlight, and a collapsed section keeps its header so it's easy to re-open.

## v2.85.0 — 2026-06-28
Maximized repo page: toggle between flat (stacked) and tabbed with `v`
- the full-screen repo page defaulted to a single flat stacked view of every section; **`v` now toggles it between flat and tabbed** (a tab bar with one section at a time), persisted. `v` also flips the tabbed/flat mode while restored. A `v flat`/`v tabbed` hint sits in the footer.

## v2.84.0 — 2026-06-28
Repo-page commits are selectable, hoverable, and clickable — Enter opens the commit's diff
- the **Commits** list is now driven by the same row machinery as branches / worktrees / stashes: each commit is **selectable** (`↑↓`/`j`/`k`), **hoverable**, and **clickable**, and **`Enter` (or a double-click) opens that commit's diff** (`git show <sha>`, with the same file-list + per-file diff view, view-mode toggle, and status filters as the other diffs). Keyboard navigation reaches the Commits list in both the tab and the maximized stacked view.

## v2.83.1 — 2026-06-28
Proper Commits section/tab icon
- the repo page's **Commits** section + tab had a stray `▴` glyph that didn't match the other sections. It now uses a real icon-set glyph — `◉` (Unicode) / `📜` (emoji) — consistent with branches / worktrees / stashes.

## v2.83.0 — 2026-06-28
Repo page: fix the maximized view, the commit columns, and tab-aware cols/sort
- **the maximized repo page was broken** — it showed only the active tab's rows (e.g. just Branches) instead of every section stacked. The active-tab row filter now applies only in tabbed mode, and the **Commits section renders stacked under its own header** in the maximized single view too.
- **the Commits list's author column now grows to the longest name** (it was truncated to ~17 chars), and the **subject fills the remaining width**.
- **the `t cols ▾` / `s sort ▾` triggers are now hidden on the Commits tab** — its `sha·date·author·subject` layout doesn't use the branch columns/sorts (the dropdown was showing irrelevant branch columns there). They stay on Branches / Worktrees / Stashes and in the maximized view.
- the repo-page top border now separates its items with `·` (`t cols ▾ · s sort ▾ · m▢ · esc✕`), and the redundant `t cols` / `m maximize` chips were removed from the footer (they live on the top border).

## v2.82.1 — 2026-06-28
Accordion: ←-collapse always lands a visible focus; clearer focused-header band
- collapsing a section with `←` while focused on one of its **rows** used to leave the selection on a now-hidden row, so nothing read as focused — it was unclear what happened. The focus now moves to the section **header** on collapse, so a `←` always lands somewhere visible.
- the focused accordion header is now a **full-width highlight band** (not just tinted text), so it's unmistakable which section is focused.
- reordered the settings footer nav hints to `tab/⇧↑↓ tab · ↑↓ move · ←→ value`.

## v2.82.0 — 2026-06-28
Settings footer is now two rows so every hotkey is visible
- the settings modal's hint footer was a single border row and **truncated the rightmost chips** — `R reset` was cut to `R`, and the `Shift+Tab` / `Shift+↑↓` tab-switch keys weren't shown at all. Split it into two: **navigation** (`↑↓ move · ⇧↑↓/tab tab · ←→ value`, or `fold` in accordion) on the row just inside the bottom border, and **actions** (`space/enter toggle · v <layout> view · R reset · esc close`) on the border. Both rows stay fully clickable.

## v2.81.1 — 2026-06-28
Settings wheel scrolls the container (web-app style), not the selection
- the mouse wheel now scrolls the settings view **freely without moving the selection** (2.81.0 moved the selection). Scroll away from the selected setting and it stays put off-screen; then any keyboard command (`↑↓` / `←→` / `space` / `enter` / `tab` / …) snaps the view back to it — exactly like a focused control in a web app. (Mirrors the changelog modal's decoupled scroll.)

## v2.81.0 — 2026-06-28
Repo-page actions keep the list status in sync; settings wheel + scrollbar-gutter fixes
- **the list status now stays in sync after a repo-page action** — checking out a branch (or deleting one / discarding / dropping a stash / removing a worktree) re-derives the repo's status from fresh local facts, so a stale pull result (e.g. a `ref gone` whose deleted branch you just switched away from) no longer lingers. The status now reflects the current branch: `dirty` / `no upstream` / `up-to-date` (the Δ column still shows any ahead/behind).
- **the mouse wheel now scrolls the settings modal** (it did nothing before).
- **the settings flat / search layouts now reserve a scrollbar gutter** so the widest row (Background's `terminal` option) is no longer clipped under the scrollbar when the list overflows.
- **emoji-mode maximize/restore glyphs are now `🗖`/`🗗`** (the literal window glyphs) instead of `🔳`/`🔲`.

## v2.80.0 — 2026-06-28
Settings: consistent flat-view spacing, and tabbed-view ←→ changes the value
- **flat layout spacing is now consistent** — it added an extra blank line after the search box only when panel padding was off (so 2 blanks off vs 1 on). Removed it: there's one spacer in both modes, and the **Panel padding** toggle now changes only the uniform border inset (off = flush, on = 1 cell), as everywhere else.
- **tabbed layout: `←`/`→` now change the selected setting's value** (like the flat layout). Switching tabs moves to **`Tab` / `Shift+Tab`** (or **`Shift+↑`/`Shift+↓`**). The footer hint reads `tab tab · ←→ value` accordingly.

## v2.79.3 — 2026-06-28
Harden the settings model so labels/tooltips can't desync again (internal)
- each setting's **label + tooltip are now co-located in one `SETTINGS` source-of-truth table** (they were parallel index-keyed lists, which is how the alphabetical reorder silently pointed every tooltip at the wrong setting). The label list + tip lookup now derive from it.
- added an **invariant test** that fails the build if labels, tips, option counts, section counts, or the read/write dispatch ever drift out of sync — so adding or reordering a setting stays solid.

## v2.79.2 — 2026-06-28
Fix every settings tooltip (stale indices), and scope "Merged PRs" to the list column
- **the settings tooltips were all wrong** in every layout (tabbed / accordion / flat) — the tip table was keyed by the *pre-alphabetical-reorder* row indices, so e.g. hovering "Merged PRs" showed the changed-row-flash description. Re-keyed the whole table to the current `SETTINGS_LABELS` order and added the missing Agent / Merged PRs tips.
- **the "Merged PRs" setting now gates only the dense list PR column.** The single-repo detail views (info panel, repo page HEAD row + info panel) always show the current branch's PR when available, in any state — so a merged PR (e.g. the one that just deleted your upstream) always shows where you're looking at one repo. Detection was never the issue; `gh` finds merged PRs fine.

## v2.79.1 — 2026-06-28
Info-panel copy glyph in emoji mode, no PR "checking…" flicker, clearer reset tooltip
- **the info panel's inline copy buttons now follow the icon set** — they showed a hardcoded Unicode `⧉` even in emoji mode; they render `📋` now, and the click region + spacing are width-aware so the 2-cell emoji stays aligned.
- **removed the in-flight "checking…" Pull Request placeholder** in the info panel — the PR line now appears only once a PR actually resolves, instead of flipping a placeholder that shifted the rows beneath it (a layout shift). Repos with no PR never render the line.
- **the footer's active `{status}` reset tag tooltip now reads "Clear the status filter"** (was the generic "Filter by this status" — it resets the filter, it doesn't set one).

## v2.79.0 — 2026-06-28
Declutter the footer, rename the filter trigger, and fix the horizontal splitter grip
- the status-filter is now owned entirely by the list-header trigger — **removed the redundant `f by-status` chip from the status-bar footer** (the active `{status}` reset tag still appears there while a filter is on). Dropped the now-dead `FilterLeader` command.
- **renamed the header trigger to a single word: `f status ⟪failed⟫ ▾`** (was `f by-status`).
- **fixed the horizontal pane-splitter hover grip** — it used to draw a bottom-hugging `▁` run on the lower pane's title row, clobbering the title text. It now draws a short heavy `━` handle on the upper pane's clean bottom border (no text to cover), and the vertical grip matches it with a heavy `┃` (was a thin `▏`).

## v2.78.1 — 2026-06-28
Bring the `f by-status` trigger to full parity with `s sort` / `t cols`
- the active filter now **rides on the trigger** like the sort tag does — `f by-status ⟪failed⟫ ▾` (and plain `f by-status ▾` when unfiltered), so you can see the active filter on the header, not only in the footer `{status}` tag.
- the trigger gained the **`▾` dropdown indicator** it was missing, matching `s sort … ▾` / `t cols ▾`.
- the trigger now **highlights on hover** like the other two — `list_filter_click` was registered as clickable but never added to `apply_hover`, so it was clickable-but-dead on hover.

## v2.78.0 — 2026-06-27
List header filter trigger is now a status-filter **dropdown** (`f by-status`), not the name filter
- the header's filter trigger is the **status filter** (`f by-status · s sort ▾ · t cols ▾`), not the name filter — pressing `f`, clicking the `f by-status` trigger, or clicking the footer hint opens a **dropdown** matching `s sort` / `t cols`, with radio rows `all` / `updated` / `up-to-date` / `skipped` / `failed` / `issues`. Pick by hover+click, `↑↓`+Enter, or the row's mnemonic letter; the active filter shows a `●` marker.
- replaces the old two-key `f` leader chord (`Leader::Filter` is gone). The `/` name filter stays in the status-bar footer (with its active `[needle]` tag) and the footer's `{status}` reset tag are both untouched.

## v2.77.0 — 2026-06-27
List header gains a filter trigger (filter · sort · columns); declutter the log title
- **the list pane's top border now leads with a `/ filter` trigger**, in the order `filter · sort · columns` with `·` separators (was `t cols ▾ s sort ▾`). Clicking `/ filter` starts the name filter; sort/columns still open their dropdowns. The footer keeps its `/ filter` (with the active `[needle]` tag) as the live filter indicator.
- **the command-log pane title drops the `pid —` noise** — the git subprocess PID only appears (`· pid N`) while a pull is actually running; a settled repo's title is now just `Command log · <repo> · <status> · <elapsed>`.
- **a dim `·` separates the copy and maximize buttons** on the result pane's top border (`📋 · m▢`), matching the header dots.

## v2.76.1 — 2026-06-27
Settings: flat-layout scrollbar, stable accordion height, fixed theme/Design-tab rows
- **fix: the flat settings layout clipped overflow with no scrollbar** — on a short terminal the bottom rows (Theming → Tooltips) were cut off and unreachable. Flat now scrolls like the accordion: a draggable scrollbar, wheel/`j`/`k` scrolling, and the selected row is kept in view.
- **the accordion settings modal no longer resizes when you fold/unfold sections** (a layout shift). It now sizes to the fully-expanded item count (capped at the available height), so its outer size is stable across collapse state while the content folds within it.
- **fix: a row-index regression from the alphabetical settings reorder** — the Theme row's auto-detect underline landed on the wrong row, and the help **Design** tab's Theme/Background/Contrast/Selection radios were dispatching to Panel-padding/Borders/Pane-splitter/Repo-page-tabs. Both now use the correct rows.

## v2.76.0 — 2026-06-27
Window-control buttons (close/maximize), icon-aware glyphs, copy-hotspot fix, alphabetical settings
- **the repo page's `[esc back]` is now an `esc✕` close button** styled like the maximize/restore controls — a clean window-control row (`m▢`/`m▣` maximize + `esc✕` close).
- **the window-control + copy glyphs follow the icon set:** Unicode mode uses `▢`/`▣`/`✕`/`⧉`, emoji mode uses 🔳/🔲/❌/📋. They're measured by display width, so the 2-cell emoji glyphs lay out and hit-test correctly.
- **fix: the result-pane copy button's hover/click hotspot was offset and the wrong width.** It's now exactly the glyph's display width and sits squarely on the glyph (no stray cell to its left), and a 2-column gap separates it from the maximize button.
- **the settings sections are now alphabetical:** Agent · Interaction · Layout · Lists · Pull requests · Sync · Theming · Tooltips.

## v2.75.0 — 2026-06-27
Every pane is maximizeable · dedicated splitter lanes · docked-pane hover fix · scrollbar hardening
- **every pane maximizes now, not just the repo page.** `m` (or the new `m▢`/`m▣` button on each pane's top border) maximizes/restores the focused pane — list `[1]`, info `[2]`, result `[3]`, or repo page `[4]` — and `1`/`2`/`3`/`4` swap which pane fills the screen while one is maximized. Unified behind a single `maximized: Option<Pane>`; only the repo page's maximize stays sticky (persisted), the others are session-only. **Favorite toggle moved off `m` → `b`, and favorites-first off `M` → `B`** to free `m` for maximize across every pane.
- **fix: the docked repo page killed hover effects on the other panes.** With panel `[4]` docked, hovering a list row / info link / result was dead because `apply_hover` only inspected the repo-page regions. Hover now follows the cursor across whichever panes are visible (independent of focus), gated by which pane is maximized so stale geometry can't false-match.
- **pane splitters are now a setting (Settings → Layout → Pane splitter).** **Dedicated** (default) reserves a visible 1-cell lane between panes — a column for list|preview, a row for the dock and info/result splits — filled with a persistent `▒` grip; **on hover** keeps the panes flush and shows only a thin grip (`▏` vertical, `▁` horizontal) under the cursor at a hotspot. (Replaces the old on/off "Splitter" toggle.)
- **scrollbar hardening:** registering a scrollbar's draggable hit is now folded into `render_scrollbar`, so a scrollbar can't be drawn without being draggable. This fixed two more decorative scrollbars found in the audit — the **build-info modal** and the **version picker** — and makes the whole class of bug impossible.

## v2.74.1 — 2026-06-27
Mouse fixes: list scrollbar drag, info-pane scrolling, splitters under modals
- **fix: dragging the repo-list scrollbar started a splitter drag instead of scrolling.** The list scrollbar was decorative — it registered no draggable region, so a mousedown on it (it sits on the list's right edge, inside the pane divider's grab band) was claimed by the splitter. The list now registers a real scrollbar hit (like the preview/diff/repo-page do), and the scrollbar grab is tested before the divider, so a grab on it scrolls the list and highlights while dragged.
- **fix: the info pane ([2]) couldn't scroll.** Its scrollbar was hardcoded to offset 0 with no draggable region, so when the info content overflowed (long PR titles, many worktrees/stashes) the bottom was clipped and unreachable — the wheel scrolled the log below instead. The info pane now scrolls by wheel (when the cursor is over it) and by dragging its scrollbar, with a per-repo offset.
- **fix: clicks leaked through an open modal to the layout splitters.** With Settings (or any modal/overlay) open over the docked repo page, a mousedown on the dock boundary or the info/result split started a resize drag instead of being absorbed by the modal. Splitter grabs are now suppressed while any overlay is up.

## v2.74.0 — 2026-06-27
Repo page (panel [4]): pane switching, docked modals, single focus-aware footer
- **`1`/`2`/`3`/`4` now switch panels from inside the repo page** — it was a focus trap (the keys were swallowed before the main-view handler ran). From a maximized page, `1`/`2`/`3` restore the panel layout first so the target panel is visible; `4` keeps the page focused.
- **fix: diff / copy / base-picker modals never appeared when opened from the docked repo page** (double-click or Enter/Space on a stash/dirty row, `y`, `b`). They opened in state but were never drawn — only the maximized page's render path drew them. The docked path now draws them too.
- **the footer is single and focus-aware** — instead of showing the repo-page keys *and* the main-view footer at once, the status bar now shows only the focused panel's keys: repo-page keys while panel `[4]` is focused, the main-view footer while a list panel is focused. The repo page keeps its own border footer only when maximized.

## v2.73.1 — 2026-06-26
Changelog / version-picker: the wheel scrolls freely again
- **fix: you couldn't scroll up past the selected release** in the Changelog / What's New / version-picker modal — every render snapped the selected (or just-expanded) release back into view, fighting the wheel. Scroll is now decoupled from selection (web-app style, like the main list): the wheel and PageUp/Down scroll freely, and only a keyboard selection move or an expand/collapse snaps the selection into view (once).

## v2.73.0 — 2026-06-26
Changelog / What's New / version-picker: markdown, maximize, hover, accordion
- **release notes render markdown** — `**bold**` and `` `code` `` show styled (markers gone) instead of literally, via one shared renderer used by the Changelog, What's New, and version-picker modals so they look identical.
- **`m` maximizes ⇄ restores** the modal — or click the `[m maximize]` / `[m restore]` title-bar button. Maximized fills ~90% of the viewport. Same control as the help modal.
- **the version picker's `[pin]` buttons and release rows now highlight on hover** (they were clickable but inert on hover before).
- **click a release header in the picker to expand it** (accordion) — not just the keyboard selection.
- **pin a version straight from the Changelog / What's New dialog** with `p` (or the new `p pin version` footer chip), not only from Build info.

## v2.72.1 — 2026-06-26
Changelog / What's New / version-picker notes now wrap
- **long release notes wrap instead of clipping at the right edge.** Note text in the Changelog, What's New, and version-picker modals is word-wrapped to the modal width (bullets get a hanging indent so continuation lines align under the text). Wrapping happens before layout, so the scrollbar and `j`/`k` scrolling stay accurate.

## v2.72.0 — 2026-06-26
pin any released version from the build-info dialog
- **new version picker.** Build info (click `built … ago`) now has a `p pin version` action (key + clickable footer chip) that opens a picker of every published release, fetched live from GitHub. Select one and polygit downloads that release's binary for your platform, installs it over itself, and reloads into it.
- **by default it only offers versions that themselves have the picker** (v2.72.0 and up) — the floor — so you can always switch again from inside the app and never strand yourself on a build with no switcher. The selected release expands its changelog notes inline.
- press `a` to **show older versions** too; they're dimmed and tagged `no in-app switch`. Pinning one pops a warning confirm explaining you won't be able to switch versions in-app afterward, with a **click-to-copy command** to reinstall the latest build (the only way back).
- self-install covers Linux and macOS (the picker is hidden on native Windows; use WSL).

## v2.71.1 — 2026-06-26
Build-info modal: scroll the settings tree with the mouse wheel
- **the Build-info settings-tree scrollbar no longer gets stuck.** Its scroll was slaved to the selected row, so the mouse wheel could never move past the selection — it stalled mid-list while the rest of the tree was unreachable. Scroll is now decoupled from selection, matching the main list: plain wheel scrolls the preview freely (web-app style), Alt+wheel and `j`/`k`/PageUp/Down/`g`/`G` move the selection and keep it in view. The thumb now tracks real scroll position.

## v2.71.0 — 2026-06-26
releases auto-publish on version bump
- **prebuilt binaries now publish automatically.** The release workflow triggers on any push to `main` that changes `Cargo.toml`: it reads the version, and if `vX.Y.Z` isn't released yet, creates the tag and builds + attaches binaries for all five targets. No more manual `git tag` step — so `curl … | install.sh` (which fetches `releases/latest`) always tracks the newest version instead of going stale.

## v2.70.0 — 2026-06-24
info panel whole-line copy, per-worktree copy, repo-page trigger hover, footer tidy
- **The Path and (non-link) Branch rows' value copies on click** — the value plus a trailing 2-char `⧉` are one click/hover region (the field label is excluded), and the `⧉` is a standout magenta so the copy affordance reads at a glance.
- **Worktrees lists one branch per line, each its own copyable line** — so you copy a single worktree branch, not all of them concatenated. The Branch name is also newly copyable; when it's a clickable remote link, clicking the name opens it and a separate, dim `⧉` copies the name (copy stays a distinct, secondary operation there).
- The Path row no longer expands-on-click (it copies instead); a long path still left-truncates to keep the filename tail. Commit-subject expand is unchanged.
- **The repo page's `t cols ▾` / `s sort ▾` triggers now get the same hover highlight** as the main list header's chips (shared hover machinery).
- **Footer fold hint reads `-/+ all`** (was `[-/][+ all]`) — `-` collapses all, `+ all` expands all, each its own click target.

## v2.69.0 — 2026-06-24
smarter PR + branch links, a "Merged PRs" setting, and a pull-hang fix
- **fix: a repo could pull indefinitely** (stuck "running", clock ticking past the timeout). A pull that needs credentials spawns a long-lived `git credential-cache--daemon` that inherits git's stdout/stderr, so the pipes never hit EOF and the output readers blocked forever — even after git itself exited or was killed on timeout. The readers now drain with a brief grace then abort, in both the TUI and `--no-tui` paths
- the **Branch** field is only a link when the branch is actually on the remote — a no-upstream / "ref gone" branch (its PR merged and the remote branch deleted) renders as plain text instead of a link that 404s
- **PR detection now finds merged & closed PRs**, not just open ones — the `gh` lookup queries all states (preferring an open one). The info panel + repo page show the PR's lifecycle state and the `pr` column is colored by it (green=open, magenta=merged, gray=closed)
- new **Settings → Pull requests → Merged PRs** toggle (off by default): merged/closed PRs only show when it's on, so by default you still see open PRs only. Detection always finds every state — the toggle gates display, so flipping it is instant (no re-query)
- the `t cols ▾` / `s sort ▾` header trigger chips now get a hover background, and the `pr` column's leading separator space is no longer underlined

## v2.68.0 — 2026-06-24
native Windows support + a configurable AI coding agent
- builds and runs natively on Windows (`x86_64-pc-windows-msvc`) — no WSL required; the release workflow now ships a Windows `.zip`
- platform-split the Unix-only bits: process reload (`execvp` on Unix, spawn+wait on Windows), `open_url` (adds a `cmd /C start` fallback), and the `c` launcher (interactive `bash` on Unix, `pwsh` on Windows)
- pulls are now bounded by tokio's timer instead of the GNU `timeout` coreutil — cross-platform, and one less external dependency on Linux too
- new Settings → **Agent** section: pick which AI coding agent the `c` key launches — claude / codex / gemini — plus a "Skip permissions" toggle that appends the agent's bypass-all-prompts flag. `PULL_CLAUDE_CMD` still overrides the command verbatim

## v2.67.0 — 2026-06-23
columns/sort: the header dropdowns are now the single picker UI
- `t` / `s` now open the `t cols ▾` / `s sort ▾` header dropdown (the footer leader-menus for columns/sort are gone)
- each dropdown row shows its mnemonic letter — pick by mouse-hover + click, `↑↓` + Enter, or pressing the letter; columns multi-toggle (stay open), sort closes on pick
- dropdowns now hover-highlight, right-align under their trigger, and open with nothing pre-selected (no more stray highlight on the first row)
- unavailable columns render dim + inert in the dropdown
- the active sort + direction (`⟪col ▲⟫`) now rides on the `s sort` trigger; the redundant footer `s sort · t cols` hint is removed
- the repo page gets the same treatment (`t`/`s` open its dropdowns; the old bottom column-toggle strip is gone)

## v2.66.0 — 2026-06-20
tooltip/repo-page fixes: sticky [x], textual maximize, single-view maximize, row hover, tri-state All-tooltips
- the column-header tooltip now stays alive while the cursor moves from the
  header into the popup (keyed off the popup OR its anchor), so the `[x]`
  hide-column button is reachable instead of vanishing mid-move.
- the repo page's maximize/restore control is now a textual `[m maximize]` /
  `[m restore]` button (its `m` key mnemonic), replacing the ▢/▣ icon.
- when maximized the repo page is a single view — every section stacked under its
  header, no tab bar (tabs apply only while restored).
- repo-page rows (branches/worktrees/stashes) now hover-highlight like the list.
- the Tooltips "All tooltips" control is now a bulk on/off over the per-area
  flags: toggling it sets them all; changing an individual area makes it mixed
  (neither radio selected). Replaces the old separate master gate.

## v2.65.0 — 2026-06-20
diff modal: raw / unified / split views with syntax highlighting
- `v` cycles the diff render style (persisted, shown in the footer): raw keeps
  git's own colored output; unified and split are structured, line-numbered,
  syntax-highlighted GitHub-PR-style views (split = old left / new right) with a
  faint green/red wash on added/removed lines.
- new src/diffview.rs: ANSI-stripping unified-diff parser (line numbers + split
  pairing) and a lightweight, language-aware syntax highlighter keyed off the
  file extension; palette gains diff_add_bg/diff_del_bg.

## v2.64.0 — 2026-06-20
CLI builder overhaul: source-driven flags, checkboxes, multiline clickable command
- source-driven flag list now includes the missing `-w/--workspace` (and `-j/--jobs`
  short forms); `f` swaps a flag's short ⇄ long form.
- every flag (value flags included) is a checkbox: Space toggles, Enter edits a value
  (typed values auto-apply and check the flag on).
- child flags (e.g. `--no-recursive` under `--depth`, `--profile-out` under `--profile`)
  disable + dim and are removed when their parent is unchecked (generic cascade).
- help-text display is a persisted button-group: always / on hover / never (`h` cycles,
  chips clickable); default on hover.
- the built command is an aligned, multiline `polygit … \` preview whose tokens are
  clickable to remove that flag (hover highlights its row, with a "click to remove"
  tooltip); the whole command stays clickable to copy.

## v2.63.2 — 2026-06-20
help modal: switching to About no longer forgets the last useful tab
- the persisted/reopened help tab is now the last non-About tab; opening About
  (credits/links) shows it but leaves the remembered tab untouched.

## v2.63.1 — 2026-06-20
dim the j/k scroll hint when nothing overflows
- the build-info and changelog modals' scroll hint now renders disabled (dim +
  inert) when the content fits the viewport (no scrollbar), via a new
  `footer_chip_state` helper.

## v2.63.0 — 2026-06-20
build-info settings preview is now a collapsible structural-data tree
- the state.json preview became a format-agnostic tree viewer (new src/treeview.rs
  DataNode model; JSON is the first adapter, YAML/TOML/etc. only need their own).
- objects and arrays collapse by default, each showing its child count in a faint
  `{N}` / `[N]`; scalars are typed-colored.
- keyboard + mouse driven: j/k move, ←/→ collapse/expand, space/enter fold the
  selected node, -/+ fold/unfold all, g/G jump; clicking a node folds it, plus
  `[- fold all]` / `[+ unfold all]` buttons on the card header.
- falls back to the raw highlighted lines when the file isn't valid JSON.

## v2.62.0 — 2026-06-20
reset-plan colored/aligned diff; design-system confirm preview copy
- the reset confirmation now renders each `Label: current → default` as an aligned
  column with the new value highlighted green and the old one dimmed (the dialog
  widens to fit, no clipping).
- the design-system confirm preview gets its own unique copy describing the
  shared dialog instead of mimicking the reset wording.

## v2.61.0 — 2026-06-20
changelog + What's New modal; clickable version tag
- the `vX.Y.Z` status-bar tag now opens a Changelog modal: every release as a
  collapsible accordion (header `▸ vX.Y.Z · <time-ago>`), the latest two expanded.
  j/k select a release, space/enter folds it, headers are clickable; the [x] /
  outside / esc close it.
- after reloading into a newer build, a "What's New" modal pops automatically,
  listing every release since the version last run (all expanded).
- notes come from an embedded CHANGELOG.md (parsed at runtime); the last-run
  version is persisted as `last_seen_version`.

## v2.60.0 — 2026-06-20
build-info modal: build duration; click-inside no longer closes
- `make build`/`make dev` time the compile and write the seconds to a
  `.polygit.build` sidecar beside the installed binary; the Build info modal
  reads it and shows "Built  <ago>  (took Nm Ms)".
- the modal no longer closes on any click — only the `[x]`, a click outside, or
  `esc`/`r` act, so you can scroll/select the state.json preview inside it.
- added `format_duration` and stored `build_info_area` for the outside-click test.

## v2.59.0 — 2026-06-20
Design System tab: flat ⇄ tabbed (vertical tabs) layout
The Design System showcase grew, so give it the same layout choice as the
settings modal:
- `v` cycles the Design tab between **flat** (every section stacked in one
  scroll, the original) and **tabbed** (a vertical tab column — Theming ·
  Palette · Buttons · List rows · Radios · Dialogs — with the active section's
  content beside it). Persisted as `design_layout`.
- in tabbed mode `[`/`]` move between sections and the vertical tabs are
  clickable (hover-highlighted, active one keeps its solid highlight).
- the content is factored into `design_sections()`; the flat view concatenates
  them under headers, the tabbed view renders one section at a time. Section
  names live in the shared `DESIGN_SECTIONS` const (single source for the tab
  labels and the key-nav count).
- footer shows `v <next> view` on the Design tab (and `[/] section` in tabbed).
Docs (README, usage, keymap.json) updated.

## v2.58.0 — 2026-06-20
confirm dialog reuses footer-chip buttons (hover); design-system preview
- the confirm dialog's yes/no are now the shared footer-chip buttons (cyan key +
  dim label) registered as HintClick regions, so they hover-highlight and a click
  injects `y`/`n` through the exact same path as every other modal footer — no
  bespoke confirm-button styling or hit-testing. Dropped confirm_yes_click /
  confirm_no_click in favor of the generic hint system.
- the [x] close button now hover-highlights too.
- Design System help tab gains a clickable "[preview confirm dialog]" button that
  opens the shared ConfirmDialog live (new ConfirmAction::Preview — a no-op accept
  that just closes), so the dialog component is discoverable and inspectable.

## v2.57.1 — 2026-06-20
accordion settings: width, scrollbar position + mouse drag
- reserve a right-hand scrollbar gutter (+1 col) so the widest row (Background's
  "terminal" chip) is no longer cropped under the scrollbar.
- scrolling back up to the first section header now snaps the view to the very
  top (revealing the collapse-all button), so the scrollbar thumb sits at the
  top — matching "I'm at the top of the list" instead of a stale mid offset.
- the accordion scrollbar is now a registered ScrollHit (new ScrollKind::Settings)
  so it's mouse-draggable; a drag drives the view and moves the keyboard
  selection onto the first visible line so the two stay in sync.

## v2.57.0 — 2026-06-20
Tooltips settings group; radio chips set-only (label cycles)
Settings radios:
- clicking a radio chip now only SETS that value — clicking the already-active
  chip is a no-op (it used to cycle). Only clicking the row label cycles to the
  next value. Applies to the settings modal and the Design System tab radios.
New "Tooltips" settings group:
- a master "All tooltips" toggle plus per-area switches — footer commands,
  column headers, group counts, settings rows, help links. All default on.
- each dwell-tooltip source is gated by the master switch AND its area's flag;
  column-header vs group-count regions are distinguished by a new TooltipArea
  tag on TooltipRegion. Tooltips still require Hover effects (cursor tracking).
- persisted as `tooltips` in state.json (serde-default all-on, so old files and
  partial objects load fine); reset-to-defaults restores every area to on.
Docs (README, usage, keymap.json, columns-and-glyphs) updated to match.

## v2.56.0 — 2026-06-20
keyboard reload/dismiss for the new-build notice
The new-build notice was mouse-only. Add its keyboard counterpart and
surface the keys in the banner itself:
- Ctrl-R reloads into the new build, Ctrl-X dismisses the notice. Both are
  handled at the top of the key match (before the per-view/modal gates) so
  they work from any view or modal, mirroring the always-on mouse handler.
- the banner now reads `↺ new build installed · [^R reload] [^X]` so the keys
  are discoverable without the help modal.
- the build-info modal's notice line now reads "press r (or click [reload])".
CLAUDE.md UI-philosophy section reframed: the app is keyboard-first AND
mouse-first, equally — every interactive element needs both a key binding and
a clickable counterpart, transient/floating overlays included, and a key
should be surfaced in the element itself.
Docs (README, usage, keymap.json) updated to match.

## v2.55.0 — 2026-06-20
accordion settings rework, column-header [x] hide, Design before About
Settings accordion layout:
- j/k (↑/↓) now navigate both section headers and the rows of expanded
  sections; previously only rows were reachable, so lower sections
  (Interaction/Layout) could be clipped off-screen with no way to reach them.
- a focused header has no active child row — it is the header itself that is
  selected (no `>` cursor glyph on headers anymore).
- space/enter (or ←/→) on a focused header folds/unfolds it; clicking a
  header focuses AND toggles it.
- the modal content scrolls to keep the selection visible when it overflows.
- active header uses a readable hover-style fill (dark-on-light) instead of
  an unreadable active color in light mode.
Column-header dwell tooltip:
- an optional column's tooltip now carries a clickable red [x] that hides the
  column outright. The tooltip stays alive while the cursor moves onto it
  (sticky via the captured popup rect) so the [x] is reachable, and the hit
  test runs before the splitter/scrollbar grabs underneath the floating popup.
Help modal:
- the Design tab now precedes About (Hotkeys · CLI · Legend · Design · About).
Docs (README, usage, columns-and-glyphs) updated to match.

## v2.54.0 — 2026-06-20
column/sort dropdown overlays (Phase 6)
Mouse-friendly companions to the t/s leaders (which still work):
- a reusable header dropdown overlay (new src/app/dropdown.rs + render_dropdown):
  a small floating menu anchored under its chip, checkboxes for columns / radios
  for sort, keyboard (arrows/space/enter/esc) + mouse (click item, click-outside
  closes; columns multi-toggle and stay open, sort picks one and closes);
- the list header gains `[cols ▾]` / `[sort ▾]` chips;
- the repo-page title bar gains the same `[cols ▾]` / `[sort ▾]` chips (left of the
  window controls), driving the repo-page columns + sort.
Docs (README, columns guide, keymap.json) updated; +1 test (166 total), clippy
clean. Verified via pty harness (chips render, dropdown opens/picks/closes).

## v2.53.0 — 2026-06-20
CLI builder overhaul (Phase 9)
The help modal's CLI tab (interactive command builder) gets:
- every flag is a checkbox row (value flags check on once a value is set), with the
  help comments aligned in a column;
- `h` toggles the help column on/off;
- parent/child grouping: `--no-recursive` indents under `--depth`, `--profile-out`
  under `--profile` (new `CliFlag.parent`);
- typed values auto-apply live (the command updates as you type; Enter/Esc just
  leave edit mode);
- `-j` → `--jobs` (the long form) in the generated command;
- the assembled command line is clickable to copy (alongside `y` / [ copy ]).
Docs (README, usage) updated; +1 test assertion (165 total), clippy clean.

## v2.52.0 — 2026-06-20
build-info modal enrichment (Phase 8)
The Build info modal (click the `built … ago` status tag) now shows, beyond the
version / build age / latest-build status:
- the running binary's size + watched path,
- the settings file location + how many files live in the config dir,
- a scrollable, JSON-syntax-highlighted preview of state.json (j/k/PgUp/PgDn/wheel;
  a scrollbar tracks position).
Details are snapshotted on open (cheap, off the render path). New pure helpers
`human_size` + `highlight_json_line` (keys cyan, strings green, numbers yellow,
punctuation dim). Docs (README, usage) updated; +2 tests (165 total), clippy clean.

## v2.51.0 — 2026-06-20
repo-page PR column + stash change-stats (Phase 5b)
- PR column (RepoPageColumn::PullRequest, `t r`): the current branch's open PR
  shows as a clickable `#N` on the HEAD row (opens the PR), blank elsewhere.
  Available only when the repo has an open PR; default on.
- Stash rows now flow through the same change-stat columns as branches
  (added/modified/deleted/total), loaded lazily via `git stash show --name-status`
  (new `git::stash_diff_stats` + `worker::run_stash_stats`, mirroring the branch
  stats loader; stats carry across the post-fetch page rebuild).
- `data_cells` gained `dirty: Option`, a `base_clickable` flag, and a `pr` cell;
  stash rows render with blank branch-only columns and a non-clickable base.
Docs (README, repo-page guide, keymap.json) updated; +2 tests (163 total),
clippy clean.

## v2.50.1 — 2026-06-20
split app.rs + render.rs god-files into module dirs
No behavior change. The two 7k-line files are decomposed into `src/app/` and
`src/render/` module directories so no logic file is a 7k god-object:
- src/app/ — mod.rs (use + AppState struct + constructor), types.rs (all
  standalone types/enums/consts), state1/2/3.rs (the AppState impl, split by
  contiguous method groups), tests.rs. Submodules `use super::*`; a few private
  helpers crossing files widened to pub(crate).
- src/render/ — mod.rs (shared helpers + the render/render_widgets dispatcher),
  list.rs, preview.rs, status_bar.rs, help.rs, repo_page.rs, modals.rs, tests.rs.
  Cluster fns widened to pub(crate); mod.rs glob-re-exports each submodule.
Largest source file drops from 7.5k → ~1.2k (logic). 161 tests + clippy clean;
release build + plain-mode smoke verified identical output. Also sanitized a
stale path reference in a comment.

## v2.50.0 — 2026-06-20
reset settings to defaults (with diff confirmation); accordion spacing
- Settings modal: press `R` (or the `R reset` footer chip) to reset every
  settings-modal preference to its default. A confirmation modal lists each
  setting that will change as `Label: current → default` (favorites, workspaces,
  caches, collapsed sets are left untouched). Opening it closes settings so the
  confirm is the sole modal (single-modal invariant), then re-applies + persists.
  Built on a single source for the option labels + defaults (`settings_option_labels`
  / `settings_default_option`), so the reset plan can't drift from the rendered rows.
- Accordion settings layout: a blank line now separates the collapsible sections
  (matching the flat layout) so the headers don't run together.
- ConfirmDialog gains generic `detail_lines` + `detail_title` body fields.
Docs (README, usage, keymap.json) updated; +2 tests (161 total), clippy clean.
Verified via pty harness: the confirm lists the diffs and the accordion spacing.

## v2.49.1 — 2026-06-20
pulled/chg columns no longer flicker on retry
The pulled (⇣) and changed (±) columns were gated on `!all_done || any_pull_result`,
so pressing `R` (retry) flashed them in for the duration of the run and back out
when it settled with nothing pulled (and a retry of a delta repo flickered when the
worker cleared `pull_result` at pull start).
Latch the columns instead: once any pull lands a delta this session (`pulled_seen`),
they stay shown; before that they stay hidden. A retry/refetch that transiently
clears every result no longer toggles their visibility. Runtime-only flag, fresh
per launch. Docs updated; +1 test (160 total), clippy clean.

## v2.49.0 — 2026-06-20
named workspaces (-w / ws picker); default scans cwd, not a saved set
Fix: launching from a directory (or with explicit DIRS) opened a previously
persisted folder set instead of the cwd/args. The old "persistent workspace"
always unioned the saved roots ahead of the CLI dirs, so the cwd was only used
when nothing was saved and explicit paths got buried.
Now a launch is ad-hoc by default — `polygit` scans the cwd, `polygit DIR...`
scans those, and nothing is persisted. Curated folder sets become opt-in named
workspaces:
- `-w/--workspace <NAME>`: open a saved workspace, or (with DIRS) define it as
  those folders; a new name with no DIRS seeds from the cwd. The folder picker
  (`A`) / remove-root (`X`) persist to the active workspace only.
- `ws` subcommand (aliases `workspace`, `workspaces`): interactive picker over
  saved workspaces; `ws ls` (alias `list`) prints them; per-command `--help`.
  Built as an extensible clap subcommand tree for future commands.
- Persistence moves from a single `roots` list to `workspaces: {name -> [roots]}`;
  the legacy `roots` migrates into a `default` workspace on load.
Docs (README, usage, keybindings guide, keymap.json) updated; +3 tests (159 total),
clippy clean. Verified end-to-end via pty harness (cwd/args scan, ws picker,
-w open/create, ws ls, no-TTY fallback).

## v2.48.0 — 2026-06-20
repo page maximize/restore window + 4-pane focus model
Reframe the docked repo page as a Windows-style maximize/restore window,
defaulting to restored, and fix the broken docked interactions.
- Window state: `dock_repo_panel` (default full-screen) → `repo_page_maximized`
  (default restored). A `▢`/`▣` button on the title bar, left of `[esc back]`,
  plus the `m` key toggle it; the state is sticky (persisted) and settable in
  Settings → Layout → "Repo page".
- Fix the dead docked controls: the title bar sits on the resize-divider row, so
  the splitter grab swallowed every click on `[esc back]` and the title. Exclude
  the title-button columns from the boundary drag (new `title_button_hit`) so the
  buttons receive their clicks.
- 4-pane focus model: `preview_focused: bool` → `focus: Pane` over
  `{List, Info, Result, RepoPage}` ([1]/[2]/[3]/[4], stable numbers). Tab/Shift-Tab
  cycle the visible panels, 1-4 jump, click focuses; the focused panel gets the
  bright border. PageUp/Down scroll the result panel when [3] is focused.
- Restored repo page is master-detail: clicks/keys outside the dock route to the
  list, and moving the list selection retargets panel [4] (cheap retarget that
  reuses the cached page). Esc/q closes an open page before quitting.
- Docs (README, usage, repo-page guide, index, keymap.json SSOT) and pure-logic
  tests updated; 157 tests + clippy clean.

## v2.47.0 — 2026-06-20
/ search on every help tab (lazygit-style key+description match on Hotkeys, plain text filter on CLI/Legend/About/Design)

## v2.46.0 — 2026-06-20
settings search box (/ focuses, filters rows across all tabs into a flat list with matched chars highlighted)

## v2.45.0 — 2026-06-20
split the preview into independent info + result/log panels with a draggable boundary (I toggles the result panel; hidden, info fills the pane)

## v2.44.0 — 2026-06-20
Design tab component showcase (buttons · list rows · radios in every state)

## v2.43.0 — 2026-06-20
floating tooltip engine (placement+flip+shift) in tui-pick; column-header tooltips drop below the header; + reusable button/list/radio primitives

## v2.42.1 — 2026-06-20
single-modal invariant (no stacking) + footer commands hover-highlight over modals

## v2.42.0 — 2026-06-19
folder picker overlay (A) to add/remove workspace folders
`A` opens a filesystem folder picker (the tui-pick `picker` widget): breadcrumbs,
fuzzy search, 📁 folders / 📦 git-repos with a "git repo" badge, bookmarks
(^B/^H), parent (←/Backspace), current-path footer. Enter opens a folder or
selects a git repo; ^S selects the current folder. The chosen folder — which may
itself be a single repo — is added as a workspace root, scanned (without
re-pulling the rest), and persisted. `X`/Delete removes the selected repo's (or
folder header's) root: it's dropped from the persisted set and its repos hidden
(the append-only repos vec keeps worker indices valid); re-adding un-hides them.
Picker bookmarks persist in state.json. Crate: picker::{Entry,PickerState,
PickerOutcome,render_picker,read_dir_entries,is_git_repo}. Tests:
remove_root_hides_repos_and_re_add_unhides. docs: README + keymap.json.

## v2.41.0 — 2026-06-19
fzf-style finder overlay (P) over all repos, shared goto-repo history
`P` opens a dedicated fuzzy finder (the tui-pick `finder` widget) over every repo
across all folders: a prompt + live query, a `matched/total` counter, a sort
header, rows showing type + usage-count + the path with matched chars
highlighted, and a keybinding footer. Type to fuzzy-filter, ↑↓/PgUp/PgDn move,
^S cycles the sort (relevance/name/recent/most-used), Enter (or a row click)
jumps the list selection to that repo, Esc/[x]/outside close.
Recent + most-used rank from the SHARED ~/.config/goto-repo/history (the same
usage file goto-repo uses), and each jump appends a visit there — so usage is
common across tools, with goto-repo as the canonical format.
Crate: finder::{FinderRow,FinderState,FinderOutcome,render_finder}. polygit wires
it via open_finder/finder_jump + a hint-key adapter (the crate owns its own
HintClick/HintKey for decoupling). docs: README + keymap.json.

## v2.40.0 — 2026-06-19
fuzzy `/` filter (nucleo subsequence + relevance ranking + highlight)
The inline `/` name filter is now fuzzy: it matches a subsequence (not just a
contiguous substring) via the tui-pick `finder::fuzzy_match` (nucleo), ranks the
results by relevance (best first) while a name filter is active, and underlines
every matched character in the row (runs coalesced). The `@` status filter and
the no-filter column sort are unchanged. Replaces the old find_ci substring
highlighter.
docs: README + keymap.json.

## v2.39.1 — 2026-06-19
Cargo workspace + reusable tui-pick crate skeleton
Convert the repo to a workspace (root stays the polygit bin package; profiles
live at the root and apply to all members) and add `tui-pick/` — a standalone,
theme-agnostic ratatui widget crate other Rust CLIs can depend on:
- modal: centered_rect, cast_shadow, modal_close_button, build_hint_footer,
  footer_chip/sep + HintKey/HintClick (self-contained; no host types)
- style: FinderStyle/PickerStyle with semantic-ANSI defaults (host frame-remap
  themes them)
- ranking: goto-repo-compatible History (~/.config/goto-repo/history,
  epoch\tpath) + SortMode (relevance/name/recent/most-used)
- finder: fuzzy_match (nucleo) returning score + match indices
- picker: filesystem entry reading + git-repo detection
The crate builds standalone (`cargo build -p tui-pick`) with its own tests; no
polygit behavior change yet (consumed from the next phase). Makefile unchanged.

## v2.39.0 — 2026-06-19
multiple folders + persistent workspace (a folder can be a single repo)
polygit now manages a SET of folders instead of one scan dir:
- CLI accepts multiple dirs (`polygit ~/work ~/oss ~/some-repo`); each may itself
  be a git repo (the walker now emits a root that is itself a repo).
- The launch roots are the union of the persisted workspace + any CLI dirs,
  canonicalized + deduped; the union is persisted (state.json `roots`), so a
  no-arg launch restores the saved folders. Empty + nothing saved → cwd.
- Each repo records its discovery root; rel_path is relative to that root, and
  the tree view becomes a forest (one top-level node per root when >1).
- run_discovery spawns one walker per root (merged channel, dedup across
  overlapping roots); plain mode (--no-tui) scans all roots too.
- Favorites are now keyed by absolute path (unambiguous across roots).
- Fixed tree root-membership to use folder-node ownership, not raw rel_path, so
  the forest doesn't duplicate repos (flat + nested).
Tests: multi_root_tree_is_a_forest; existing 147 green. docs: README + usage.

## v2.38.0 — 2026-06-19
wheel scrolls the list view; Alt+wheel moves selection
The plain mouse wheel over the repo list now scrolls the view independently of
the selection (web-app style — the selected repo stays put and may scroll out of
view) instead of moving the selection. Alt+wheel moves the selection like ↑/↓.
Keyboard / Alt+wheel nav scrolls only as far as needed to keep the selection on
screen — so pressing Up with the bottom row selected no longer shifts the view.
Implemented with a manual list_scroll model: render_list drops the scrolled-past
items and renders from there (ratatui's List treats select(None) as select(0) and
would snap the offset back, so ListState::offset can't give a selection-independent
scroll). A render-frame guard scrolls-into-view only when the selection changed.
docs: README + keymap.json; test wheel_scroll_is_independent_of_selection.

## v2.37.0 — 2026-06-19
settings reorg (Lists/Layout), hide folder lines, Result hover, splitter off scrollbar
- Settings sections reorganized: the old "General" is renamed and merged into
  "Layout" (now panel padding + borders + splitter + repo-page tabs + dock +
  branch-check), and a new "Lists" section holds grouping + tree view + a new
  "Hide folder lines" toggle. Contained reindex (Lists keeps 3 rows so the
  Theming/Sync/Interaction blocks keep their indices).
- "Hide folder lines": drop the dim dash-fill leader lines in group / folder /
  ★ Favorites headers (finish_header_line uses blank fill), for a cleaner row.
- The Result / Errors summary rows now get the hover tint like any other row.
- Splitter grip no longer paints over the left pane's scrollbar column: it's a
  single column on the divider (right-pane side) instead of straddling col-1.
docs: README + usage; tests updated for the reindex.

## v2.36.0 — 2026-06-19
scoped fetch on folder/group headers; [-/][+ all] footer hotspots
- With a folder or group header selected, `r` (retry) / `e` (refetch) act only
  on the repos that header covers — a folder's whole subtree (recursively) or a
  group's members — instead of no-op'ing. A repo-row selection still drives the
  single-repo action; `R`/`E` remain the all-repos variants. `r`/`e` stay
  applicable (not dim) when the selected header has an eligible repo.
- Footer fold hint is now two unambiguous bracketed hotspots, `[-/]` (collapse
  all) and `[+ all]` (expand all), replacing the run-together `-/+ all`.
Adds selected_header_repos / selected_header_retryable / selected_header_refetchable;
test scoped_fetch_targets_selected_folder_subtree.
docs: README + keymap.json.

## v2.35.0 — 2026-06-19
repo page distributes width to branch/subject (no fixed truncation)
The repo page now allocates its inner width across the visible columns instead
of using fixed caps: the fixed-width optional columns take their share, then
`branch` (uncapped up to the available space, leaving room for a readable
subject) and `subject` (the remainder) expand to fill what's left. Hiding
columns reclaims that space for the text columns rather than leaving them
truncated at the old 40/50-char limits.
docs: repo-page guide.

## v2.34.0 — 2026-06-19
favorites (mark repos, ★ Favorites pinned section, favorite column)
- `m` toggles the selected repo as a favorite (persisted by relative path)
- `M` (footer "M ★favs", shown once anything is favorited) pins a ★ Favorites
  section to the top of the list; favorited repos move there and out of the
  normal groups/tree/flat body
- optional favorite column (`t f`) shows a clickable ★/☆ star per repo
- favorites + favorites-first persist in state.json
Touches ListRow (new FavoritesHeader), ColumnFlags/Column (Favorite), the
column header + width allocator, per-frame fav_cell_click capture, and the
footer. Tests for the pinned partition and the column toggle.
docs: README + keymap.json.

## v2.33.0 — 2026-06-19
`/` name filter previews the first match live
While typing a name filter, the selection now follows the first matching repo
so Enter jumps straight to it; Esc clears the filter and restores the repo you
were on before opening `/`. Empty filter (just opening `/`) doesn't move the
selection.
- begin/commit/cancel_filter_input + select_first_filtered_row on AppState
- filter_prev_selection remembers the pre-filter repo
- wired into the `/` key, the filter-input keys, and the NameFilter hint toggle
- tests for preview + esc-restore + commit-keeps
docs: README + keymap.json.

## v2.32.0 — 2026-06-19
dwell tooltips for column headers and group/folder count tails
Hovering ~1s (with hover effects on) now explains more of the UI:
- each sortable column title (name/branch/status/↑↓/Δ/age/wt/br/st/pull/chg/pr)
  shows a one-line description
- a group or folder header's right-corner count (e.g. (27)) shows its
  breakdown ("27 repos in group · 3 running · 1 failed")
Implemented via a per-frame hover_tooltips region list (row, cols, text) and
app.tooltip_at(), wired into the existing dwell path alongside the footer and
settings tooltips.
docs: README + columns-and-glyphs guide.

## v2.31.0 — 2026-06-19
accordion settings layout (collapsible sections)
Adds a third settings-modal layout between tabbed and flat. `v` now cycles
tabbed → accordion → flat. The accordion stacks every section under a
collapsible header:
- click a header, or press ←/→ on the selected row's section, to fold/unfold
- a top [- collapse all] / [+ expand all] button folds/unfolds them all
- the folded set persists (collapsed_settings in state.json)
- the section owning the selection keeps a highlighted header even when its
  rows are folded away, and keyboard nav (j/k) skips collapsed rows
docs: README + usage settings section.

## v2.30.0 — 2026-06-19
Icons "hide zeros" setting (hide zero-count cells in Unicode mode)
Adds a "Hide zeros" row under Icons in the Theming settings section. When on,
zero-count column cells render blank instead of a dim 0, in the Unicode icon
set too (emoji mode already always hides them). Under emoji icons the row is
force-selected and rendered dim + inert, since emoji always hides zeros.
- persist.rs: new hide_zero_counts (serde default false)
- count_cell_hidden(emoji, hide_zero, count) gates blank cells
- settings_row_line gains a `disabled` arg (dim + no click regions)
- full settings-row reindex (Theming 6→7 rows; rows 4..20 shift +1) across
  set_setting_option / toggle_selected_setting / settings_active_option /
  settings_tip / settings_tabbed_blank_before / design-tab radios + tests
- docs: README + usage settings list

## v2.29.0 — 2026-06-19
tab labels (CLI/Design + uppercase DESIGN SYSTEM); theme autodetect underline; b→docked; hide-zero info Changes; settings selection/hover group separator

## v2.28.1 — 2026-06-19
dim all pane borders + titles while a modal is open

## v2.28.0 — 2026-06-19
footer commands dim+inert when inapplicable; modal recedes footer except settings/help/quit; q→close in modals

## v2.27.0 — 2026-06-19
clickable ←/→ fold footer buttons; widen e//r//y/ + resize hotspots; fix settings/keyboard/help footer hover

## v2.26.0 — 2026-06-19
move Button hover into Theming (below List selection) with tabbed-view separators; clearer Contrast tooltip

## v2.25.1 — 2026-06-19
break sort ties by name (alphabetical secondary sort, always A→Z)

## v2.25.0 — 2026-06-18
Pull Request column (clickable #N) with a 5-min TTL PR cache

## v2.24.0 — 2026-06-18
Design System help tab (theming radios + live palette swatches)

## v2.23.1 — 2026-06-18
count-zero dim uses faint (survives terminal min-contrast); emoji mode hides zero cells

## v2.23.0 — 2026-06-18
settings click on active chip / row label cycles the value (3-radio wraps)

## v2.22.1 — 2026-06-18
keyboard viewer hover highlights the whole key cell, not just one row

## v2.22.0 — 2026-06-18
clickable hover-styled modal border footers; remove build-info [restart] button; settings single footer

## v2.21.0 — 2026-06-18
split selection into List selection + Button hover (inverted/subtle)

## v2.20.0 — 2026-06-18
open-PR detection (gh), clickable Pull Request row in info panel

## v2.19.0 — 2026-06-18
filter the Hotkeys help tab with /
On the Hotkeys tab, / starts a filter over the keybinding list (type to
narrow; @ prefix matches the keys column; esc clears), shown in the bottom
border. Completes the /+@ filtering clause on the keybindings view too (the
repo-list filter was the primary half).

## v2.18.0 — 2026-06-18
adaptive periodic branch-check (no pull)
New Layout setting "Auto branch-check" (off/auto): when auto, periodically
refreshes every repo's local git facts (no network) on an interval that
scales with the repo count (repos/10 s, clamped 1..60), paused while any
pull runs. Completes the branch-check clause (the u/U hotkey was the manual
half). branch_check_interval_secs is pure + unit-tested.

## v2.17.0 — 2026-06-18
draggable docked-panel splitter (resizable dock)
The docked repo panel's top edge is now a draggable horizontal splitter:
drag it to resize the dock (height persisted as dock_ratio, clamped). A
second resizable boundary alongside the panes divider — the flexible
multi-splitter layout.

## v2.16.0 — 2026-06-18
dock the repo page as a bottom panel
New Layout setting "Dock repo page" + b key + clickable "b dock" footer
hint (Command::ToggleDock): show the open repo page as a docked bottom
panel (the two panes stay visible above) instead of full-screen. Reuses
render_repo_page into a bottom split, so selection/scroll/clicks/tabs all
work there. Persisted.

## v2.15.0 — 2026-06-18
Commits tab on the repo page (read-only)
Add a Commits tab (git log, local-only) alongside Branches/Worktrees/
Stashes. Repo-page tabs get their own RepoTab identity (Branches/Worktrees/
Stashes map to PageRowKind; Commits renders separately, no row-machinery
ripple). CommitInfo + list_commits + RepoPageData.commits loaded in
run_repo_page. First step toward superseding lazygit.

## v2.14.0 — 2026-06-18
tabbed repo page (Branches/Worktrees/Stashes)
New Layout setting "Repo page tabs" (off/auto): auto splits the repo page
into Branches/Worktrees/Stashes tabs when 2+ sections have rows. A clickable
tab bar replaces the section headers; Tab/Shift+Tab or click switch. The
active tab is filtered in repo_page_rows() so selection/nav/clicks all scope
to it consistently.

## v2.13.0 — 2026-06-18
interactive CLI command builder (CLI & Flags tab)
The CLI & Flags help tab is now a builder: each flag is a row you toggle
(boolean) or fill in (value, inline-edited), the constructed polygit command
updates live below the flag list, and y / [ copy ] copies it. Keyboard
(up/down/space/enter/y) + mouse (click row / copy) parity. CliBuilder state
+ CLI_FLAGS catalog + command() are pure and unit-tested.

## v2.12.2 — 2026-06-18
click a pane to focus it
A left-click inside the panes now focuses whichever side it landed in
(same as the 1/2/Tab keys), so the active-pane border follows your click.

## v2.12.1 — 2026-06-18
u/U refresh local git facts without pulling
New keys: u refreshes the selected repo's local facts (branch, ahead/
behind, dirty, stash, branch count) via run_repo_details; U refreshes all.
Network-free (no fetch/pull). Also drop a stale README line about Go/Bun/
bash subcommands (removed in 2.6.0).

## v2.12.0 — 2026-06-18
@-prefix status/attribute filter on the repo list
The name filter (/) now treats a leading @ as a status/attribute filter:
@failed/@updated/@skipped/@queued/... match the status keyword, and
@dirty/@clean/@ahead/@behind match attributes. The filter prompt hints at
it and flips its label in status mode. status_token_matches() is pure +
unit-tested.

## v2.11.2 — 2026-06-18
dim the footer during leader mode, highlight the trigger
When a leader menu (t/f/s/...) is armed, rows 2-3 of the footer (and the
right meta) dim and go inert so only the leader menu stands out; the armed
leaders own trigger (e.g. "f by-status") gets a highlight pill.

## v2.11.1 — 2026-06-18
dim zero-count cells more (explicit blend)
Zero / still-loading count cells (branches/worktrees/stashes) now use an
explicit faint-toward-surface blend instead of DarkGray->faint, so they
actually recede on the normal/soft backgrounds (and in terminal-bg mode).

## v2.11.0 — 2026-06-18
settings tooltips on hover-dwell
Dwelling ~1s over a settings row (or a specific option) shows a one-line
tooltip explaining it, reusing the footer dwell + render_tooltip path.
settings_tip() supplies the text — e.g. the unicode icon option explains
per-type colorization vs fixed-color emoji, and the changed-row rows note
the status column also marks changes.

## v2.10.3 — 2026-06-18
configurable changed-row flash + highlight
Replace the fixed 3x flash with two independent Interaction toggles:
"flash" pulses changed cells (the old behavior), "highlight" holds them
steady (REVERSED) for the attention window. Both gate the single flash_on
used by every flagged-cell style; flash_active() exposes the whole window.
Turn both off to rely on the status text column instead.

## v2.10.2 — 2026-06-18
show/hide splitter setting
Add a Splitter on/off toggle to the Layout settings tab (persisted, default
on). When off, the divider grip is not drawn (panes sit flush); dragging the
boundary to resize still works.

## v2.10.1 — 2026-06-18
show/hide borders setting (new Layout tab)
Add a Layout settings tab with a Borders on/off toggle (persisted, default
on). When off, the two main panes (and the info panel) drop their borders
and reclaim the border cells. pane_borders() centralizes the choice.

## v2.10.0 — 2026-06-18
tabbed settings modal (IDE-style) with flat fallback
Restructure the settings modal into vertical tabs (General / Theming /
Sync / Interaction) shown one at a time, JetBrains-style. ←/→ or Tab
switch tabs, ↑/↓ move within a tab, click a tab to switch. v toggles
back to the flat all-sections-stacked layout (persisted; hint in the
bottom border). SETTINGS_TABS is the single source of truth for tab
names + row counts; settings_row_line shared by both layouts.

## v2.9.1 — 2026-06-18
maximize/restore the help modal
Add a [m maximize] / [m restore] button to the help tab bar (and the m
key): toggles the modal between fit-to-content and ~90% of the viewport,
so long content (links, hotkeys) gets room to breathe. Session-only.

## v2.9.0 — 2026-06-18
help About links: grouped, collapsible, title-only, hover-URL
Rework the About tab links: group them (polygit / lazygit / collapsible
Notes), render titles only (not raw URLs), and reveal the URL on hover —
in the modal bottom-left immediately (browser-style) plus a dwell tooltip
after 1s. Notes expands/collapses on click. hover_tooltip now owns a String
so it can carry dynamic URLs; status_hint drives the bottom-left preview.

## v2.8.0 — 2026-06-18
Selection style setting (blue bar vs subtle tint)
New Theming setting "Selection": blue = solid blue bar + white text (today);
subtle = soft tint that keeps each column's own color (status hue, branch
accent) readable, for the color-coded tables. Applies to the repo list, repo
page, and diff file list from one place. selection_style enum persisted;
derived subtle tones live on Palette. Default blue. Verified: blue = #2563eb
+ white; subtle = soft tint keeping per-column colors.

## v2.7.3 — 2026-06-18
three-state hover (distinct sel+hover, subtle in terminal bg)
Hover used to paint the selected row with the plain hover tint (washing it
out) and, in Terminal-bg mode, fell back to selection_bg so a hovered row
looked identically loud-blue as the selected one. Now derive the tints from
the palette: hover_bg (subtle wash toward base_bg), selection_hover_bg
(darker shade of the selection). New base_bg palette field gives a real RGB
to blend toward even in Terminal mode. apply_hover routes the hovered
*selected* row (main list + diff file list) to the stronger tint. Verified:
rest/hover/selected/sel+hover are four distinct tones in normal and terminal bg.

## v2.7.2 — 2026-06-18
hover no longer tints the whole pane
The scroll track spans the full pane width (for wheel hit-testing), and
apply_hover highlighted the entire track on hover — so moving the cursor
into the preview/command-log pane (or the repo page) shaded the whole
pane, even when it did not overflow. Highlight only the scrollbar column,
and only when the pane actually scrolls. Shared scrollbar_col_hit() closure
replaces the full-track push in all three hover branches.

## v2.7.1 — 2026-06-18
blue/white/bold active-row selection
The selected row now reads as a saturated-blue background with near-white
bold text in every theme (lazygit-style), replacing the muted gray-on-
gray-blue look. selection_bg is a deep blue in all four palettes; new
theme-independent selection_fg (near-white) is forced on the row (main
list via highlight_style, repo page by overriding each span). Verified
white-on-blue + bold in both light and dark via the pyte harness; the
palette remap emits true 24-bit RGB so WT/Warp render identically.

## v2.7.0 — 2026-06-18
fast scroll with modifier + wheel
Hold a modifier while scrolling: Shift jumps 5x the base step, Ctrl/Alt
scrolls a full page. Applies to the list, the preview, and the repo page
(diff modal keeps its existing Shift/Alt file-list scroll). New pure
wheel_step() helper + unit test.

## v2.6.5 — 2026-06-18
never remember the About help tab
Persist HelpTab::Hotkeys instead of About so reopening help lands on a
useful tab, not the credits/links page. Other tabs are still remembered.
Extracted HelpTab::persisted() (pure) + unit test.

## v2.6.4 — 2026-06-18
title the pull pane "Command log"
The right pane's per-repo log view now leads its title with "Command log"
(lazygit-style): [2] Command log · <repo> · <status> · <pid> · <elapsed>.

## v2.6.3 — 2026-06-18
no hover bg on the result summary rows
The Result/Errors summary rows in the left list tinted on hover like any
repo row; under the cursor it read as the whole result pane shading. Skip
the hover-row rect when list_selection_at lands on a summary row (index
>= visible row count), so only real repo/group rows react.

## v2.6.2 — 2026-06-18
no hover bg on the repo page body
The repo-page row-hover highlight painted a full content-width bar under
the cursor; as the mouse moved it read as the whole page tinting. Drop the
body-row tint entirely so only the page's controls (sort headers, toggle
chips, back button, hint footer, scrollbar) stay reactive.

## v2.6.1 — 2026-06-17
archive bash polygit-repos backend
Remove the now-inert `polygit-siblings/polygit-repos` bash script and
its directory (preserved in git history); `src/plain.rs` superseded it.
Update the two remaining CLAUDE.md mentions to point at history rather
than a live path.
Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>

## v2.6.0 — 2026-06-16
remove Go/Bun/bash sibling-backend dispatch
Drop the `polygit go|bun|cli` subcommand forwarding and the
`polygit-siblings/` resolution. The Rust build is now the only
implementation; `polygit-siblings/polygit-repos` is left in place,
inert, pending archival.
- main.rs: remove maybe_dispatch_sibling/sibling_program + the
  after_help sibling block
- render.rs: drop the SUBCOMMANDS help-modal section
- Makefile: `install` is now a plain alias of `build`
- docs/README/CLAUDE.md: remove sibling references; delete the
  Sibling builds reference page and its sidebar entry
Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>

## v2.5.2 — 2026-06-15
make the j/k move hint clickable
The status bar's `j/k move` hint is now clickable: `[j/]` moves the selection
down, `[k move]` moves it up (new NavDown/NavUp commands), matching the rest of
the clickable status-bar chrome.

## v2.5.1 — 2026-06-15
repo-page row hover, splitter grip on hover, no hover while dragging
- Repo-page branch/worktree/stash rows now highlight on hover (like the main
  list), via a stored content area.
- Hovering the splitter brightens its grip handle to cyan (matching the drag
  feedback), in addition to the column tint.
- Suppress hover entirely while dragging the splitter or a scrollbar — the drag
  has its own feedback and a moving highlight under the cursor is just noise.

## v2.5.0 — 2026-06-15
hover tooltips on footer commands
When hover effects are on, dwelling ~1s on a status-bar command shows a small
bordered tooltip above it describing what it does (web-style). The dwell timer
resets when the cursor moves to a different command or off the bar.
Each Command carries a one-line `tooltip()`; the event loop tracks the dwell
(reading the last frame's click regions, which are stable frame-to-frame) and
sets `hover_tooltip`, which `render_tooltip` draws as a popup above the anchor.

## v2.4.2 — 2026-06-15
context-aware hover (no modal bleed, active tab, co-highlight, headers)
The hover pass gathered every layer's click regions, so a large modal (e.g. the
near-full-screen help modal) let background regions sitting geometrically inside
it bleed through — hovering content highlighted whole rows. Rewrite it to
consider only the foreground's own regions per context (main / repo page /
settings / help / keyboard / diff / copy / base / build-info / confirm).
Also, per request:
- Hovering the active help tab keeps its active color instead of a hover tint.
- Hovering a status-bar command or footer hint highlights the key and its label
  together (every span that runs the same command/key), not just the one cell.
- Sortable list column headers now highlight on hover.

## v2.4.1 — 2026-06-15
hover-effects bugs (stale highlight, lost clicks, modal bleed)
- Toggling hover off left the UI unclickable: disabling all-motion tracking
  (1003l) drops button reporting in some terminals. Re-assert 1000h/1002h/1006h
  when turning it off.
- Hover highlights showed even with the setting off: a stray motion event could
  set the cursor position regardless. Only record it when hover effects are on.
- Hover bled through to the background behind a modal: every layer's click
  regions were considered. Scope hovering to the foreground modal's area — the
  status bar, list, and footers behind it stay inert; background-only targets
  (splitter, list rows) only hover when no modal is open.

## v2.4.0 — 2026-06-15
optional mouse hover effects
Add a "Hover effects" setting (Settings → Mouse, off by default). When on,
polygit enables all-motion mouse tracking (DEC 1003) and highlights the
actionable element under the cursor: status-bar commands, footer hints,
table-sort headers, column chips, info links/copy buttons, scrollbars, the
splitter, and main-list rows.
Mechanism: a post-palette `apply_hover` pass paints a subtle hover background
(selection color halved toward the surface) over whichever registered click
region — or scrollbar track / divider / list row — contains `app.hover`. The
event loop records the cursor on `Moved` events and syncs the 1003 terminal mode
to the setting each render (and disables it on exit, since DisableMouseCapture
doesn't). Off by default because all-motion tracking takes over the terminal's
own text selection and URL hover.

## v2.3.3 — 2026-06-15
make unavailable column-picker chips reliably dim on normal/soft bg
The unavailable (`–`) chips relied on the DIM attribute, which is materialized
(faded toward bg) only on a normal/soft background — and even then it didn't
always fire, leaving them nearly as bright as the off (`○`) chips. On a terminal
background the native dim worked, so the two looked fine only there.
Set the dim color explicitly: pre-blend `faint` 0.72 toward the resolved
background (no DIM-attribute dependency), so unavailable chips clearly recede in
both light and dark normal/soft themes. Terminal background keeps the DIM
attribute (its native dim already looked right).

## v2.3.2 — 2026-06-15
pane-title copy, copy-button underline, /filter click toggle
Pane-title copy (`⧉`): clicking it copied nothing. Two bugs — the click region
was one cell left of the right-aligned glyph (sub(2) instead of sub(1)), so every
click missed; and it copied the log, which is empty for an up-to-date repo. Align
the region to the glyph, and copy the log when it has output, otherwise the repo
path — so it's always useful (same clipboard handler as the info-panel Path copy).
Copy button: the info-panel `⧉` was styled as a link (underlined). It's a button,
not a link — cyan + bold, no underline, matching the pane-title `⧉`.
`/ filter`: clicking the hint while already filtering now exits filter input
(dropping an empty filter) instead of being a no-op — a proper toggle.

## v2.3.1 — 2026-06-15
column-picker polish (distinct unavailable glyph, two-row wrap)
Repo-page column menu: unavailable columns (no repo has that data) were a dim
`○`, indistinguishable from an off-but-available `○`. Give them a distinct,
non-circular `–` rendered faint (DIM), so the three states read clearly: on `●`,
off `○`, unavailable `–`.
Root column picker: with all columns it no longer fit one status row and got
truncated with `…`. Pack the chips across as many rows as needed (a new
`pack_chips_into_rows` breaks only at chip boundaries); when it wraps it takes
over the find row while open, so every column stays visible and clickable —
including on the second row.

## v2.3.0 — 2026-06-15
align repo-page branch table, make its columns sortable
Alignment fix: the `upstream` data cell wasn't padded to its header's fixed
28-cell width (only `age` was), so a short upstream let the `base` column — and
everything after it — start at a different screen column than the header,
misaligning rows against each other and against the headers. Pad `upstream`
and `base` to the header width so every column lines up.
Sortable table: click any branch-table column header to sort by it (re-click
flips ▲/▼); the active column shows the arrow and renders bold. Sorts by name,
ahead/behind, dirty, added/modified/deleted/total, upstream, base, age, or
subject. Branch and worktree sections sort independently; stashes keep their
recency order; the selection stays on the same row across a re-sort; default is
git's natural order (HEAD first). Age sorts chronologically via a new
committer-timestamp field (`%(committerdate:unix)`) threaded through
BranchInfo → PageRow.

## v2.2.0 — 2026-06-15
interactive keyboard viewer, dynamic + clickable hint footers, theme-aware dim
Keyboard viewer (Hotkeys help tab → `[K ⌨ keyboard]` or `K`): a responsive
on-screen keyboard built from the shared keymap.json — full bordered board when
there's room, a compact strip when not, nav cluster on the right. Press or click
any key (including physical Shift/Ctrl on terminals with the Kitty protocol) to
highlight it and list every action it drives in a scrollable panel; Esc closes.
Dynamic status-bar hints: the repo-only actions (enter page · c claude ·
l lazygit · o open · y/Y copy) are hidden when the selected row isn't a repo
(the Result/Errors summary row or a folder/group header) — nothing for them to
act on.
Clickable, root-styled footers everywhere: the repo page and every modal footer
(settings, copy menu, base picker, diff modal) now render in the root status-bar
style (bold accent keys, dim labels, `·` separators) and are clickable. A
clicked hint injects its key through a synthetic-key queue, so it runs the exact
same handler as the keypress — no per-action duplication.
Theme-aware DIM: materialized DIM faded foreground 70% toward background, which
washed out disabled hints to near-white on light backgrounds. Fade only 40% on
light backgrounds; the light recording's one baked-in dim value is remapped to
match.

## v2.1.0 — 2026-06-14
interactive keyboard viewer in the TUI (Hotkeys tab)
Add an in-terminal keyboard viewer reachable from the help modal's Hotkeys
tab via a clickable [⌨ keyboard] button. It mirrors the docs-site viewer and
is built from the same source: src/keymap.rs `include_str!`s the very
keymap.json the docs import, so the two can't drift.
The modal draws an OS-aware on-screen keyboard (QWERTY block + nav/arrow
cluster + per-OS modifier row) with bound keys highlighted. Pressing or
clicking any key selects it and lists every action that key drives in a
scrollable panel below; Esc closes the viewer and stops capturing. While open
it captures all keypresses (Ctrl-C still quits, for safety/consistency).
- src/keymap.rs: parse shared keymap.json, physical-key -> actions map,
  token/keycode -> layout-code resolution, layout(Os) + cluster(), Os::current().
- app.rs: keyboard modal state + click regions; ScrollKind::Keyboard.
- render.rs: render_keyboard_modal + the Hotkeys-tab [⌨ keyboard] button.
- main.rs: open-from-button, capture-all-keys-except-Esc, key/close/wheel mouse.
- Docs: keybindings guide tip + README help-modal note.
Verified under a real-sized pty (120x40, pyte): button opens the modal;
pressing `c` lists 3 actions, `/` lists 2; Esc returns to the help modal.

## v2.0.1 — 2026-06-13
WSL clipboard via UTF-16LE; single-cell status glyphs
- copy_to_clipboard feeds clip.exe UTF-16LE (it reads stdin as the OEM
  code page and otherwise mangles non-ASCII, e.g. • → ΓÇó). The Unix
  tools still get UTF-8. Fixes copy under WSL.
- Status glyphs skipped/no-upstream move from the Math Operators block
  (⊘ ⊝, which Cascadia Code and friends lack, so terminals substituted a
  double-width fallback and shifted the repo name) to Geometric Shapes
  (◇ ▽), which those fonts render at a true single cell. Legend + docs
  updated to match.

## v1.9.0 — 2026-06-13
pulled/changed columns + info-panel pull delta
Surface what each pull actually delivered:
- Two optional sortable list columns: `pull` (commits the last pull
  landed, `t p`) and `chg` (files changed, `t c`), rendered as
  glyph+count per the icon set. Dim zero for up-to-date repos, `…`
  while still pulling, and both auto-hide once a run finishes having
  pulled nothing.
- Info-panel "Pulled" row showing the before/after delta:
  old sha → new sha · N commits · M files (+ins −del) · N new tags ·
  N new branches. Tag/branch counts are best-effort from fetch output;
  commits/files/sha come exactly from the reflog (HEAD@{1}..HEAD).
- The info panel re-fetches details after a pull (details_stale flag),
  so ahead/behind and last-commit reflect the new HEAD.
- The sort (`s`) leader menu now lists only currently-visible columns.
New pure parsers parse_shortstat / parse_fetch_summary in git.rs, with
unit tests. Docs (README, keymap.ts, columns-and-glyphs.mdx) updated.

## v1.8.0 — 2026-06-13
copy-confirmation toast with content preview, failure-kind labels in the status column

## v1.7.0 — 2026-06-12
softer dirty marker, icon fixes, wrapping info-panel links, terminal bg + live theme, restart button
- Dirty `•N` marker is amber, not red (uncommitted = "modified", not an error); red stays for failures
- Distinct worktree glyph `⑃` (vs branches `⑂`); group/folder rollup gains a space (`⊘ 1 (2)`) so the glyph no longer collides with its count; pull-log markers always render as Unicode (`⊘`/`↻`) regardless of icon style
- Info panel: Path copy `⧉` moved after the value so the value column aligns; Branch and Remote links wrap (breaking on separators) instead of truncating, each wrapped segment clickable
- Background gains a `terminal` option (no base background — the terminal's shows through); the `auto` theme re-detects dark/light at runtime (tty-safe poll) so an OS light↔dark switch re-themes live, no restart
- Build-info modal gains a `[restart]` button (or `r`) that exec-restarts into the latest build
Docs + README synced; 109 unit tests.

## v1.5.1 — 2026-06-11
clickable "built … ago" tag opens a Build info modal
Clicking the status-bar "built … ago" tag opens a modal showing the running version, the
watched executable path, when it was built, how the new-build watch works (polls size+mtime),
and whether a newer build is currently waiting. Any key or click closes it.
Docs + README updated.

## v1.5.0 — 2026-06-11
interactive info panel, gentler deleted-upstream status, reload notice on every screen
Info panel (i)
- Bold field labels; hide rows that carry nothing (↑0 ↓0, all-zero Changes, empty Worktrees)
- Clickable links to the remote: branch → /tree, commit hash → /commit, Remote → repo
- Truncated values expand on click (path left-truncated to keep the filename tail; long commit subject from the right); expanded text wraps from the value column, never under the label
- ⧉ copy buttons: next to Path (absolute path) and on the log pane border (whole pull log)
- Status spells out the timing ("pull took 1.49s")
Status
- A branch whose tracked remote ref was deleted ("no such ref was fetched") is classified as no-upstream, not a red failure
New-build notice
- Rendered on top of every screen (repo list, repo page, and over any modal), so a freshly-installed build is always one click from reloading
Docs + README synced; 99 unit tests.

## v1.4.0 — 2026-06-11
recursive discovery, tree view, throttle adaptation, groups; repo-page columns + info panel, diff filters, theme split
Discovery & list
- Recursive, streaming, pruned parallel repo discovery (--depth caps it)
- Collapsible directory-tree view (v t), orthogonal to grouping (flat / grouped / tree / tree+groups)
- Repo groups from groups.json (pattern / list / command / url sources, TTL-cached)
- Throttle adaptation: detect remote rate-limiting, halve concurrency, exponential backoff, restore when quiet
- Always-sorted list, Name asc default; sort by checked-out branch (s c); removed the no-sort option
Repo page
- Branch column system (page-local t menu, persisted) with added / modified / deleted / total stats vs the base branch (async per-branch pass)
- Bottom info panel (i) with branch / upstream / base + merge-base sha, ahead-behind, change stats, and tip commit
- Dim zeros and auto-hidden empty columns, via a count-cell helper shared with the root list
Diff modal
- Footer hints adapt to the focused pane; Shift/Alt+PgUp/PgDn pages the file list
- Status-filter chips with count badges when a change set has >10 files across 2+ statuses; list groups by status
- Diff-panel title shows the full path, left-truncating only when it overflows
Theme
- Background and Contrast are now independent axes (surface tones vs text/accent saturation), composed at runtime
- Brighter, more separated light-theme diff green/red
Docs site + README kept in sync; 98 unit tests.

## v1.0.0 — 2026-06-09
sortable list, contextual help, branch diffs, themes, draggable scrollbars
First stable release. Highlights since 0.17.x:
Repo list
- No-upstream is a distinct, non-error state (`⊝`), off the Errors page, counted as done.
- Sort by any column via the `s` leader or by clicking a 2-row column header (▲/▼);
  `f` is the status-filter leader; `/` filters by name. (`e`/`E` = refetch.)
- Always-on dirty marker (`•`); `t d` adds the count (`•N`). `age` column widened.
- Lazygit-style panes: rounded borders, a bright border on the focused pane (`Tab`/`1`/`2`),
  and a draggable divider with a grip + live highlight.
- 3-row footer grouped by purpose, with right-aligned per-row stats that collapse gracefully.
- Refetch keeps old column values and flashes only the cells that changed.
Repo page & diffs
- `l` opens lazygit; `o` opens the branch on the remote in the browser; `y` is a copy menu
  (path / branch / both); the `d` action and footer are dynamic to the selected row.
- Selection opens on the current (HEAD) branch; empty/0 worktree+stash sections are hidden;
  icon-prefixed section headers; a `•N` dirty-count column; bottom action banner.
- Diff modal: two bordered sub-panels, `Tab` to switch focus, `Enter` on a branch shows its
  changes vs the base branch, "no changes" toasts instead of an empty modal.
App-wide
- Context-aware, tabbed help (`?`): Hotkeys (for the current view) · CLI & Flags · About,
  switchable by `Tab` or click; multi-line, clickable footer labels.
- Draggable scrollbars everywhere (preview, diff panels, help, repo page), thumb highlighted
  while dragged; fixed the thumb not reaching the bottom at max scroll.
- Reusable toast component; modal drop-shadows + rounded borders.
- Settings (`,`): panel padding, Unicode⇄emoji icons, and a theme (auto / dark / light) — persisted.
README + docs updated.
