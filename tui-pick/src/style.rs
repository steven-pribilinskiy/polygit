//! Themeable styles for the widgets. Defaults use **semantic ANSI colors** (`Color::Cyan`, …) so a
//! host that remaps the frame buffer at flush (like polygit's palette) themes the widgets for free;
//! other hosts can override any field or restyle wholesale.

use ratatui::style::{Color, Modifier, Style};

/// Styling for the [`crate::finder`] overlay.
#[derive(Debug, Clone, Copy)]
pub struct FinderStyle {
    /// Modal border.
    pub border: Color,
    /// The `Select …:` prompt label.
    pub prompt: Style,
    /// The live query text after the prompt.
    pub query: Style,
    /// The `matched/total` counter + the keybinding header line.
    pub header: Style,
    /// A non-matched row.
    pub row: Style,
    /// The selected row (a highlight bar).
    pub selected: Style,
    /// Matched characters within a row.
    pub matched: Style,
    /// The leading type column (e.g. `repo`/`wt`).
    pub kind: Style,
    /// The usage-count column.
    pub count: Style,
}

impl Default for FinderStyle {
    fn default() -> Self {
        Self {
            border: Color::Cyan,
            prompt: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            query: Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            header: Style::default().fg(Color::DarkGray),
            row: Style::default().fg(Color::Gray),
            selected: Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD),
            matched: Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            kind: Style::default().fg(Color::DarkGray),
            count: Style::default().fg(Color::Magenta),
        }
    }
}

/// Styling for the [`crate::picker`] dialog.
#[derive(Debug, Clone, Copy)]
pub struct PickerStyle {
    pub border: Color,
    /// Breadcrumb segments.
    pub breadcrumb: Style,
    /// Toolbar buttons (back / home / up / bookmarks / mode toggle).
    pub button: Style,
    /// The active toolbar button (e.g. the current mode).
    pub button_active: Style,
    /// A folder row.
    pub folder: Style,
    /// A git-repo row (name).
    pub repo: Style,
    /// The `git repo` badge.
    pub badge: Style,
    /// The selected row.
    pub selected: Style,
    /// Matched characters within a row (search).
    pub matched: Style,
    /// The current-path footer.
    pub path: Style,
}

impl Default for PickerStyle {
    fn default() -> Self {
        Self {
            border: Color::Cyan,
            breadcrumb: Style::default().fg(Color::Gray),
            button: Style::default().fg(Color::DarkGray),
            button_active: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            folder: Style::default().fg(Color::Blue),
            repo: Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            badge: Style::default().fg(Color::Green),
            selected: Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD),
            matched: Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            path: Style::default().fg(Color::Gray),
        }
    }
}
