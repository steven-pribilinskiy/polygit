//! Presentational primitives — **buttons** and **list rows** — drawn in every interaction state, so
//! a host gets one themeable widget vocabulary it can reuse everywhere (and show off storybook-style).
//!
//! Each primitive is a pure `&str` → [`Line`] builder taking an [`Interaction`] state and a small
//! style struct. Colors default to **semantic ANSI** (so a host that remaps the frame buffer themes
//! them for free) and the two effect axes — button hover and list selection — mirror the choices a
//! host like polygit exposes in its settings, so the same toggle drives both the real UI and the
//! showcase.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// The interaction state a primitive is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interaction {
    /// Resting, interactive.
    Normal,
    /// Pointer hovering over it.
    Hover,
    /// Selected / current — a persistent highlight (e.g. the active list row).
    Selected,
    /// Pressed / activated this instant.
    Active,
    /// Has keyboard focus (no pointer) — a focus ring.
    Focused,
    /// Non-interactive: dim and inert, no click affordance.
    Disabled,
}

impl Interaction {
    /// All states, in showcase order.
    pub const ALL: [Interaction; 6] = [
        Interaction::Normal,
        Interaction::Hover,
        Interaction::Selected,
        Interaction::Active,
        Interaction::Focused,
        Interaction::Disabled,
    ];

    /// A short label for the showcase.
    pub fn label(self) -> &'static str {
        match self {
            Interaction::Normal => "normal",
            Interaction::Hover => "hover",
            Interaction::Selected => "selected",
            Interaction::Active => "active",
            Interaction::Focused => "focused",
            Interaction::Disabled => "disabled",
        }
    }
}

/// How a button shows hover/active (mirrors a host's "Button hover" setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverEffect {
    /// Reverse video: label color becomes the background.
    Inverted,
    /// A light touch: accent label + bold, no fill.
    Subtle,
}

/// How a list row shows selection (mirrors a host's "List selection" setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionEffect {
    /// A solid accent bar across the row.
    Accent,
    /// A faint background tint + bold label.
    Subtle,
}

/// Styling for [`button`].
#[derive(Debug, Clone, Copy)]
pub struct ButtonStyle {
    /// Label color at rest.
    pub label: Color,
    /// Accent for hover/active fills, the focus ring, and the selected state.
    pub accent: Color,
    /// Foreground used on accent fills (inverted hover / active).
    pub on_accent: Color,
    /// Dim color for the disabled state.
    pub disabled: Color,
    /// How hover/active are shown.
    pub hover: HoverEffect,
    /// Wrap the label in `[`…`]` brackets (chip look).
    pub brackets: bool,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        ButtonStyle {
            label: Color::Gray,
            accent: Color::Blue,
            on_accent: Color::White,
            disabled: Color::DarkGray,
            hover: HoverEffect::Inverted,
            brackets: true,
        }
    }
}

