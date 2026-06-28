//! In-terminal keyboard viewer data. Parses the SAME `keymap.json` the docs use (so the two can't
//! drift), builds a physical-key → actions map, and defines the keyboard layout. A pressed key is
//! resolved to a layout `code`; the map yields every action that key participates in.

use std::collections::HashMap;
use std::sync::OnceLock;

use crossterm::event::{KeyCode, KeyModifiers, ModifierKeyCode};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Binding {
    pub keys: Vec<String>,
    pub action: String,
    /// Optional clarifying note (shown dim after the action in the help list).
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Section {
    /// Stable id (`list` / `page` / `diff` / `pr`) — what the help tab matches the current view on.
    pub id: String,
    pub title: String,
    pub bindings: Vec<Binding>,
}

/// The keybinding sections, shared verbatim with the docs (`docs/src/data/keymap.json`).
pub fn sections() -> &'static [Section] {
    static SECTIONS: OnceLock<Vec<Section>> = OnceLock::new();
    SECTIONS.get_or_init(|| {
        let json = include_str!("../docs/src/data/keymap.json");
        serde_json::from_str(json).unwrap_or_default()
    })
}

/// One action a physical key participates in, with the human-readable key combo + which section.
/// `shift`/`ctrl`/`alt` are the modifiers the combo requires, so the keyboard viewer can filter to
/// the exact chord the user pressed (e.g. `Shift+G` vs plain `g`).
#[derive(Debug, Clone)]
pub struct KeyUse {
    pub combo: String,
    pub action: String,
    pub section: &'static str,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// physical-key `code` → the actions it's part of. Built once from `sections()`.
pub fn key_uses() -> &'static HashMap<&'static str, Vec<KeyUse>> {
    static MAP: OnceLock<HashMap<&'static str, Vec<KeyUse>>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map: HashMap<&'static str, Vec<KeyUse>> = HashMap::new();
        for section in sections() {
            let title: &'static str = section_label(&section.title);
            for binding in &section.bindings {
                let tokens: Vec<&String> =
                    binding.keys.iter().filter(|key| !is_mouse(key)).collect();
                if tokens.is_empty() {
                    continue;
                }
                let combo = tokens.iter().map(|key| key.as_str()).collect::<Vec<_>>().join(" ");
                // `Ctrl`/`Alt` are binding-level tokens; Shift is decided PER KEY — an UPPERCASE
                // letter (or `?`/`+`/`*`) implies Shift, but only with no Ctrl/Alt (the uppercase `C`
                // in `Ctrl C` is just the C key, not Ctrl+Shift+C). Computing Shift per token means a
                // mixed binding like `g G Home End` registers `g` as no-shift and `G` as Shift — so a
                // Shift filter shows only the keys that truly need Shift.
                let ctrl = tokens.iter().any(|token| token.as_str() == "Ctrl");
                let alt = tokens.iter().any(|token| token.as_str() == "Alt");
                let explicit_shift =
                    tokens.iter().any(|token| token.as_str() == "Shift" || token.contains("Shift"));
                let mut any_shift = false;
                for token in &tokens {
                    if let Some((code, token_shift)) = token_to_code(token) {
                        let shift = explicit_shift || (!ctrl && !alt && token_shift);
                        any_shift |= shift;
                        push(&mut map, code, &combo, &binding.action, title, shift, ctrl, alt);
                    }
                }
                if any_shift {
                    push(&mut map, "ShiftLeft", &combo, &binding.action, title, true, ctrl, alt);
                }
                if ctrl {
                    push(&mut map, "ControlLeft", &combo, &binding.action, title, explicit_shift, true, alt);
                }
                if alt {
                    push(&mut map, "AltLeft", &combo, &binding.action, title, explicit_shift, ctrl, true);
                }
            }
        }
        map
    })
}

#[allow(clippy::too_many_arguments)]
fn push(
    map: &mut HashMap<&'static str, Vec<KeyUse>>,
    code: &'static str,
    combo: &str,
    action: &str,
    section: &'static str,
    shift: bool,
    ctrl: bool,
    alt: bool,
) {
    let entry = map.entry(code).or_default();
    if !entry.iter().any(|use_| use_.combo == combo && use_.action == action) {
        entry.push(KeyUse {
            combo: combo.to_string(),
            action: action.to_string(),
            section,
            shift,
            ctrl,
            alt,
        });
    }
}

