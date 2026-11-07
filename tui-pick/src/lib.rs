//! Reusable ratatui widgets, extracted from polygit so other Rust CLI tools can share them:
//!
//! - [`tooltip`] — a floating-element positioning engine (placement + flip + shift), à la floating-ui.
//! - [`components`] — presentational primitives (buttons, list rows, radios) drawn in every
//!   interaction state, for a consistent widget vocabulary across tools.
//!
//! The fuzzy finder, the folder picker and the goto-repo usage history that used to live here moved
//! to tuilith's `find` component (`tuilith::find`), so one copy exists; polygit uses it from there.

pub mod components;
pub mod tooltip;

pub use tooltip::{Align, Placement, PositionOptions, Positioned, Side, position};
