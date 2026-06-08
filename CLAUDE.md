# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`pull-all` is an interactive multi-repo `git pull` dashboard: a Rust/ratatui TUI that pulls every git repo in a directory in parallel with live per-repo logs. The Rust build is canonical; the same binary also fronts Go, Bun, and bash alternatives via `pull-all go|bun|cli` subcommands (which `exec` siblings from `pull-all-siblings/`, kept off `$PATH`).

Stack: Rust (stable) · ratatui 0.29 · crossterm 0.28 (event-stream) · tokio · clap · anyhow.

## Commands

```bash
make build          # cargo build --release → bin/pull-all
make test           # cargo test
make bench          # time bin/pull-all --no-tui on the cwd
cargo clippy        # lint (keep clean before committing)
cargo test <name>   # run a single test, e.g. cargo test classify_no_upstream
```

- **Unit tests live in `src/git.rs`** (`classify_pull_output`, the `parse_*` helpers) and `src/app.rs` (retry/refetch/sort logic). Pure functions only — the TUI itself is verified manually.
- **Run the TUI:** `pull-all [DIR]`. Plain streaming mode: `pull-all --no-tui [DIR]` (the TUI is gated on `stderr` being a TTY — redirecting stderr forces plain mode). `-j N` / `PULL_JOBS` sets concurrency; `--timeout S` per pull.
- **Installing while a copy is running:** `make install` does a plain `cp` which fails with "Text file busy" if the binary is in use. Use an atomic rename instead: `cp target/release/pull-all ~/bin/pull-all.new && mv -f ~/bin/pull-all.new ~/bin/pull-all`.
- **Bump `Cargo.toml` version on every change** (patch = fix, minor = feature) — this project treats it as release-worthy.

## Architecture

Source is a flat module set under `src/` (no submodules); each file is one concern:

- **`main.rs`** — clap CLI, sibling dispatch, terminal setup, and the **synchronous event loop** (`run_event_loop`). Owns all key + mouse handling, the leader-chord state machine, and "suspend the TUI to run an external program" flows.
- **`app.rs`** — all state types: `AppState` (the god-object), `RepoState`, the status/column/sort/filter/leader/icon enums, `IconSet`, and the **pure logic + hit-test helpers** (`visible_indices`, `list_selection_at`, `set_sort`, `counts`, etc.).
- **`render.rs`** — every ratatui draw call: the two main panes, status-bar footer, info block, help/settings/confirm/diff modals, and the full-screen repo page. No state mutation except writing captured geometry back to `AppState`.
- **`worker.rs`** — async tokio tasks: the pull workers (`pull_repo`, semaphore-bounded), refetch/retry batches, and the lazy loaders for repo details, diffs, the repo page, and diff-modal file lists.
- **`git.rs`** — every `git` subprocess call + output classification/parsing. `classify_pull_output` maps stdout/stderr+exit to a `PullOutcome`.
- **`plain.rs`** — the `--no-tui` path; output is byte-compatible with the original bash `pull-all-repos` script.
- **`persist.rs`** — `~/.config/pull-all/state.json` (columns, sort, icon style, padding, help tab, splitter). `#[serde(default)]` so old files load.
- **`profile.rs`** — the optional `--profile` per-repo timing report.

### Concurrency model

`AppState` is shared as `Arc<Mutex<AppState>>` between the synchronous event loop and spawned tokio tasks. Each repo is an independent `Arc<Mutex<RepoState>>` (`SharedRepoState`). Workers mutate per-repo state; the loop reads it to render. **Before spawning a task or doing anything slow, `drop(app)` to release the `AppState` lock** — holding it across `.await` or a subprocess deadlocks the UI. The loop locks `AppState` once per iteration to render and once to handle each event.

### Render-every-tick

The loop polls events with a 50ms timeout and calls `terminal.draw` every iteration regardless of input. Animations (spinner, refetch attention-flash, divider drag highlight) rely on this — they're derived from `Instant`/tick at render time, not driven by events.

### Geometry capture → hit-testing (load-bearing)

`render.rs` writes the **exact** `Rect`s it drew into back onto `AppState` every frame (`list_rows_area`, `header_area`/`header_click`, `preview_scroll_area`, `diff_files_area`, `diff_body_area`, `divider_col`, `clickable`, …). Mouse handlers in `main.rs` hit-test against those captured rects — they must **not** recompute geometry from borders/padding. Hardcoding "+1 for the border" silently breaks when panel padding or the column header shifts content; always capture-then-hit-test.