/// Stable &'static label for a section title (so KeyUse can hold &'static str).
fn section_label(title: &str) -> &'static str {
    match title {
        "Repo page" => "Repo page",
        "Diff modal" => "Diff modal",
        _ => "Repo list",
    }
}

fn is_mouse(token: &str) -> bool {
    matches!(token, "click" | "double-click" | "wheel")
}

/// Map a keymap.json token to a layout `code` (+ whether Shift is required).
fn token_to_code(token: &str) -> Option<(&'static str, bool)> {
    let named = match token {
        "↑" => "ArrowUp",
        "↓" => "ArrowDown",
        "←" => "ArrowLeft",
        "→" => "ArrowRight",
        "Space" => "Space",
        "Enter" => "Enter",
        "Esc" => "Escape",
        "Tab" => "Tab",
        "Home" => "Home",
        "End" => "End",
        "PgUp" => "PageUp",
        "PgDn" => "PageDown",
        "Ctrl" => "ControlLeft",
        "Shift" => "ShiftLeft",
        "[" => "BracketLeft",
        "]" => "BracketRight",
        "/" => "Slash",
        "," => "Comma",
        "." => "Period",
        ";" => "Semicolon",
        "-" => "Minus",
        "=" => "Equal",
        _ => "",
    };
    if !named.is_empty() {
        return Some((named, false));
    }
    match token {
        "?" => return Some(("Slash", true)),
        "+" => return Some(("Equal", true)),
        "*" => return Some(("Digit8", true)),
        _ => {}
    }
    let chars: Vec<char> = token.chars().collect();
    if chars.len() == 1 {
        let ch = chars[0];
        if ch.is_ascii_lowercase() {
            return Some((letter_code(ch.to_ascii_uppercase()), false));
        }
        if ch.is_ascii_uppercase() {
            return Some((letter_code(ch), true));
        }
        if ch.is_ascii_digit() {
            return Some((digit_code(ch), false));
        }
    }
    None
}

/// Resolve a crossterm key press to a layout `code` (so pressing a key selects it on the board).
pub fn keycode_to_code(code: KeyCode, mods: KeyModifiers) -> Option<&'static str> {
    let resolved = match code {
        KeyCode::Up => "ArrowUp",
        KeyCode::Down => "ArrowDown",
        KeyCode::Left => "ArrowLeft",
        KeyCode::Right => "ArrowRight",
        KeyCode::Enter => "Enter",
        KeyCode::Tab | KeyCode::BackTab => "Tab",
        KeyCode::Home => "Home",
        KeyCode::End => "End",
        KeyCode::PageUp => "PageUp",
        KeyCode::PageDown => "PageDown",
        KeyCode::Backspace => "Backspace",
        KeyCode::CapsLock => "CapsLock",
        // A bare modifier press (needs the kitty REPORT_ALL_KEYS flag) arrives as its own KeyCode.
        KeyCode::Modifier(modifier) => return modifier_to_code(modifier),
        KeyCode::Char(ch) => return Some(char_to_code(ch)),
        _ => return None,
    };
    // On terminals without REPORT_ALL_KEYS, a Ctrl press only shows up as a modifier on the next
    // key; map it so Ctrl+<key> still lights up the Ctrl cell.
    if mods.contains(KeyModifiers::CONTROL) {
        return Some("ControlLeft");
    }
    Some(resolved)
}

/// Map a bare modifier key press to its layout `code`.
fn modifier_to_code(modifier: ModifierKeyCode) -> Option<&'static str> {
    match modifier {
        ModifierKeyCode::LeftShift => Some("ShiftLeft"),
        ModifierKeyCode::RightShift => Some("ShiftRight"),
        ModifierKeyCode::LeftControl => Some("ControlLeft"),
        ModifierKeyCode::RightControl => Some("ControlRight"),
        ModifierKeyCode::LeftAlt => Some("AltLeft"),
        ModifierKeyCode::RightAlt => Some("AltRight"),
        ModifierKeyCode::LeftSuper | ModifierKeyCode::LeftMeta => Some("MetaLeft"),
        ModifierKeyCode::RightSuper | ModifierKeyCode::RightMeta => Some("MetaRight"),
        _ => None,
    }
}

