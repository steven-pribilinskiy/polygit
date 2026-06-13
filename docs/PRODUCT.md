# Product

## Register

product

(The site is a reference-docs site that serves the tool. The **homepage `index.mdx`** is the one
brand surface — it gets brand-level craft, a custom hero, and the terminal showcase; every other
page stays a fast, legible Starlight content page.)

## Users

Developers who keep many git repos on disk — a `~/projects` tree, a polyrepo, an OSS contributor's
fan-out of clones. They live in the terminal, read docs to learn a keybinding or flag, and leave.
They arrive from a README link or a search, scan for the one thing they need (a key, a flag, the
install line), and want it without ceremony. They already trust terminal tools; the docs must earn
the same trust at a glance.

## Product Purpose

`polygit` is a Rust/ratatui TUI that discovers every git repo under a directory and manages them as
one fleet: parallel fast-forward pulls with live per-repo logs, a persisted status cache so a launch
is useful instantly, configurable auto-pull, and per-repo branch/worktree/stash/diff drill-down. The
docs exist to get someone from "what is this" to "installed and pulling" fast, and to be the
reference they return to for a key or a flag. Success: a visitor finds the answer in one scan and
the site reads unmistakably as *this* tool, not a generic docs template.

## Brand Personality

Terminal-native, precise, quick. Three words: **fast, exact, unfussy.** The voice is a competent
CLI tool's: concrete nouns and verbs, no salesmanship, dry where dry is honest. The interface should
feel like it was made by someone who respects the reader's time and fluency — the same character as
the TUI itself (monospace structure, status glyphs, keyboard-first).

## Anti-references

- **Generic SaaS-cream docs** — warm cream/sand body bg, big gradient hero, identical icon-card
  grids, a tiny uppercase eyebrow over every section. The saturated AI-docs default. No.
- **Stock unstyled Starlight** — default purple accent, zero identity, looks like every other
  Starlight site. It must be visibly polygit.
- **Over-designed / loud** — heavy animation, glassmorphism, neon gradients, anything that fights
  the legibility or speed of reference content. Docs stay fast and readable.
- **Corporate / enterprise** — navy-and-gray dashboard stiffness, formal enterprise-docs tone.
  This is a personal dev tool with character, not a vendor portal.

## Design Principles

1. **The docs ARE a terminal tool's docs.** The identity comes from the app's own material —
   monospace structure, the status-glyph palette, kbd keys, an actual rendered/recorded TUI — not
   from decoration bolted on top. Show the tool, don't describe it.
2. **Reference speed is sacred.** Content pages optimize for scan-and-leave: strong hierarchy, high
   contrast, dense where density helps (the keymap, the legend). Brand craft lives on the homepage
   and in the chrome, never between the reader and the answer.
3. **Earned familiarity over novelty.** Standard Starlight navigation, search, and page structure
   stay standard. Surprise is spent only where it pays (the hero, the glyph legend, the kbd keys).
4. **Honest, concrete copy.** No marketing buzzwords, no aphoristic cadence, no em dashes. Say what
   a key does, what a flag sets, what the tool literally does.

## Accessibility & Inclusion

- WCAG 2.1 AA: body text ≥4.5:1, large/bold ≥3:1, in both light and dark themes (Starlight ships
  both; verify the custom accent + glyph palette in each).
- `prefers-reduced-motion`: the hero showcase and any reveal must degrade to a static, fully-legible
  default — the terminal recording shows a poster frame and doesn't autoplay-loop motion.
- The glyph legend must not rely on color alone — each status carries a glyph + a text label, so
  color-blind readers and the recording's still frames remain decodable.
- Keyboard: the keymap filter, the player controls, and all nav are reachable and visibly focused.
