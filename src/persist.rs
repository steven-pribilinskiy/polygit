use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::{ColumnFlags, HelpTab, IconStyle, SortColumn, SortDir};

/// UI preferences persisted between runs at `~/.config/pull-all/state.json`.
/// `#[serde(default)]` keeps older state files (missing newer fields) loadable.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedState {
    pub columns: ColumnFlags,
    /// The info block (`i`) was shown on last exit.
    pub info_pinned: bool,
    /// Left/right splitter position.
    pub split_ratio: f64,
    /// 1-cell padding inside every bordered panel/modal.
    pub panel_padding: bool,
    /// Glyph set (Unicode vs emoji).
    pub icon_style: IconStyle,
    /// Repo-list sort column (default: discovery order).
    pub sort_column: SortColumn,
    /// Repo-list sort direction.
    pub sort_dir: SortDir,
    /// Last-active help-modal tab.
    pub help_tab: HelpTab,
}

fn state_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("pull-all").join("state.json"))
}

/// Load persisted UI state, falling back to defaults on any error.
pub fn load() -> PersistedState {
    let Some(path) = state_path() else {
        return PersistedState::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => PersistedState::default(),
    }
}

/// Persist UI state, best-effort (errors are ignored).
pub fn save(state: &PersistedState) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, contents);
    }
}