/// Render a button label in the given [`Interaction`] state as a single styled [`Line`].
pub fn button(label: &str, state: Interaction, style: &ButtonStyle) -> Line<'static> {
    let text = if style.brackets { format!(" {label} ") } else { label.to_string() };
    let span = match state {
        Interaction::Normal => Span::styled(text, Style::default().fg(style.label)),
        Interaction::Hover => match style.hover {
            HoverEffect::Inverted => Span::styled(
                text,
                Style::default().fg(style.on_accent).bg(style.accent).add_modifier(Modifier::BOLD),
            ),
            HoverEffect::Subtle => {
                Span::styled(text, Style::default().fg(style.accent).add_modifier(Modifier::BOLD))
            }
        },
        Interaction::Active => Span::styled(
            text,
            Style::default()
                .fg(style.on_accent)
                .bg(style.accent)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Interaction::Selected => Span::styled(
            text,
            Style::default().fg(style.on_accent).bg(style.accent).add_modifier(Modifier::BOLD),
        ),
        Interaction::Focused => Span::styled(
            text,
            Style::default().fg(style.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ),
        Interaction::Disabled => Span::styled(text, Style::default().fg(style.disabled)),
    };
    Line::from(span)
}

/// Styling for [`list_item`].
#[derive(Debug, Clone, Copy)]
pub struct ListItemStyle {
    /// Label color at rest.
    pub label: Color,
    /// Accent for the selected bar / focus.
    pub accent: Color,
    /// Foreground on the accent bar.
    pub on_accent: Color,
    /// Faint background for hover + the subtle selection effect.
    pub muted_bg: Color,
    /// Dim color for the disabled state.
    pub disabled: Color,
    /// How selection is shown.
    pub selection: SelectionEffect,
}

impl Default for ListItemStyle {
    fn default() -> Self {
        ListItemStyle {
            label: Color::Gray,
            accent: Color::Blue,
            on_accent: Color::White,
            muted_bg: Color::DarkGray,
            disabled: Color::DarkGray,
            selection: SelectionEffect::Accent,
        }
    }
}

/// Render a list row (optional `icon`, `label`, optional trailing `badge`) in the given state,
/// padded to `width` cells so the row background fills the full line.
pub fn list_item(
    icon: Option<&str>,
    label: &str,
    badge: Option<&str>,
    state: Interaction,
    style: &ListItemStyle,
    width: u16,
) -> Line<'static> {
    let lead = match icon {
        Some(icon) => format!("{icon} {label}"),
        None => label.to_string(),
    };
    // Pad so the row fills `width` (badge sits at the right).
    let badge_text = badge.unwrap_or("");
    let badge_w = UnicodeWidthStr::width(badge_text);
    let lead_w = UnicodeWidthStr::width(lead.as_str());
    let gap = (width as usize).saturating_sub(lead_w + badge_w + 2).max(1);
    let body = format!(" {lead}{}{badge_text} ", " ".repeat(gap));

    let (fg, bg, modifier) = match state {
        Interaction::Normal => (style.label, None, Modifier::empty()),
        Interaction::Hover => (style.label, Some(style.muted_bg), Modifier::empty()),
        Interaction::Selected | Interaction::Active => match style.selection {
            SelectionEffect::Accent => (style.on_accent, Some(style.accent), Modifier::BOLD),
            SelectionEffect::Subtle => (style.label, Some(style.muted_bg), Modifier::BOLD),
        },
        Interaction::Focused => (style.accent, None, Modifier::BOLD | Modifier::UNDERLINED),
        Interaction::Disabled => (style.disabled, None, Modifier::empty()),
    };
    let mut span_style = Style::default().fg(fg).add_modifier(modifier);
    if let Some(bg) = bg {
        span_style = span_style.bg(bg);
    }
    Line::from(Span::styled(body, span_style))
}

/// A filled / hollow radio glyph + label, used for option groups (e.g. settings radios).
pub fn radio(label: &str, active: bool, style: &ButtonStyle) -> Line<'static> {
    let glyph = if active { "\u{25cf}" } else { "\u{25cb}" }; // ● / ○
    let label_style = if active {
        Style::default().fg(style.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(style.label)
    };
    Line::from(vec![
        Span::styled(format!("{glyph} "), Style::default().fg(style.accent)),
        Span::styled(label.to_string(), label_style),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn button_brackets_and_disabled_is_dim() {
        let style = ButtonStyle::default();
        let normal = button("Save", Interaction::Normal, &style);
        assert_eq!(line_text(&normal), " Save ");
        let disabled = button("Save", Interaction::Disabled, &style);
        // Disabled keeps the text but uses the dim color and no emphasis.
        assert_eq!(disabled.spans[0].style.fg, Some(style.disabled));
        assert!(!disabled.spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inverted_hover_fills_background() {
        let style = ButtonStyle { hover: HoverEffect::Inverted, ..ButtonStyle::default() };
        let hover = button("Go", Interaction::Hover, &style);
        assert_eq!(hover.spans[0].style.bg, Some(style.accent));
    }

    #[test]
    fn subtle_hover_has_no_fill() {
        let style = ButtonStyle { hover: HoverEffect::Subtle, ..ButtonStyle::default() };
        let hover = button("Go", Interaction::Hover, &style);
        assert_eq!(hover.spans[0].style.bg, None);
        assert_eq!(hover.spans[0].style.fg, Some(style.accent));
    }

    #[test]
    fn list_item_selection_effects_differ() {
        let accent = ListItemStyle { selection: SelectionEffect::Accent, ..ListItemStyle::default() };
        let subtle = ListItemStyle { selection: SelectionEffect::Subtle, ..ListItemStyle::default() };
        let row_a = list_item(None, "repo", None, Interaction::Selected, &accent, 20);
        let row_b = list_item(None, "repo", None, Interaction::Selected, &subtle, 20);
        assert_eq!(row_a.spans[0].style.bg, Some(accent.accent));
        assert_eq!(row_b.spans[0].style.bg, Some(subtle.muted_bg));
    }

    #[test]
    fn list_item_fills_width() {
        let style = ListItemStyle::default();
        let row = list_item(None, "x", None, Interaction::Normal, &style, 20);
        assert_eq!(UnicodeWidthStr::width(line_text(&row).as_str()), 20);
    }
}
