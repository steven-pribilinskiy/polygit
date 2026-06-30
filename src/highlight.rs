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

/// Highlight `content` (whole file text) into ratatui lines, choosing a syntax by file extension /
/// name (falling back to the first line, then plain text). `dark` picks the theme. Returns one
/// `Line` per source line. Binary / huge files should be filtered by the caller.
pub fn highlight_file(file_name: &str, content: &str, dark: bool) -> Vec<Line<'static>> {
    let set = syntaxes();
    let theme = theme(dark);
    let extension = std::path::Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    let syntax = set
        .find_syntax_by_extension(extension)
        .or_else(|| set.find_syntax_by_token(file_name))
        .or_else(|| content.lines().next().and_then(|line| set.find_syntax_by_first_line(line)))
        .unwrap_or_else(|| set.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for raw in content.lines() {
        match highlighter.highlight_line(raw, set) {
            Ok(ranges) => {
                let spans: Vec<Span<'static>> = ranges
                    .into_iter()
                    .map(|(style, text)| {
                        // Use ONLY the syntect foreground (+ bold/italic) — never the theme's
                        // per-span background, so the preview sits cleanly on the panel surface
                        // (some themes box types/functions in a bg color, which reads as noise).
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
                lines.push(Line::from(spans));
            }
            // On any highlighter hiccup, fall back to the raw line (never drop content).
            Err(_) => lines.push(Line::from(raw.to_string())),
        }
    }
    lines
}

/// Whether `content` looks like binary (has NUL bytes in the sampled prefix) — the caller shows a
/// "binary file" placeholder instead of highlighting. Cheap: checks only the first 8 KiB.
pub fn looks_binary(content: &[u8]) -> bool {
    content.iter().take(8192).any(|&byte| byte == 0)
}
