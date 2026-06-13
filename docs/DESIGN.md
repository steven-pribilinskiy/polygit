# Design

Visual system for the polygit docs site (Astro Starlight). Light is the default; dark mirrors it via
Starlight's `data-theme`. The identity is **terminal-native**: the app's own status-glyph palette is
the design system, monospace carries display and structure, and the homepage centers a real recorded
TUI session.

## Theme

Terminal-native, not skeuomorphic. We borrow the app's *material* (glyph colors, kbd keys, mono
grid, a rendered frame) without faking a CRT. Restrained on content pages (one accent + neutrals),
committed on the homepage hero (the terminal frame and its glyphs carry color). Both light and dark
are first-class; neither is the "real" one.

## Color (OKLCH)

The palette is the app's status glyphs, promoted to a real system. Green is the brand accent (the
TUI's "updated"/branches color). Tinted neutrals lean a hair toward the green hue (≈150), never
toward warm cream.

**Accent (green)** — primary actions, current nav item, links, focus rings.
- light: `--sl-color-accent: oklch(0.62 0.14 152)` (~#1f9d57); low `oklch(0.92 0.06 152)`; high `oklch(0.42 0.11 152)`
- dark:  `--sl-color-accent: oklch(0.76 0.15 152)` (~#36c576); high `oklch(0.92 0.08 152)`

**Glyph roles** (used in the legend, the rendered TUI frame, and inline status dots). Each role is a
token; each is paired with a glyph + label so it never carries meaning by color alone:
- ok/updated → green `--glyph-ok`
- cyan/worktrees → `--glyph-cyan` `oklch(0.7 0.12 210)`
- warn/throttled → amber `--glyph-warn` `oklch(0.75 0.13 80)`
- err/failed → red `--glyph-err` `oklch(0.62 0.2 25)`
- magenta/stash → `--glyph-magenta` `oklch(0.62 0.18 330)`
- muted / dim → neutral ramp (the "idle/cached" dim state)

**Surfaces** — keep Starlight's neutral ramp but nudge tint toward green hue 152 at chroma ≈0.008,
not warm. Content bg stays high-contrast paper/ink; the hero uses a darker terminal surface
(`oklch(0.2 0.02 152)`-ish in light, near-black in dark) so the recorded frame reads as a terminal
regardless of page theme.

Contrast: body ≥4.5:1 both themes; the green accent on white needs the darker `accent-high` for
link/body use (the mid green fails AA as body text). Verify the amber `warn` glyph on light surface —
bump toward `oklch(0.62 0.13 80)` if it's under 4.5:1 against the legend cell.

## Typography

Two families on a real contrast axis (mono + sans), plus the body sans Starlight already ships.
- **Display / structural: monospace** (`ui-monospace` stack, or a bundled mono like JetBrains Mono /
  Berkeley-ish) for the hero wordmark, section labels, kbd keys, the legend glyphs, code. This is the
  terminal signal — used deliberately, not for prose.
- **Body: Starlight's sans** for all running text and reference prose (legibility at scan speed).
- Hierarchy by weight + scale, ≥1.25 step ratio. Content pages use Starlight's fixed rem scale (no
  fluid headings on reference pages). The homepage h1/hero may use one `clamp()` (max ≤ 4rem) since
  it's the brand surface.
- No all-caps body. Short mono labels (≤3 words) may be lowercase-with-tracking, terminal-style, used
  sparingly — NOT an eyebrow over every section.

## Components

- **kbd keys**: mono, subtle 2px bottom border (physical-key hint), high contrast. The keymap
  explorer's defining texture. Keep the existing structure; sharpen contrast + focus.
- **GlyphLegend**: a tight 2-col grid of `glyph · label · note`. Each glyph in its role color. **Fix
  the `--mixed` glyph** — it currently uses `background-clip:text` gradient (an absolute ban): render
  the two-tone status as two adjacent solid glyphs or a glyph + a small swatch, never gradient text.
- **KeymapExplorer**: sticky filter + grouped rows; the site's most product-like surface. Default,
  hover, focus states standardized; filter input has a visible focus ring and an empty state.
- **Terminal frame** (homepage): a bordered "window" (mono title bar `[1] polygit · …`, optional
  traffic-dots OR a plain ⎯ rule, never both) wrapping the recorded session. One consistent frame
  reused for any inline TUI still.
- **Homepage cards**: replace the generic icon+title+text CardGrid reflex with content that teaches —
  paired glyph + one concrete capability line, or a compact feature list, not four identical cards.

## Layout

- Content pages: Starlight's standard three-column shell, untouched structurally (nav speed > novelty).
- Homepage: a custom splash — hero (wordmark + one-line value + the recorded terminal as the
  centerpiece + two actions), then a "what it does" band, quick-start, and a links row. Responsive by
  structure (the terminal frame scales/letterboxes; cards collapse to one column under ~50rem). Test
  the hero heading + the frame at 360px — no overflow.
- Spacing: vary rhythm; generous around the hero frame, denser in the legend/keymap.

## Motion

- 150–250ms state transitions (hover/focus) on content chrome; conveys state, not decoration.
- Homepage: the terminal recording (asciinema-player) is the motion — a real session playing. It
  shows a **poster/still first**, plays on interaction or gentle autoplay, and **respects
  `prefers-reduced-motion`** (no autoplay-loop; static poster + controls). One small entrance on the
  hero is fine; no orchestrated page-load sequence.
- Reduced motion: every transition has a crossfade/instant fallback; the player does not loop.
