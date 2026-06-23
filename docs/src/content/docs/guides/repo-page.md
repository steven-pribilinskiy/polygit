---
title: Repo page & diff modal
description: The per-repo view (panel 4) for branches, worktrees, and stashes, and the inline diff modal.
---

Press `Enter` (or double-click) on a repo in the list to open its **repo page** (panel `[4]`) —
which runs `git fetch` and then lists everything about the repo. By default it opens **restored**
(a docked panel across the bottom, so the list stays live above it and selecting another repo
retargets the page); press `m` or the `[m maximize]` title-bar button (left of `[esc back]`) to
**maximize** it full-screen, `[m restore]`/`m` to restore. **Maximized is a single view** — every
section stacked under its header, no tab bar (tabs apply only while restored). The state is sticky
(Settings → Layout → **Repo page**). Rows hover-highlight like the main list.

## What the page shows

Each section header is prefixed with its type icon. The worktrees and stashes sections only
appear when there's something to list.

- **`⑂ BRANCHES`** — every local branch under a labeled header row, with a HEAD marker,
  fresh ahead/behind vs upstream, an uncommitted-change count, **added / modified / deleted /
  total** change counts vs the base branch, the upstream name, last-commit date, and subject.
- **`⑂ WORKTREES`** — each linked worktree's branch and path.
- **`≡ STASHES`** — every stash entry, with its own **added / modified / deleted / total** change
  counts (from `git stash show`, loaded lazily) shown in the same columns as branches.

A red `●` marks any branch or worktree with uncommitted changes; count cells show a dim zero
rather than a blank. The result of an action (e.g. "Dropped stash@{0}") and any fetch error
appear in a banner pinned to the bottom row.

### Columns (`t`) and the info panel (`i`)

Press `t` (or click the **`t cols ▾`** title-bar trigger) to open the columns dropdown, then
pick with the mouse, the `↑↓` arrows + `Enter`, or a row's mnemonic key — `b` ahead/behind, `y` dirty,
`a` added, `m` modified, `d` deleted, `c` total, `u` upstream, `f` base, `g` age, `r` pr, `s` subject.
Sorting works the same way via `s` (or the **`s sort ▾`** trigger), or by clicking a column header.
The **pr** column shows the current branch's open pull request (a clickable `#N`, via
`gh`) on the HEAD row, blank on the others. The added/modified/deleted counts are each branch's changes vs the merge-base with the
repo's default branch, computed in the background (cells show `…` until ready). A column every
branch leaves empty auto-hides and its dropdown row goes dim and inert. Choices persist across runs.
The page distributes its width across the visible columns — hiding columns reclaims that space
for the **branch** and **subject** text columns (they expand to fill it instead of truncating).

`i` toggles a bottom **info panel** detailing the selected row: branch, upstream, base branch
plus merge-base sha, ahead/behind, change stats, and the tip commit (sha · author · date ·
subject). On the HEAD row it also shows the open **pull request** (`#N title`) when the current
branch has one (via `gh`). For a worktree it adds the path; for a stash, the stash ref and label.
Persisted.

## Acting on a row

Navigate rows with `j`/`k`/`g`/`G`/`Home`/`End` (or the wheel / a click), then:

| Key | Action |
|-----|--------|
| `Enter` / double-click | Open the **diff modal** (stash or dirty row) |
| `Shift`+`Enter` | Check out the selected branch (clean, non-current) |
| `t` | Open the columns dropdown (then `b`/`y`/`a`/`m`/`d`/`c`/`u`/`f`/`g`/`r`/`s`, or `↑↓`+`Enter`) |
| `s` | Open the sort dropdown (then `n`/`b`/`y`/`a`/`m`/`d`/`c`/`u`/`f`/`g`/`s`, or click a header) |
| `i` | Toggle the bottom info panel |
| `p` | Fast-forward the selected branch / worktree |
| `P` | Fast-forward every fast-forwardable branch |
| `d` | Delete branch / drop stash / remove worktree / discard changes — with a confirm |
| `o` | Open the selected branch on the remote |
| `y` | Copy the selected row's path |
| `c` | Launch the AI coding agent in the row's path (set in Settings → Agent) |
| `l` | Open [lazygit](https://github.com/jesseduffield/lazygit) in the row's path |
| `m` | Maximize / restore the page (restored = docked panel `[4]`; maximized = full-screen) |
| `Esc` / `q` | Return to the repo list |

### The `d` key, by row type

`d` is context-sensitive and always routes through a confirmation dialog whose severity
scales with how destructive the action is:

- **Stash** → drop the stash (`git stash drop`). The confirm lists the files the stash holds
  (the ones you'd lose).
- **Worktree** → remove the worktree (force when it's dirty).
- **Non-current branch** → delete it (`git branch -d`, or `-D` when unmerged).
- **Current branch with uncommitted changes** → **discard** those changes
  (`git reset --hard` + `git clean -fd`). The confirm lists exactly which files will be
  restored and which untracked files will be deleted.

While a pull (`p`/`P`) runs, an animated spinner appears in the page title — the same spinner
the main list uses for an in-progress pull.

## The diff modal

`Enter` or a double-click on a **stash** or a **dirty** branch/worktree opens a
90%-of-screen diff modal, split into two bordered, independently scrollable sub-panels (which
also pick up panel padding when that setting is on):

- **Top — `files (N)`** (≤40% of the height): every changed file with its git status
  (`M`/`A`/`D`/`R`/`?`). Pick a file with `↑`/`↓`/`j`/`k`, `g`/`G` for first/last, or by clicking it.
  `Tab` switches focus between the file list and the diff; the footer hint adapts to whichever
  pane is focused. `Shift`/`Alt`+`PgUp`/`PgDn` page the file list (`Shift`/`Alt`+wheel scrolls it
  without moving the selection).
- **Bottom — the selected file's diff**, loaded on demand. Scroll it with `PgUp`/`PgDn`/`Home`/`End`
  or the wheel. The panel title shows the full file path, left-truncating (with a leading `…`)
  only when it doesn't fit the line. **`v` cycles the render style — raw / unified / split**
  (persisted, shown in the footer): **raw** keeps git's own colored output; **unified** and **split**
  are structured, line-numbered, **syntax-highlighted** GitHub-PR-style views (split = old on the
  left, new on the right) with a faint green/red wash on added/removed lines. Highlighting is a
  lightweight, language-aware lexer keyed off the file extension.

When a change set has more than 10 files across at least two statuses, a clickable
**status-filter chip row** appears above the list — `[ all N ]`, `[ M … ]`, `[ A … ]`, … with
count badges. Click a chip or press `f` (cycles all → each status → all) to filter; the list
then groups by status. The diff still loads the originally selected file's absolute index, so
filtering never loses your place.

What the file set contains depends on the source: a **stash** lists the files it holds (including
untracked); a **dirty row** lists uncommitted changes, and `t` toggles between *uncommitted*
(vs HEAD) and *vs base branch* (everything changed since you forked from `origin/HEAD`).

Inside the modal, `d`
discards (current branch), removes (worktree), or drops (stash) — same confirm as the
page. `Esc` or `q` closes it.
