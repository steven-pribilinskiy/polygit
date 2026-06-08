---
title: Repo page & diff modal
description: The full-screen per-repo view for branches, worktrees, and stashes, and the inline diff modal.
---

Press `Enter` (or double-click) on a repo in the list to open its **repo page** — a
full-screen view that runs `git fetch` and then lists everything about the repo.

## What the page shows

Each section header is prefixed with its type icon. The worktrees and stashes sections only
appear when there's something to list.

- **`⑂ BRANCHES`** — every local branch, with a HEAD marker, fresh ahead/behind vs upstream,
  the upstream name, last-commit date, and subject.
- **`⑂ WORKTREES`** — each linked worktree's branch and path.
- **`≡ STASHES`** — every stash entry.

A red `●` marks any branch or worktree with uncommitted changes. The result of an action
(e.g. "Dropped stash@{0}") and any fetch error appear in a banner pinned to the bottom row.

## Acting on a row

Navigate rows with `j`/`k`/`g`/`G`/`Home`/`End` (or the wheel / a click), then:

| Key | Action |
|-----|--------|
| `Enter` / double-click | Open the **diff modal** (stash or dirty row) |
| `Shift`+`Enter` | Check out the selected branch (clean, non-current) |
| `p` | Fast-forward the selected branch / worktree |
| `P` | Fast-forward every fast-forwardable branch |
| `d` | Delete branch / drop stash / remove worktree / discard changes — with a confirm |
| `o` | Open the selected branch on the remote |
| `y` | Copy the selected row's path |
| `c` | Start claude code in the row's path |
| `l` | Open [lazygit](https://github.com/jesseduffield/lazygit) in the row's path |
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
- **Bottom — the selected file's diff**, loaded on demand. Scroll it with `PgUp`/`PgDn`/`Home`/`End`
  or the wheel.

What the file set contains depends on the source: a **stash** lists the files it holds (including
untracked); a **dirty row** lists uncommitted changes, and `t` toggles between *uncommitted*
(vs HEAD) and *vs base branch* (everything changed since you forked from `origin/HEAD`).

Inside the modal, `d`
discards (current branch), removes (worktree), or drops (stash) — same confirm as the
page. `Esc` or `q` closes it.