fn char_to_code(ch: char) -> &'static str {
    match ch {
        ' ' => "Space",
        '`' | '~' => "Backquote",
        '-' | '_' => "Minus",
        '=' | '+' => "Equal",
        '[' | '{' => "BracketLeft",
        ']' | '}' => "BracketRight",
        '\\' | '|' => "Backslash",
        ';' | ':' => "Semicolon",
        '\'' | '"' => "Quote",
        ',' | '<' => "Comma",
        '.' | '>' => "Period",
        '/' | '?' => "Slash",
        '8' | '*' => "Digit8",
        _ if ch.is_ascii_digit() => digit_code(ch),
        _ if ch.is_ascii_alphabetic() => letter_code(ch.to_ascii_uppercase()),
        _ => "",
    }
}

fn letter_code(upper: char) -> &'static str {
    match upper {
        'A' => "KeyA", 'B' => "KeyB", 'C' => "KeyC", 'D' => "KeyD", 'E' => "KeyE", 'F' => "KeyF",
        'G' => "KeyG", 'H' => "KeyH", 'I' => "KeyI", 'J' => "KeyJ", 'K' => "KeyK", 'L' => "KeyL",
        'M' => "KeyM", 'N' => "KeyN", 'O' => "KeyO", 'P' => "KeyP", 'Q' => "KeyQ", 'R' => "KeyR",
        'S' => "KeyS", 'T' => "KeyT", 'U' => "KeyU", 'V' => "KeyV", 'W' => "KeyW", 'X' => "KeyX",
        'Y' => "KeyY", 'Z' => "KeyZ",
        _ => "",
    }
}

fn digit_code(ch: char) -> &'static str {
    match ch {
        '0' => "Digit0", '1' => "Digit1", '2' => "Digit2", '3' => "Digit3", '4' => "Digit4",
        '5' => "Digit5", '6' => "Digit6", '7' => "Digit7", '8' => "Digit8", '9' => "Digit9",
        _ => "",
    }
}

/// One key in the rendered layout: its `code`, its label, and width in cells.
pub struct KeyDef {
    pub code: &'static str,
    pub label: &'static str,
    pub width: u16,
}

const fn key(code: &'static str, label: &'static str, width: u16) -> KeyDef {
    KeyDef { code, label, width }
}

/// The main keyboard rows, mirroring the docs. `os` selects the bottom modifier row labels.
/// The renderer pairs these with `cluster()` (the nav/arrow block) for the full board.
pub fn layout(os: Os) -> Vec<Vec<KeyDef>> {
    let mut rows = vec![
        vec![
            key("Escape", "esc", 5), key("Backquote", "`", 3), key("Digit1", "1", 3), key("Digit2", "2", 3), key("Digit3", "3", 3), key("Digit4", "4", 3), key("Digit5", "5", 3), key("Digit6", "6", 3), key("Digit7", "7", 3), key("Digit8", "8", 3), key("Digit9", "9", 3), key("Digit0", "0", 3), key("Minus", "-", 3), key("Equal", "=", 3), key("Backspace", "⌫", 4),
        ],
        vec![
            key("Tab", "tab", 5), key("KeyQ", "q", 3), key("KeyW", "w", 3), key("KeyE", "e", 3), key("KeyR", "r", 3), key("KeyT", "t", 3), key("KeyY", "y", 3), key("KeyU", "u", 3), key("KeyI", "i", 3), key("KeyO", "o", 3), key("KeyP", "p", 3), key("BracketLeft", "[", 3), key("BracketRight", "]", 3), key("Backslash", "\\", 4),
        ],
        vec![
            key("CapsLock", "caps", 6), key("KeyA", "a", 3), key("KeyS", "s", 3), key("KeyD", "d", 3), key("KeyF", "f", 3), key("KeyG", "g", 3), key("KeyH", "h", 3), key("KeyJ", "j", 3), key("KeyK", "k", 3), key("KeyL", "l", 3), key("Semicolon", ";", 3), key("Quote", "'", 3), key("Enter", "⏎", 6),
        ],
        vec![
            key("ShiftLeft", "⇧", 6), key("KeyZ", "z", 3), key("KeyX", "x", 3), key("KeyC", "c", 3), key("KeyV", "v", 3), key("KeyB", "b", 3), key("KeyN", "n", 3), key("KeyM", "m", 3), key("Comma", ",", 3), key("Period", ".", 3), key("Slash", "/", 3), key("ShiftRight", "⇧", 7),
        ],
    ];
    rows.push(match os {
        Os::Mac => vec![
            key("ControlLeft", "⌃", 5), key("AltLeft", "⌥", 4), key("MetaLeft", "⌘", 5), key("Space", "", 18), key("MetaRight", "⌘", 5), key("AltRight", "⌥", 4), key("ControlRight", "⌃", 5),
        ],
        Os::Windows => vec![
            key("ControlLeft", "ctrl", 5), key("MetaLeft", "⊞", 4), key("AltLeft", "alt", 5), key("Space", "", 18), key("AltRight", "alt", 5), key("MetaRight", "⊞", 4), key("ControlRight", "ctrl", 5),
        ],
        Os::Linux => vec![
            key("ControlLeft", "ctrl", 5), key("MetaLeft", "super", 6), key("AltLeft", "alt", 5), key("Space", "", 16), key("AltRight", "alt", 5), key("MetaRight", "super", 6), key("ControlRight", "ctrl", 5),
        ],
    });
    rows
}

