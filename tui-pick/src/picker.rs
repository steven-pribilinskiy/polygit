//! The filesystem folder / git-repo picker dialog (breadcrumbs, home, bookmarks, up/back,
//! folder↔git toggle, git-repo badges, current-path). State + render + handlers land here with the
//! host integration.

use std::path::{Path, PathBuf};

/// One filesystem entry shown in the picker list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_git_repo: bool,
}

/// Read the immediate sub-directories of `dir`, flagging which are git repos, sorted by name.
/// Hidden dirs are kept (a leading `.` sorts first), matching a file-browser's behavior.
pub fn read_dir_entries(dir: &Path) -> std::io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let is_git_repo = path.join(".git").exists();
        entries.push(Entry { name, path, is_dir: true, is_git_repo });
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

/// Whether `dir` is itself a git repo (selecting it adds a single-repo root).
pub fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}
