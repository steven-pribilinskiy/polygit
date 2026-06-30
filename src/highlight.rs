//! Syntax-highlighted file preview. Uses `two-face` (the bat project's curated syntax + theme sets
//! on top of `syntect`) so broad languages — TypeScript/TSX, TOML, Dockerfile, etc. — are covered
//! and the themes are high-contrast, then renders into ratatui `Line`s via `syntect-tui`. The heavy
//! sets load once (lazily, the first time a file is previewed) so startup pays nothing.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::SyntaxSet;
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName};

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme_set() -> &'static EmbeddedLazyThemeSet {
    static SET: OnceLock<EmbeddedLazyThemeSet> = OnceLock::new();
    SET.get_or_init(two_face::theme::extra)
}

/// A high-contrast theme per background: Monokai Extended on dark (vivid keywords/strings),
/// OneHalf Light on light (strong, legible color without washing out). Both ship with `two-face`.
fn theme(dark: bool) -> &'static Theme {
    let name = if dark {
        EmbeddedThemeName::MonokaiExtended
    } else {
        EmbeddedThemeName::OneHalfLight
    };
    theme_set().get(name)
}

/// Map an extension `two-face`/`syntect` doesn't know to one it does (so JSON-with-comments, modern
/// JS/TS variants, etc. still highlight). Returns the original if there's no alias.
fn alias_extension(ext: &str) -> &str {
    match ext {
        "jsonc" | "json5" => "json",
        "cjs" | "mjs" => "js",
        "cts" | "mts" => "ts",
        "zsh" | "bash" => "sh",
        "yml" => "yaml",
        "htm" => "html",
        other => other,
    }
}

/// Resolve a syntax for `file_name` (by extension, with aliasing; then the whole name; then the
/// first line; else plain text).
fn syntax_for<'set>(set: &'set SyntaxSet, file_name: &str, first_line: &str) -> &'set syntect::parsing::SyntaxReference {
    let extension = std::path::Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    set.find_syntax_by_extension(extension)
        .or_else(|| set.find_syntax_by_extension(alias_extension(extension)))
        .or_else(|| set.find_syntax_by_token(file_name))
        .or_else(|| set.find_syntax_by_first_line(first_line))
        .unwrap_or_else(|| set.find_syntax_plain_text())
}

/// Highlight just the given window of source `lines` (already split, no trailing newlines) — for the
/// virtualized preview, so only the visible rows are highlighted (huge files stay instant). The
/// syntect highlighter runs over the window in order, so multi-line constructs render correctly
/// within it; state from above the window isn't carried (an acceptable trade for config files).
pub fn highlight_window(file_name: &str, lines: &[&str], dark: bool) -> Vec<Line<'static>> {
    let set = syntaxes();
    let theme = theme(dark);
    let syntax = syntax_for(set, file_name, lines.first().copied().unwrap_or(""));
    let mut highlighter = HighlightLines::new(syntax, theme);
    lines
        .iter()
        .map(|raw| match highlighter.highlight_line(raw, set) {
            Ok(ranges) => {
                let spans: Vec<Span<'static>> = ranges
                    .into_iter()
                    .map(|(style, text)| {
                        // Use ONLY the syntect foreground (+ bold/italic) — never the theme's
                        // per-span background, so the preview sits cleanly on the panel surface.
                        let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
                        let mut out = Style::default().fg(fg);
                        if style.font_style.contains(FontStyle::BOLD) {
                            out = out.add_modifier(Modifier::BOLD);
                        }
                        if style.font_style.contains(FontStyle::ITALIC) {
                            out = out.add_modifier(Modifier::ITALIC);
                        }
                        Span::styled(text.to_string(), out)
                    })
                    .collect();
                Line::from(spans)
            }
            Err(_) => Line::from((*raw).to_string()),
        })
        .collect()
}

/// Whether `content` looks like binary (has NUL bytes in the sampled prefix) — the caller shows a
/// "binary file" placeholder instead of highlighting. Cheap: checks only the first 8 KiB.
pub fn looks_binary(content: &[u8]) -> bool {
    content.iter().take(8192).any(|&byte| byte == 0)
}
