//! Persisted PR cache (`~/.config/polygit/pr-cache.json`). Maps a repo+branch to its last-resolved
//! PR (via `gh`, open/merged/closed), with a per-entry timestamp and a 5-minute TTL, so the Pull
//! Request column and the info panel don't re-hit the network every frame — or on every launch
//! within the window. A cached `pr: None` is a valid result (no PR) and is honored for the TTL, so
//! a PR-less branch isn't re-queried each frame either.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::PrInfo;

/// How long a resolved PR entry stays fresh before a re-query (seconds).
pub const PR_TTL_SECS: i64 = 300;

/// One repo+branch's cached PR lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrCacheEntry {
    /// `None` = resolved, no PR (a real, cacheable answer).
    #[serde(default)]
    pub pr: Option<PrInfo>,
    /// Unix seconds when this entry was resolved — drives the TTL + the "… ago" age.
    pub checked_at: i64,
}

/// The whole cache: `"{abs_repo_path}\u{1f}{branch}"` → entry.
pub type PrCache = HashMap<String, PrCacheEntry>;

/// Cache key for a repo path + branch. Branch in the key means a branch switch is a natural miss.
pub fn key(path: &Path, branch: &str) -> String {
    format!("{}\u{1f}{branch}", path.display())
}

/// Whether `checked_at` is still within the TTL relative to `now` (both unix seconds).
pub fn is_fresh(checked_at: i64, now: i64) -> bool {
    now - checked_at < PR_TTL_SECS
}

fn cache_path() -> Option<PathBuf> {
    Some(crate::persist::config_dir()?.join("pr-cache.json"))
}

/// Load the PR cache. A missing/corrupt file yields an empty cache.
pub fn load() -> PrCache {
    cache_path()
        .and_then(|path| std::fs::read_to_string(&path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Persist the PR cache, best-effort (errors ignored). No-op under test so unit tests can't
/// clobber the real cache.
#[cfg_attr(test, allow(dead_code))]
pub fn save(cache: &PrCache) {
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
    fn is_fresh_honors_the_ttl_boundary() {
        let now = 1_000_000;
        assert!(is_fresh(now, now)); // just resolved
        assert!(is_fresh(now - (PR_TTL_SECS - 1), now)); // within window
        assert!(!is_fresh(now - PR_TTL_SECS, now)); // exactly at TTL → stale
        assert!(!is_fresh(now - 10_000, now)); // long stale
    }

    #[test]
    fn key_includes_branch() {
        let path = Path::new("/repos/a");
        assert_ne!(key(path, "main"), key(path, "feature/x"));
        assert_eq!(key(path, "main"), "/repos/a\u{1f}main");
    }

    #[test]
    fn entry_round_trips_with_and_without_pr() {
        let mut cache = PrCache::new();
        cache.insert(
            key(Path::new("/repos/a"), "main"),
            PrCacheEntry {
                pr: Some(PrInfo {
                    number: 42,
                    title: "fix".into(),
                    url: "https://x/pull/42".into(),
                    state: crate::app::PrState::Open,
                    base_ref: "main".into(),
                }),
                checked_at: 123,
            },
        );
        cache.insert(
            key(Path::new("/repos/b"), "main"),
            PrCacheEntry { pr: None, checked_at: 456 },
        );
        let json = serde_json::to_string(&cache).unwrap();
        let back: PrCache = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[&key(Path::new("/repos/a"), "main")].pr.as_ref().unwrap().number, 42);
        assert!(back[&key(Path::new("/repos/b"), "main")].pr.is_none());
    }
}
