//! Persisted per-repo status cache (`~/.config/polygit/status-cache.json`). Lets the list show
//! each repo's last-known state instantly on launch — without any git work — so the dashboard is
//! useful before (or without) pulling. Keyed by absolute repo path. Only terminal statuses are
//! cached; `Queued`/`Running` are transient and never written.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::{PullResult, RepoDetails, RepoStatus};

/// A cached terminal pull status. Mirrors the terminal `RepoStatus` variants only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheStatus {
    UpToDate,
    Updated,
    NoUpstream,
    Skipped,
    Throttled,
    Failed,
}

impl CacheStatus {
    /// The cacheable status for a repo, or `None` for the transient `Queued`/`Running` states.
    pub fn from_status(status: &RepoStatus) -> Option<Self> {
        Some(match status {
            RepoStatus::UpToDate => Self::UpToDate,
            RepoStatus::Updated => Self::Updated,
            RepoStatus::NoUpstream => Self::NoUpstream,
            RepoStatus::Skipped => Self::Skipped,
            RepoStatus::Throttled => Self::Throttled,
            RepoStatus::Failed => Self::Failed,
            RepoStatus::Queued | RepoStatus::Running { .. } => return None,
        })
    }

    pub fn to_status(self) -> RepoStatus {
        match self {
            Self::UpToDate => RepoStatus::UpToDate,
            Self::Updated => RepoStatus::Updated,
            Self::NoUpstream => RepoStatus::NoUpstream,
            Self::Skipped => RepoStatus::Skipped,
            Self::Throttled => RepoStatus::Throttled,
            Self::Failed => RepoStatus::Failed,
        }
    }
}

/// One repo's cached last-known state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedRepo {
    pub status: CacheStatus,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub details: Option<RepoDetails>,
    #[serde(default)]
    pub pull_result: Option<PullResult>,
    /// Unix seconds when this entry was written — drives the "… ago" staleness age.
    pub updated_at: i64,
}

/// The whole cache: absolute repo path → last-known state.
pub type StatusCache = HashMap<PathBuf, CachedRepo>;

fn cache_path() -> Option<PathBuf> {
    Some(crate::persist::config_dir()?.join("status-cache.json"))
}

/// Load the status cache. A missing/corrupt file yields an empty cache.
pub fn load() -> StatusCache {
    cache_path()
        .and_then(|path| std::fs::read_to_string(&path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Persist the status cache, best-effort (errors ignored). No-op under test so unit tests can't
/// clobber the real cache.
#[cfg_attr(test, allow(dead_code))]
pub fn save(cache: &StatusCache) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(contents) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(&path, contents);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_status_round_trips_terminal_states() {
        for status in [
            RepoStatus::UpToDate,
            RepoStatus::Updated,
            RepoStatus::NoUpstream,
            RepoStatus::Skipped,
            RepoStatus::Throttled,
            RepoStatus::Failed,
        ] {
            let cached = CacheStatus::from_status(&status).expect("terminal status caches");
            assert_eq!(cached.to_status(), status);
        }
        assert!(CacheStatus::from_status(&RepoStatus::Queued).is_none());
        assert!(CacheStatus::from_status(&RepoStatus::Running { pid: 1 }).is_none());
    }

    #[test]
    fn cached_repo_json_round_trips() {
        let mut cache: StatusCache = HashMap::new();
        cache.insert(
            PathBuf::from("/repos/app"),
            CachedRepo {
                status: CacheStatus::Updated,
                branch: Some("main".to_string()),
                details: Some(RepoDetails { ahead: Some(0), behind: Some(0), ..Default::default() }),
                pull_result: None,
                updated_at: 1_700_000_000,
            },
        );
        let json = serde_json::to_string(&cache).unwrap();
        let back: StatusCache = serde_json::from_str(&json).unwrap();
        let entry = back.get(&PathBuf::from("/repos/app")).unwrap();
        assert_eq!(entry.status, CacheStatus::Updated);
        assert_eq!(entry.branch.as_deref(), Some("main"));
        assert_eq!(entry.updated_at, 1_700_000_000);
    }
}
