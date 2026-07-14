//! Persisted email→GitHub-username cache (`~/.config/polygit/author-cache.json`). A commit
//! author's real GitHub login can only be resolved from a non-noreply email via a `gh api` call
//! (GitHub does the email→account matching); caching the result means it's paid once per email,
//! ever after. `None` = confirmed no GitHub account matches this email — still worth caching, so a
//! dead-end email isn't re-queried on every click.

use std::collections::HashMap;
use std::path::PathBuf;

pub type AuthorCache = HashMap<String, Option<String>>;

fn cache_path() -> Option<PathBuf> {
    Some(crate::persist::config_dir()?.join("author-cache.json"))
}

/// Load the author cache. A missing/corrupt file yields an empty cache.
pub fn load() -> AuthorCache {
    cache_path()
        .and_then(|path| std::fs::read_to_string(&path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Persist the author cache, best-effort (errors ignored). No-op under test so unit tests can't
/// clobber the real cache.
#[cfg_attr(test, allow(dead_code))]
pub fn save(cache: &AuthorCache) {
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
