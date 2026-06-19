//! The **goto-repo-compatible** usage store. Canonical format: an append-only file
//! (`~/.config/goto-repo/history` by default) with one line per visit, `<unix-epoch>\t<path>`.
//! From it we derive, per path, a visit `count` and a `last_used` timestamp — the inputs to the
//! `recent` and `most-used` sort modes. This is the same file `goto-repo`/`rank-repos.sh` use, so
//! visits are shared across tools.

use std::collections::HashMap;
use std::path::PathBuf;

/// Sort order for the finder. `Relevance` ranks by fuzzy score; the rest mirror goto-repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Relevance,
    Name,
    Recent,
    MostUsed,
}

impl SortMode {
    /// Cycle relevance → name → recent → most-used → relevance (mirrors goto-repo's `^S`).
    pub fn cycle(self) -> Self {
        match self {
            SortMode::Relevance => SortMode::Name,
            SortMode::Name => SortMode::Recent,
            SortMode::Recent => SortMode::MostUsed,
            SortMode::MostUsed => SortMode::Relevance,
        }
    }

    /// Short label for the header (`^S:sort(<label>)`).
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Relevance => "relevance",
            SortMode::Name => "name",
            SortMode::Recent => "recent",
            SortMode::MostUsed => "most-used",
        }
    }
}

/// The default canonical history path, `~/.config/goto-repo/history`.
pub fn goto_repo_history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("goto-repo")
        .join("history")
}

/// Per-path usage derived from the goto-repo history file.
#[derive(Debug, Clone, Default)]
pub struct History {
    /// path → (visit count, last-used unix epoch).
    stats: HashMap<String, (u32, i64)>,
    /// The file the history is read from + appended to.
    path: PathBuf,
}

impl History {
    /// Load (and remember) a history file. A missing/unreadable file yields an empty store that
    /// `record_use` will create on first write.
    pub fn load(path: PathBuf) -> Self {
        let mut stats: HashMap<String, (u32, i64)> = HashMap::new();
        if let Ok(contents) = std::fs::read_to_string(&path) {
            for line in contents.lines() {
                let mut parts = line.splitn(2, '\t');
                let (Some(epoch), Some(entry)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let epoch: i64 = epoch.trim().parse().unwrap_or(0);
                let stat = stats.entry(entry.to_string()).or_insert((0, 0));
                stat.0 += 1;
                if epoch > stat.1 {
                    stat.1 = epoch;
                }
            }
        }
        History { stats, path }
    }

    /// Load the canonical goto-repo history.
    pub fn load_default() -> Self {
        Self::load(goto_repo_history_path())
    }

    /// Visit count for a path (0 if never recorded).
    pub fn count(&self, path: &str) -> u32 {
        self.stats.get(path).map(|stat| stat.0).unwrap_or(0)
    }

    /// Last-used unix epoch for a path (0 if never recorded).
    pub fn last_used(&self, path: &str) -> i64 {
        self.stats.get(path).map(|stat| stat.1).unwrap_or(0)
    }

    /// Record a visit: append `<epoch>\t<path>` to the canonical file (best-effort) and update the
    /// in-memory tallies, so the same file feeds `goto-repo` and a subsequent `recent`/`most-used`.
    pub fn record_use(&mut self, path: &str) {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|dur| dur.as_secs() as i64)
            .unwrap_or(0);
        let stat = self.stats.entry(path.to_string()).or_insert((0, 0));
        stat.0 += 1;
        stat.1 = epoch;
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        use std::io::Write;
        if let Ok(mut file) =
            std::fs::OpenOptions::new().create(true).append(true).open(&self.path)
        {
            let _ = writeln!(file, "{epoch}\t{path}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_counts_and_recency() {
        let dir = std::env::temp_dir().join(format!("tui-pick-hist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("history");
        std::fs::write(&file, "100\t/a\n200\t/a\n150\t/b\n").unwrap();
        let history = History::load(file.clone());
        assert_eq!(history.count("/a"), 2);
        assert_eq!(history.last_used("/a"), 200);
        assert_eq!(history.count("/b"), 1);
        assert_eq!(history.count("/missing"), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sort_mode_cycles() {
        assert_eq!(SortMode::Relevance.cycle(), SortMode::Name);
        assert_eq!(SortMode::MostUsed.cycle(), SortMode::Relevance);
    }
}