/// The nav + arrow cluster shown to the right of the main block.
pub fn cluster() -> Vec<Vec<KeyDef>> {
    vec![
        vec![key("Home", "home", 6), key("PageUp", "pgup", 6)],
        vec![key("End", "end", 6), key("PageDown", "pgdn", 6)],
        vec![key("__gap", "", 6), key("ArrowUp", "↑", 6)],
        vec![key("ArrowLeft", "←", 6), key("ArrowDown", "↓", 6), key("ArrowRight", "→", 6)],
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    /// The OS polygit is running on.
    pub fn current() -> Os {
        match std::env::consts::OS {
            "macos" => Os::Mac,
            "windows" => Os::Windows,
            _ => Os::Linux,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_modifier_presses_resolve_to_layout_codes() {
        let none = KeyModifiers::NONE;
        assert_eq!(keycode_to_code(KeyCode::Modifier(ModifierKeyCode::LeftShift), none), Some("ShiftLeft"));
        assert_eq!(keycode_to_code(KeyCode::Modifier(ModifierKeyCode::RightShift), none), Some("ShiftRight"));
        assert_eq!(keycode_to_code(KeyCode::Modifier(ModifierKeyCode::LeftControl), none), Some("ControlLeft"));
        assert_eq!(keycode_to_code(KeyCode::Modifier(ModifierKeyCode::RightControl), none), Some("ControlRight"));
        assert_eq!(keycode_to_code(KeyCode::Modifier(ModifierKeyCode::LeftAlt), none), Some("AltLeft"));
        assert_eq!(keycode_to_code(KeyCode::Modifier(ModifierKeyCode::LeftSuper), none), Some("MetaLeft"));
        assert_eq!(keycode_to_code(KeyCode::CapsLock, none), Some("CapsLock"));
    }

    #[test]
    fn ctrl_held_lights_the_ctrl_cell() {
        // Fallback for terminals without REPORT_ALL_KEYS: a Ctrl-modified named key lights Ctrl.
        assert_eq!(keycode_to_code(KeyCode::Up, KeyModifiers::CONTROL), Some("ControlLeft"));
    }

    #[test]
    fn every_key_use_resolves_to_a_layout_code() {
        // Guard against a keymap.json token that the renderer can't place on the board.
        let codes: std::collections::HashSet<&str> = layout(Os::Linux)
            .iter()
            .chain(cluster().iter())
            .flatten()
            .map(|key| key.code)
            .collect();
        for code in key_uses().keys() {
            assert!(codes.contains(code), "key use {code} has no cell in the layout");
        }
    }
}