### Leader chords

`app.pending_leader` (`Toggle` = `t`, `Filter` = `f`, `Sort` = `s`) is a two-key chord: the first key arms it, the next picks a column/filter/sort. Handled in `main.rs` *before* the normal-key match. Current top-level keymap (see README for the full table): `t` columns · `s` sort · `f` status-filter · `/` name-filter · `r`/`R` retry · `e`/`E` refetch · `c` claude · `l` lazygit · `1`/`2` pane focus.

### Icon abstraction

`IconStyle` (Unicode vs emoji) selects an `&'static IconSet`; all glyphs route through it. Render pads columns by **display width** (`pad_display`/`unicode-width`) because emoji are 2 cells. Only single-codepoint emoji are allowed — variation-selector sequences (e.g. `⏭️`, `⚠️`) render at inconsistent widths across terminals and desync/garble columns.

### Suspend-to-launch

`c` (claude) and `l` (lazygit) set a `pending_*: Option<PathBuf>` in the key handler; at the top of the next loop iteration the TUI pops keyboard-enhancement flags, leaves the alt screen, runs the external program to completion, then restores (`launch_claude` / `launch_lazygit`). ANSI parsing in `ansi_line_to_ratatui` iterates **chars, not bytes** (byte-as-char corrupts multi-byte UTF-8).

### Adding a `RepoStatus`/`PullOutcome` variant

`counts()` returns a fixed-arity tuple and many `match`es over `RepoStatus` are exhaustive — a new variant ripples to `render.rs` (glyph/color/label), `worker.rs` (outcome→status), `plain.rs`, `profile.rs`, and the result/error summaries. Classification of new outcomes happens in `git.rs::classify_pull_output`.

## Repo conventions

- **This is a public personal repo: keep it free of any employer/organization-internal names** (internal service names, hosts, property IDs, private URLs, org details) in source, tests, comments, commit messages, or PR bodies — the tool scans whatever real repos you point it at, but none of that belongs in tracked content. Grep the diff before committing.
- **Verifying TUI changes:** run it under tmux and drive it with SGR mouse sequences (`\e[<0;col;row M`/`m` click, `\e[<64/65..M` wheel); `tmux capture-pane -e -p` shows color escapes for asserting active-pane borders, flashes, etc. "typecheck + clippy pass" is not "done" for visual changes.

## Docs ↔ code sync (mandatory)

The docs site (`docs/`, Astro Starlight → GitHub Pages, auto-deploys on any push touching `docs/`) and `README.md` are **part of every user-facing change, in the same commit** — not a follow-up. When you add/change/remove a keybinding, flag, status, glyph, modal, pane behavior, or any visible feature, update:

1. `docs/src/data/keymap.ts` — the **single source of truth for the keybinding table** (the in-page explorer renders from it). It carries a `(vX.Y.Z)` stamp.
2. The relevant `docs/src/content/docs/**` page(s) and the `README.md` keybinding table / feature list.
3. The `(vX.Y.Z)` stamp in `keymap.ts` — bump it to match the new `Cargo.toml` version. **This bump is your sign-off that the keybinding docs reflect this release.**

This is enforced by three layers, so drift is caught, not just discouraged:

- **Prevention (a test gate):** `tests/docs_in_sync.rs` asserts the `keymap.ts` stamp equals `CARGO_PKG_VERSION`. Because every change bumps `Cargo.toml` and `make test` runs before push, shipping code without re-stamping the keymap **fails the build** — forcing a deliberate "do the docs need updating?" checkpoint each release.
- **Detection (visible staleness):** Starlight `lastUpdated` (in `docs/astro.config.mjs`) shows each page's git last-modified date in its footer; the deploy workflow uses `fetch-depth: 0` so the dates are real. A page that hasn't moved while the code churned is visibly stale.
- **Intent (this rule):** docs live beside the code that they describe; treat a docs edit as required, not optional.

(A deeper single-source would generate `keymap.ts` from `main.rs`, but key handling is imperative `match` arms, not declarative — the version-stamp gate is the pragmatic 80/20.)
