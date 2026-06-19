//! Self-contained modal helpers: centering, drop shadow, a `[x]` close button, and a clickable
//! hint-footer builder. No host dependencies — the crate owns these so it stands alone.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// A keystroke a footer hint represents (so a click can inject the same key the hint names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKey {
    Char(char),
    Enter,
    Tab,
    Esc,
}

/// A clickable hint region captured at render time: a single cell-row span bound to a [`HintKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintClick {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub key: HintKey,
}

impl HintClick {
    /// Whether `(col, row)` falls inside this region.
    pub fn contains(&self, col: u16, row: u16) -> bool {
        row == self.row && col >= self.col_start && col < self.col_end
    }
}

/// A centered `width`×`height` rect clamped inside `area`.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect { x, y, width, height }
}

/// Dim a 1-cell drop shadow down the right edge and across the bottom of `area`, offset by +1.
/// Call before the modal's `Clear` so the shadow falls on the UI just outside the box.
pub fn cast_shadow(frame: &mut Frame, area: Rect) {
    let bounds = frame.area();
    let buffer = frame.buffer_mut();
    let shadow_x = area.x + area.width;
    for row in (area.y + 1)..(area.y + area.height + 1) {
        if shadow_x < bounds.right() && row < bounds.bottom() {
            if let Some(cell) = buffer.cell_mut((shadow_x, row)) {
                cell.set_bg(Color::Black).set_fg(Color::DarkGray);
            }
        }
    }
    let shadow_y = area.y + area.height;
    for col in (area.x + 1)..(area.x + area.width + 1) {
        if col < bounds.right() && shadow_y < bounds.bottom() {
            if let Some(cell) = buffer.cell_mut((col, shadow_y)) {
                cell.set_bg(Color::Black).set_fg(Color::DarkGray);
            }
        }
    }
}

/// A right-aligned `[x]` close button for a modal's top border + its click region `(row, start, end)`.
pub fn modal_close_button(modal: Rect) -> (Line<'static>, (u16, u16, u16)) {
    let text = "[x]";
    let line = Line::from(Span::styled(
        text,
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD),
    ))
    .right_aligned();
    let end = modal.x + modal.width.saturating_sub(1);
    let start = end.saturating_sub(text.len() as u16);
    (line, (modal.y, start, end))
}

/// Lay out styled `(text, style, key)` footer segments left→right from `(x, row)`, registering a
/// [`HintClick`] for each keyed segment, and return the assembled `Line`.
pub fn build_hint_footer(
    segments: Vec<(String, Style, Option<HintKey>)>,
    x: u16,
    row: u16,
    clicks: &mut Vec<HintClick>,
) -> Line<'static> {
    let mut spans = Vec::with_capacity(segments.len());
    let mut col = x;
    for (text, style, key) in segments {
        let width = UnicodeWidthStr::width(text.as_str()) as u16;
        if let Some(key) = key {
            clicks.push(HintClick { row, col_start: col, col_end: col + width, key });
        }
        col += width;
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// A `key`+` label` chip pair for [`build_hint_footer`]: the key bright, the label dim, both clickable.
pub fn footer_chip(key: &str, label: &str, hint: HintKey) -> Vec<(String, Style, Option<HintKey>)> {
    vec![
        (key.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD), Some(hint)),
        (label.to_string(), Style::default().fg(Color::DarkGray), Some(hint)),
    ]
}

/// A non-clickable ` · ` separator segment.
pub fn footer_sep() -> (String, Style, Option<HintKey>) {
    (" \u{b7} ".to_string(), Style::default().fg(Color::DarkGray), None)
}
