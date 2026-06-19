//! Reusable ratatui selection widgets, extracted from polygit so other Rust CLI tools can share
//! them:
//!
//! - [`finder`] — a fzf-style fuzzy finder overlay (prompt, `matched/total`, highlighted matches,
//!   sort cycling, a keybinding header).
//! - [`picker`] — a filesystem folder / git-repo picker (breadcrumbs, home, bookmarks, up/back,
//!   folder↔git toggle, git-repo badges, current-path).
//! - [`ranking`] — the **goto-repo-compatible** usage-history store (`~/.config/goto-repo/history`,
//!   lines `epoch\tpath`) + sort modes (relevance / name / recent / most-used).
//! - [`modal`] — self-contained modal helpers (centering, shadow, close button, hint footer).
//! - [`style`] — themeable [`FinderStyle`]/[`PickerStyle`]; defaults use **semantic ANSI colors**
//!   so a host that remaps the frame buffer (like polygit) themes the widgets for free.
//!
//! The widgets own small state structs and pure `render`/`handle_*` functions — they never depend
//! on a host's god-object, so they drop into any ratatui app.

pub mod finder;
pub mod modal;
pub mod picker;
pub mod ranking;
pub mod style;

pub use modal::{HintClick, HintKey};
pub use ranking::{History, SortMode};
pub use style::{FinderStyle, PickerStyle};
