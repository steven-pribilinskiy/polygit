//! Moving a repo — and everything that points at it.
//!
//! A repo path is referenced by far more than the repo: its own worktrees, this tool's caches, and
//! a long tail of other tools' state. polygit's remit stops at a deliberate line. It moves the
//! subtree, repairs git, rewrites **its own** state, and *reports* every foreign reference it can
//! see into a handoff manifest. It does not edit another tool's files: doing that silently, across
//! config directories it does not own, is where an unrecoverable mistake lives.
//!
//! The sharpest gate here is not a filesystem one. `~/.gitconfig` can switch commit identity by
//! path via `includeIf "gitdir:…"`, so moving a repo across such a boundary changes its author
//! email with no error and no output — the only breakage in the whole list that changes what you
//! *commit*. It blocks a move unless explicitly acknowledged.

use std::path::{Path, PathBuf};

/// A repo and the worktrees that must travel with it. `<repo>.worktrees/` is keyed to the repo's
/// directory name, so the two are one unit — moving them separately leaves a window where the
/// worktrees are orphaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveUnit {
    pub from: PathBuf,
    pub to: PathBuf,
    /// Worktree checkouts, as `git worktree list --porcelain` reports them, with their new paths.
    pub worktrees: Vec<WorktreeMove>,
}

impl MoveUnit {
    /// The `<repo>.worktrees` sibling directory for a repo path.
    pub fn worktrees_dir(repo: &Path) -> PathBuf {
        let name = repo.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
        repo.with_file_name(format!("{name}.worktrees"))
    }

    pub fn worktrees_from(&self) -> PathBuf {
        Self::worktrees_dir(&self.from)
    }

    pub fn worktrees_to(&self) -> PathBuf {
        Self::worktrees_dir(&self.to)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeMove {
    pub from: PathBuf,
    pub to: PathBuf,
    pub locked: bool,
}

/// Why a move will not be attempted. Every one of these is checked before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    /// A process has its working directory inside the subtree.
    InUse(Vec<i32>),
    /// Source and destination are on different filesystems, so the move is a copy plus a delete:
    /// not atomic, and every open file handle is left pointing at a deleted file.
    CrossDevice,
    DestinationOccupied,
    /// A `git worktree lock` marker. Locked worktrees are deliberate; report and leave them.
    LockedWorktree(PathBuf),
    /// The move would change which `includeIf` section of the git config applies, and therefore
    /// the commit author email.
    IdentityChange { from: String, to: String },
}

impl Blocker {
    pub fn label(&self) -> String {
        match self {
            Blocker::InUse(pids) => {
                let list: Vec<String> = pids.iter().map(i32::to_string).collect();
                format!("in use by pid {}", list.join(", "))
            }
            Blocker::CrossDevice => "destination is on another filesystem".to_string(),
            Blocker::DestinationOccupied => "destination already exists".to_string(),
            Blocker::LockedWorktree(path) => format!("locked worktree at {}", path.display()),
            Blocker::IdentityChange { from, to } => {
                format!("commit identity would change: {from} -> {to}")
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Path rewriting
// ---------------------------------------------------------------------------------------------

/// Rewrite `path` when it is `old` or sits under it. Compares whole path components, so
/// `/a/repo-two` is untouched by a move of `/a/repo`.
pub fn rewrite_prefix(path: &Path, old: &Path, new: &Path) -> Option<PathBuf> {
    if path == old {
        return Some(new.to_path_buf());
    }
    path.strip_prefix(old).ok().map(|rest| new.join(rest))
}

/// The same rewrite over a string path, for the many stores that key by a rendered path.
pub fn rewrite_prefix_str(value: &str, old: &Path, new: &Path) -> Option<String> {
    rewrite_prefix(Path::new(value), old, new).map(|path| path.display().to_string())
}

// ---------------------------------------------------------------------------------------------
// The identity boundary
// ---------------------------------------------------------------------------------------------

/// One `includeIf "gitdir:<pattern>"` from a git config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitdirInclude {
    pub pattern: String,
    pub path: String,
    /// `gitdir/i` — case-insensitive.
    pub case_insensitive: bool,
}

/// Pull the `includeIf "gitdir:…"` sections out of a git config file's text.
pub fn parse_gitdir_includes(config: &str, home: &Path) -> Vec<GitdirInclude> {
    let mut out = Vec::new();
    let mut current: Option<(String, bool)> = None;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            current = None;
            let inner = trimmed.trim_start_matches('[').trim_end_matches(']');
            if let Some(rest) = inner.strip_prefix("includeIf ") {
                let spec = rest.trim().trim_matches('"');
                let (keyword, pattern) = match spec.split_once(':') {
                    Some(pair) => pair,
                    None => continue,
                };
                let insensitive = keyword.eq_ignore_ascii_case("gitdir/i");
                if keyword.eq_ignore_ascii_case("gitdir") || insensitive {
                    current = Some((expand_home(pattern, home), insensitive));
                }
            }
            continue;
        }
        if let Some((pattern, insensitive)) = current.clone()
            && let Some((key, value)) = trimmed.split_once('=')
            && key.trim().eq_ignore_ascii_case("path")
        {
            out.push(GitdirInclude {
                pattern,
                path: expand_home(value.trim(), home),
                case_insensitive: insensitive,
            });
            current = None;
        }
    }
    out
}

fn expand_home(value: &str, home: &Path) -> String {
    match value.strip_prefix("~/") {
        Some(rest) => home.join(rest).display().to_string(),
        None => value.to_string(),
    }
}

/// Whether a `gitdir:` pattern covers a repo path, following git's own rules: a pattern ending in
/// `/` matches everything beneath it, and one that is neither absolute nor `**`-anchored matches at
/// any depth.
pub fn gitdir_matches(pattern: &str, repo: &Path, case_insensitive: bool) -> bool {
    let mut pattern = pattern.to_string();
    if pattern.ends_with('/') {
        pattern.push_str("**");
    }
    if !pattern.starts_with('/') && !pattern.starts_with("**") {
        pattern = format!("**/{pattern}");
    }
    let subject = format!("{}/", repo.display());
    let (pattern, subject) = if case_insensitive {
        (pattern.to_lowercase(), subject.to_lowercase())
    } else {
        (pattern, subject)
    };
    // `**` spans separators; the rest is a plain prefix comparison after that expansion.
    match pattern.strip_prefix("**/") {
        Some(rest) => {
            let rest = rest.trim_end_matches("**");
            subject.contains(rest)
        }
        None => {
            let rest = pattern.trim_end_matches("**");
            subject.starts_with(rest)
        }
    }
}

/// The include files that apply to a repo, in config order.
pub fn applicable_includes(includes: &[GitdirInclude], repo: &Path) -> Vec<String> {
    includes
        .iter()
        .filter(|include| gitdir_matches(&include.pattern, repo, include.case_insensitive))
        .map(|include| include.path.clone())
        .collect()
}

/// Whether moving from `old` to `new` changes which identity config applies. Returns the two
/// descriptions when it does, so the message can say what would change rather than just refusing.
pub fn identity_change(
    includes: &[GitdirInclude],
    old: &Path,
    new: &Path,
) -> Option<(String, String)> {
    let before = applicable_includes(includes, old);
    let after = applicable_includes(includes, new);
    if before == after {
        return None;
    }
    Some((describe_identity(&before), describe_identity(&after)))
}

fn describe_identity(paths: &[String]) -> String {
    if paths.is_empty() {
        "the default git identity".to_string()
    } else {
        paths.join(" + ")
    }
}

// ---------------------------------------------------------------------------------------------
// Liveness
// ---------------------------------------------------------------------------------------------

/// Pids whose working directory sits inside `root`. Linux-only (`/proc/<pid>/cwd`); elsewhere it
/// reports nothing and the caller falls back to the lock-file check.
pub fn processes_inside(root: &Path) -> Vec<i32> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|text| text.parse::<i32>().ok()) else {
            continue;
        };
        if let Ok(cwd) = std::fs::read_link(entry.path().join("cwd"))
            && (cwd == root || cwd.starts_with(root))
        {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids
}

/// Whether a git process is mid-write in this repo.
pub fn has_index_lock(repo: &Path) -> bool {
    repo.join(".git").join("index.lock").exists()
}

// ---------------------------------------------------------------------------------------------
// Foreign references
// ---------------------------------------------------------------------------------------------

/// Something outside polygit that names this path and will be stale after the move. Reported, never
/// edited — see the module note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignReference {
    /// The tool or store that owns it.
    pub owner: String,
    /// Where it lives, for the person or tool that will fix it.
    pub location: String,
    pub detail: String,
}

/// Encode an absolute path the way Claude Code names its per-project directory: both `/` and `.`
/// become `-`. Lossy and non-injective, so this is only ever used to encode-and-compare — an
/// encoded name cannot be turned back into a path.
pub fn encode_project_dir(path: &Path) -> String {
    path.display().to_string().replace(['/', '.'], "-")
}

/// Everything polygit can see that names the old path and is not polygit's to rewrite.
pub fn foreign_references(home: &Path, old: &Path) -> Vec<ForeignReference> {
    let mut out = Vec::new();
    let encoded = encode_project_dir(old);
    let projects = home.join(".claude").join("projects");
    if projects.join(&encoded).is_dir() {
        out.push(ForeignReference {
            owner: "claude-code".to_string(),
            location: projects.join(&encoded).display().to_string(),
            detail: "session transcripts for this directory".to_string(),
        });
    }
    // Other config roots keep their own copy of the same tree.
    if let Ok(profiles) = std::fs::read_dir(home.join(".claude-profiles")) {
        for profile in profiles.flatten() {
            let candidate = profile.path().join("projects").join(&encoded);
            if candidate.is_dir() {
                out.push(ForeignReference {
                    owner: "claude-code".to_string(),
                    location: candidate.display().to_string(),
                    detail: format!(
                        "session transcripts under profile {}",
                        profile.file_name().to_string_lossy()
                    ),
                });
            }
        }
    }
    let global_config = home.join(".claude.json");
    if global_config.is_file() {
        out.push(ForeignReference {
            owner: "claude-code".to_string(),
            location: global_config.display().to_string(),
            detail: "projects[] entry — trust, tool allowlist, MCP enablement".to_string(),
        });
    }
    out
}

/// The handoff manifest: what moved, and what still names the old paths.
#[derive(Debug, Clone, Default)]
pub struct MoveManifest {
    pub moves: Vec<(PathBuf, PathBuf)>,
    pub foreign: Vec<ForeignReference>,
}

impl MoveManifest {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "moves": self
                .moves
                .iter()
                .map(|(from, to)| serde_json::json!({ "from": from, "to": to }))
                .collect::<Vec<_>>(),
            "foreign_references": self
                .foreign
                .iter()
                .map(|reference| serde_json::json!({
                    "owner": reference.owner,
                    "location": reference.location,
                    "detail": reference.detail,
                }))
                .collect::<Vec<_>>(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// polygit's own path-keyed state
// ---------------------------------------------------------------------------------------------

/// Rewrite the `<epoch>\t<path>` usage history shared with the fuzzy finder.
pub fn rewrite_usage_history(contents: &str, old: &Path, new: &Path) -> String {
    let mut out = String::with_capacity(contents.len());
    for line in contents.lines() {
        match line.split_once('\t') {
            Some((stamp, path)) => {
                let moved = rewrite_prefix_str(path, old, new).unwrap_or_else(|| path.to_string());
                out.push_str(stamp);
                out.push('\t');
                out.push_str(&moved);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/dev")
    }

    #[test]
    fn prefix_rewriting_respects_component_boundaries() {
        let old = Path::new("/p/repo");
        let new = Path::new("/q/area/repo");
        assert_eq!(rewrite_prefix(old, old, new), Some(new.to_path_buf()));
        assert_eq!(
            rewrite_prefix(Path::new("/p/repo/src/main.rs"), old, new),
            Some(PathBuf::from("/q/area/repo/src/main.rs"))
        );
        // A sibling whose name merely starts with the same text must not be caught.
        assert_eq!(rewrite_prefix(Path::new("/p/repo-two"), old, new), None);
        assert_eq!(rewrite_prefix(Path::new("/elsewhere"), old, new), None);
    }

    #[test]
    fn worktrees_are_a_sibling_keyed_to_the_repo_name() {
        let unit = MoveUnit {
            from: PathBuf::from("/p/demo"),
            to: PathBuf::from("/q/team/demo"),
            worktrees: Vec::new(),
        };
        assert_eq!(unit.worktrees_from(), PathBuf::from("/p/demo.worktrees"));
        assert_eq!(unit.worktrees_to(), PathBuf::from("/q/team/demo.worktrees"));
    }

    const CONFIG: &str = r#"
[user]
    email = personal@example.com
[includeIf "gitdir:~/projects/work/"]
    path = ~/.gitconfig-work
[includeIf "gitdir/i:~/projects/CASED/"]
    path = ~/.gitconfig-cased
"#;

    #[test]
    fn gitdir_includes_are_parsed_with_home_expanded() {
        let includes = parse_gitdir_includes(CONFIG, &home());
        assert_eq!(includes.len(), 2);
        assert_eq!(includes[0].pattern, "/home/dev/projects/work/");
        assert_eq!(includes[0].path, "/home/dev/.gitconfig-work");
        assert!(!includes[0].case_insensitive);
        assert!(includes[1].case_insensitive);
    }

    #[test]
    fn gitdir_patterns_cover_their_subtree() {
        assert!(gitdir_matches("/home/dev/projects/work/", Path::new("/home/dev/projects/work/repo"), false));
        assert!(gitdir_matches(
            "/home/dev/projects/work/",
            Path::new("/home/dev/projects/work/area/repo"),
            false
        ));
        assert!(!gitdir_matches("/home/dev/projects/work/", Path::new("/home/dev/projects/personal/repo"), false));
        // A bare pattern is anchored at any depth, per git's rule.
        assert!(gitdir_matches("work/", Path::new("/anywhere/work/repo"), false));
        // Case folding only when the config asked for it.
        assert!(!gitdir_matches("/home/dev/projects/CASED/", Path::new("/home/dev/projects/cased/repo"), false));
        assert!(gitdir_matches("/home/dev/projects/CASED/", Path::new("/home/dev/projects/cased/repo"), true));
    }

    #[test]
    fn moving_across_an_identity_boundary_is_detected() {
        let includes = parse_gitdir_includes(CONFIG, &home());
        // Out of the work tree: identity changes, and this is the one breakage that alters what you
        // commit rather than what a tool displays.
        let change = identity_change(
            &includes,
            Path::new("/home/dev/projects/work/repo"),
            Path::new("/home/dev/projects/personal/repo"),
        );
        let (before, after) = change.expect("crossing the boundary must be reported");
        assert!(before.contains(".gitconfig-work"));
        assert_eq!(after, "the default git identity");

        // Moving within the same tree keeps the identity, so it must not block.
        assert_eq!(
            identity_change(
                &includes,
                Path::new("/home/dev/projects/work/repo"),
                Path::new("/home/dev/projects/work/area/repo")
            ),
            None
        );
        // And a move entirely outside every pattern is likewise no change.
        assert_eq!(
            identity_change(&includes, Path::new("/tmp/a"), Path::new("/tmp/b")),
            None
        );
    }

    #[test]
    fn project_dir_encoding_folds_dots_and_slashes() {
        // `.worktrees` encodes as `-worktrees`, which is why the encoding cannot be reversed.
        assert_eq!(
            encode_project_dir(Path::new("/home/dev/projects/demo.worktrees/feat")),
            "-home-dev-projects-demo-worktrees-feat"
        );
        assert_eq!(
            encode_project_dir(Path::new("/home/dev/projects/demo-worktrees/feat")),
            encode_project_dir(Path::new("/home/dev/projects/demo.worktrees/feat")),
            "the encoding is not injective — only ever encode-and-compare"
        );
    }

    #[test]
    fn usage_history_rewrites_only_the_path_column() {
        let old = Path::new("/p/repo");
        let new = Path::new("/q/repo");
        let input = "1700000000\t/p/repo\n1700000001\t/p/repo-two\n1700000002\t/q/other\n";
        let output = rewrite_usage_history(input, old, new);
        assert_eq!(
            output,
            "1700000000\t/q/repo\n1700000001\t/p/repo-two\n1700000002\t/q/other\n"
        );
    }

    #[test]
    fn a_process_in_the_tree_is_detected() {
        // This test's own process has its cwd inside the repo it runs in, which is the exact
        // condition the gate exists to catch.
        let cwd = std::env::current_dir().unwrap();
        let pids = processes_inside(&cwd);
        assert!(
            pids.contains(&(std::process::id() as i32)),
            "the running test must be seen as inside its own working directory"
        );
        // A directory nothing runs in reports nobody.
        let empty = tempfile::tempdir().unwrap();
        assert!(processes_inside(empty.path()).is_empty());
    }

    #[test]
    fn the_manifest_carries_both_halves() {
        let manifest = MoveManifest {
            moves: vec![(PathBuf::from("/p/repo"), PathBuf::from("/q/repo"))],
            foreign: vec![ForeignReference {
                owner: "claude-code".into(),
                location: "/home/dev/.claude/projects/-p-repo".into(),
                detail: "session transcripts".into(),
            }],
        };
        let json = manifest.to_json();
        assert_eq!(json["moves"][0]["from"], "/p/repo");
        assert_eq!(json["foreign_references"][0]["owner"], "claude-code");
    }
}

// ---------------------------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------------------------

/// A move that passed every gate, or the reasons it did not.
#[derive(Debug, Clone)]
pub struct MoveCandidate {
    pub unit: MoveUnit,
    pub blockers: Vec<Blocker>,
}

impl MoveCandidate {
    pub fn movable(&self) -> bool {
        self.blockers.is_empty()
    }
}

/// Run every read-only gate against one proposed move. Nothing is written here.
pub async fn vet(from: &Path, to: &Path, includes: &[GitdirInclude]) -> MoveCandidate {
    let mut blockers = Vec::new();
    if to.exists() {
        blockers.push(Blocker::DestinationOccupied);
    }
    if let Some(parent) = to.parent()
        && !same_device(from, parent)
    {
        blockers.push(Blocker::CrossDevice);
    }
    let mut busy = processes_inside(from);
    busy.extend(processes_inside(&MoveUnit::worktrees_dir(from)));
    busy.sort_unstable();
    busy.dedup();
    // The running process's own cwd is not a reason to block its own move.
    busy.retain(|pid| *pid != std::process::id() as i32);
    if !busy.is_empty() {
        blockers.push(Blocker::InUse(busy));
    } else if !cfg!(target_os = "linux") && has_index_lock(from) {
        // Without /proc there is no cwd scan, so a live git write is the only signal left.
        blockers.push(Blocker::InUse(Vec::new()));
    }
    if let Some((before, after)) = identity_change(includes, from, to) {
        blockers.push(Blocker::IdentityChange { from: before, to: after });
    }

    let mut worktrees = Vec::new();
    for entry in crate::git::list_worktrees(from).await {
        let path = entry.path.clone();
        let Some(moved) =
            rewrite_prefix(&path, &MoveUnit::worktrees_dir(from), &MoveUnit::worktrees_dir(to))
        else {
            continue;
        };
        // The admin directory's name is the worktree's basename, possibly truncated and possibly
        // deduped with a numeric suffix — so it is matched through its recorded gitdir, not derived.
        let needle = path.display().to_string();
        let locked = from.join(".git").join("worktrees").read_dir().ok().is_some_and(|entries| {
            entries.flatten().any(|admin| {
                admin.path().join("locked").exists()
                    && std::fs::read_to_string(admin.path().join("gitdir"))
                        .map(|text| text.contains(&needle))
                        .unwrap_or(false)
            })
        });
        if locked {
            blockers.push(Blocker::LockedWorktree(path.clone()));
        }
        worktrees.push(WorktreeMove { from: path, to: moved, locked });
    }

    MoveCandidate { unit: MoveUnit { from: from.to_path_buf(), to: to.to_path_buf(), worktrees }, blockers }
}

/// Whether two paths live on the same filesystem, so a rename is atomic and preserves inodes.
fn same_device(left: &Path, right: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(left), std::fs::metadata(right)) {
            (Ok(left), Ok(right)) => left.dev() == right.dev(),
            _ => true,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (left, right);
        true
    }
}

/// What one applied move did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveOutcome {
    Moved,
    Blocked(Vec<String>),
    Failed(String),
}

/// Move one vetted unit: the repo and its worktrees together, then repair git. Records the
/// completed rename in `journal` before repairing, so a later failure can be walked back.
pub async fn apply(
    candidate: &MoveCandidate,
    journal: &mut Vec<(PathBuf, PathBuf)>,
) -> MoveOutcome {
    if !candidate.movable() {
        return MoveOutcome::Blocked(
            candidate.blockers.iter().map(Blocker::label).collect(),
        );
    }
    let unit = &candidate.unit;
    if let Some(parent) = unit.to.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        return MoveOutcome::Failed(format!("creating {}: {err}", parent.display()));
    }
    if let Err(err) = std::fs::rename(&unit.from, &unit.to) {
        return MoveOutcome::Failed(format!("moving the repo: {err}"));
    }
    journal.push((unit.from.clone(), unit.to.clone()));

    let worktrees_from = unit.worktrees_from();
    if worktrees_from.exists() {
        let worktrees_to = unit.worktrees_to();
        if let Err(err) = std::fs::rename(&worktrees_from, &worktrees_to) {
            return MoveOutcome::Failed(format!("moving the worktrees: {err}"));
        }
        journal.push((worktrees_from, worktrees_to));
    }

    let new_paths: Vec<PathBuf> = unit.worktrees.iter().map(|entry| entry.to.clone()).collect();
    if let Err(err) = crate::git::repair_worktrees(&unit.to, &new_paths).await {
        return MoveOutcome::Failed(format!("repairing worktrees: {err}"));
    }
    MoveOutcome::Moved
}

/// Undo completed renames, newest first. Called when a run fails partway: a half-finished
/// reorganize with no way back is the worst outcome this feature can have.
pub async fn roll_back(journal: &[(PathBuf, PathBuf)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (from, to) in journal.iter().rev() {
        if let Err(err) = std::fs::rename(to, from) {
            errors.push(format!("restoring {}: {err}", from.display()));
        }
    }
    errors
}

#[cfg(test)]
mod apply_tests {
    use super::*;

    /// Build a real repo with a real worktree, so the repair is exercised rather than assumed.
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("demo");
        std::fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str], cwd: &Path| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(cwd)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .output()
                .unwrap();
            assert!(status.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&status.stderr));
        };
        run(&["init", "-q", "-b", "main"], &repo);
        std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
        run(&["add", "."], &repo);
        run(&["commit", "-qm", "first"], &repo);
        run(&["worktree", "add", "-q", "-b", "feat", "../demo.worktrees/feat"], &repo);
        (root, repo)
    }

    #[tokio::test]
    async fn a_repo_and_its_worktrees_move_together_and_still_resolve() {
        let (root, repo) = fixture();
        let destination = root.path().join("area").join("demo");
        let candidate = vet(&repo, &destination, &[]).await;
        assert!(candidate.movable(), "unexpected blockers: {:?}", candidate.blockers);
        assert_eq!(candidate.unit.worktrees.len(), 1);

        let mut journal = Vec::new();
        assert_eq!(apply(&candidate, &mut journal).await, MoveOutcome::Moved);

        assert!(destination.join("file.txt").exists());
        let moved_worktree = root.path().join("area").join("demo.worktrees").join("feat");
        assert!(moved_worktree.join("file.txt").exists());

        // The repair must have reconnected both directions, so git resolves the worktree at its
        // new path and reports nothing prunable.
        let listed = crate::git::list_worktrees(&destination).await;
        assert_eq!(listed.len(), 1, "the worktree must still be linked: {listed:?}");
        assert_eq!(listed[0].path, moved_worktree);
        let status = std::process::Command::new("git")
            .args(["-C", moved_worktree.to_str().unwrap(), "status", "--porcelain"])
            .output()
            .unwrap();
        assert!(status.status.success(), "the moved worktree must be a working checkout");
    }

    #[tokio::test]
    async fn an_occupied_destination_blocks_before_anything_is_written() {
        let (root, repo) = fixture();
        let destination = root.path().join("taken");
        std::fs::create_dir_all(&destination).unwrap();
        let candidate = vet(&repo, &destination, &[]).await;
        assert!(candidate.blockers.contains(&Blocker::DestinationOccupied));

        let mut journal = Vec::new();
        assert!(matches!(apply(&candidate, &mut journal).await, MoveOutcome::Blocked(_)));
        assert!(journal.is_empty());
        assert!(repo.join("file.txt").exists(), "the source must be untouched");
    }

    #[tokio::test]
    async fn an_identity_boundary_blocks_the_move() {
        let (root, repo) = fixture();
        // An include scoped to the repo's current location: moving out of it changes identity.
        let includes = vec![GitdirInclude {
            pattern: format!("{}/", repo.display()),
            path: "/nowhere/.gitconfig-work".to_string(),
            case_insensitive: false,
        }];
        let outside = root.path().join("outside").join("demo");
        let candidate = vet(&repo, &outside, &includes).await;
        assert!(
            candidate
                .blockers
                .iter()
                .any(|blocker| matches!(blocker, Blocker::IdentityChange { .. })),
            "leaving the include's tree changes the commit identity: {:?}",
            candidate.blockers
        );
    }

    #[tokio::test]
    async fn rolling_back_restores_the_original_layout() {
        let (root, repo) = fixture();
        let destination = root.path().join("area").join("demo");
        let candidate = vet(&repo, &destination, &[]).await;
        let mut journal = Vec::new();
        apply(&candidate, &mut journal).await;
        assert!(!repo.exists());

        let errors = roll_back(&journal).await;
        assert!(errors.is_empty(), "rollback reported: {errors:?}");
        assert!(repo.join("file.txt").exists());
        assert!(root.path().join("demo.worktrees").join("feat").exists());
    }
}


// ---------------------------------------------------------------------------------------------
// polygit's own stores
// ---------------------------------------------------------------------------------------------

/// One store polygit rewrote, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreUpdate {
    pub file: PathBuf,
    pub changed: usize,
}

/// Rewrite every path-keyed value in a parsed settings document, returning how many changed.
///
/// `favorites` holds ABSOLUTE paths despite its wording, while `session.collapsed_folders`
/// genuinely holds relative ones — rewriting the latter would corrupt it, so fields are handled by
/// name rather than by shape.
pub fn rewrite_settings_json(value: &mut serde_json::Value, old: &Path, new: &Path) -> usize {
    let mut changed = 0usize;
    let mut rewrite_string = |slot: &mut serde_json::Value| {
        if let Some(text) = slot.as_str()
            && let Some(moved) = rewrite_prefix_str(text, old, new)
        {
            *slot = serde_json::Value::String(moved);
            changed += 1;
        }
    };

    for pointer in ["/lists/favorites", "/workspaces/folder_bookmarks", "/workspaces/roots"] {
        if let Some(list) = value.pointer_mut(pointer).and_then(|node| node.as_array_mut()) {
            for entry in list {
                rewrite_string(entry);
            }
        }
    }
    if let Some(map) =
        value.pointer_mut("/workspaces/workspaces").and_then(|node| node.as_object_mut())
    {
        for folders in map.values_mut() {
            if let Some(list) = folders.as_array_mut() {
                for entry in list {
                    rewrite_string(entry);
                }
            }
        }
    }
    if let Some(map) =
        value.pointer_mut("/repo_page/base_overrides").and_then(|node| node.as_object_mut())
    {
        let rekeyed: serde_json::Map<String, serde_json::Value> = map
            .iter()
            .map(|(key, entry)| match rekey_path_branch_string(key, old, new) {
                Some(moved) => {
                    changed += 1;
                    (moved, entry.clone())
                }
                None => (key.clone(), entry.clone()),
            })
            .collect();
        *map = rekeyed;
    }
    changed
}

/// Rewrite a key of the form `<abs path>\u{1f}<branch>`, splitting first so a branch name that happens
/// to contain the old path is left alone.
fn rekey_path_branch_string(key: &str, old: &Path, new: &Path) -> Option<String> {
    match key.split_once(PATH_BRANCH_SEP) {
        Some((path, branch)) => {
            rewrite_prefix_str(path, old, new).map(|moved| format!("{moved}{PATH_BRANCH_SEP}{branch}"))
        }
        None => rewrite_prefix_str(key, old, new),
    }
}

/// The unit separator polygit uses between a repo path and a branch in composite cache keys.
pub const PATH_BRANCH_SEP: char = '\u{1f}';

/// Rewrite a flat `{"<path>": ...}` cache document.
pub fn rewrite_path_keyed_json(value: &mut serde_json::Value, old: &Path, new: &Path) -> usize {
    rekey_object(value, |key| rewrite_prefix_str(key, old, new))
}

/// Rewrite a `{"<path>\u{1f}<branch>": ...}` cache document.
pub fn rewrite_path_branch_keyed_json(
    value: &mut serde_json::Value,
    old: &Path,
    new: &Path,
) -> usize {
    rekey_object(value, |key| rekey_path_branch_string(key, old, new))
}

fn rekey_object(value: &mut serde_json::Value, rename: impl Fn(&str) -> Option<String>) -> usize {
    let Some(map) = value.as_object_mut() else { return 0 };
    let mut changed = 0usize;
    let rekeyed: serde_json::Map<String, serde_json::Value> = map
        .iter()
        .map(|(key, entry)| match rename(key) {
            Some(moved) => {
                changed += 1;
                (moved, entry.clone())
            }
            None => (key.clone(), entry.clone()),
        })
        .collect();
    *map = rekeyed;
    changed
}

/// Read a JSON file, rewrite it, write it back atomically. A missing file is not an error — a cache
/// that was never written holds nothing stale.
fn update_json_file(
    path: &Path,
    old: &Path,
    new: &Path,
    rewrite: impl Fn(&mut serde_json::Value, &Path, &Path) -> usize,
) -> Option<StoreUpdate> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let changed = rewrite(&mut value, old, new);
    if changed == 0 {
        return None;
    }
    let rendered = serde_json::to_string_pretty(&value).ok()?;
    write_atomic(path, &rendered)?;
    Some(StoreUpdate { file: path.to_path_buf(), changed })
}

/// Write through a sibling temp file and rename, so a concurrent reader never sees half a document.
fn write_atomic(path: &Path, contents: &str) -> Option<()> {
    let temporary = path.with_extension("polygit-tmp");
    std::fs::write(&temporary, contents).ok()?;
    std::fs::rename(&temporary, path).ok()
}

/// Rewrite every polygit-owned store that keys on an absolute repo path.
pub fn rewrite_own_state(config_dir: &Path, old: &Path, new: &Path) -> Vec<StoreUpdate> {
    [
        ("state-v3.json", rewrite_settings_json as fn(&mut serde_json::Value, &Path, &Path) -> usize),
        ("status-cache.json", rewrite_path_keyed_json),
        ("pr-cache.json", rewrite_path_branch_keyed_json),
    ]
    .into_iter()
    .filter_map(|(name, rewrite)| update_json_file(&config_dir.join(name), old, new, rewrite))
    .collect()
}

/// Rewrite the usage history the fuzzy finder ranks by, shared with `goto-repo`.
pub fn rewrite_history_file(path: &Path, old: &Path, new: &Path) -> Option<StoreUpdate> {
    let raw = std::fs::read_to_string(path).ok()?;
    let rewritten = rewrite_usage_history(&raw, old, new);
    if rewritten == raw {
        return None;
    }
    let changed = raw
        .lines()
        .filter(|line| {
            line.split_once('\t')
                .is_some_and(|(_, text)| rewrite_prefix_str(text, old, new).is_some())
        })
        .count();
    write_atomic(path, &rewritten)?;
    Some(StoreUpdate { file: path.to_path_buf(), changed })
}

#[cfg(test)]
mod store_tests {
    use super::*;

    #[test]
    fn settings_rewrite_touches_absolute_keys_and_leaves_relative_ones() {
        let old = Path::new("/p/repo");
        let new = Path::new("/q/area/repo");
        let raw = r#"{
              "lists": { "favorites": ["/p/repo", "/p/other"] },
              "workspaces": {
                "folder_bookmarks": ["/p/repo"],
                "roots": ["/p/repo"],
                "workspaces": { "work": ["/p/repo", "/elsewhere"] }
              },
              "repo_page": { "base_overrides": { "/p/repo\u001fmain": "origin/dev" } },
              "session": { "collapsed_folders": ["p/repo"] }
            }"#;
        let mut value: serde_json::Value = serde_json::from_str(raw).unwrap();

        let changed = rewrite_settings_json(&mut value, old, new);
        assert_eq!(changed, 5);
        assert_eq!(value["lists"]["favorites"][0], "/q/area/repo");
        assert_eq!(value["lists"]["favorites"][1], "/p/other", "an unrelated favorite is untouched");
        assert_eq!(value["workspaces"]["workspaces"]["work"][0], "/q/area/repo");
        let moved_key = format!("/q/area/repo{}main", PATH_BRANCH_SEP);
        assert!(value["repo_page"]["base_overrides"][&moved_key].is_string());
        // collapsed_folders really is relative — rewriting it would corrupt it.
        assert_eq!(value["session"]["collapsed_folders"][0], "p/repo");
    }

    #[test]
    fn caches_are_rewritten_on_disk_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let old = Path::new("/p/repo");
        let new = Path::new("/q/repo");
        std::fs::write(
            dir.path().join("status-cache.json"),
            r#"{"/p/repo":{"status":"ok"},"/p/repo-two":{"status":"ok"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("pr-cache.json"),
            r#"{"/p/repo\u001fmain":{"pr":null}}"#,
        )
        .unwrap();

        let updates = rewrite_own_state(dir.path(), old, new);
        assert_eq!(updates.len(), 2, "both caches carry the old path: {updates:?}");

        let status: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("status-cache.json")).unwrap(),
        )
        .unwrap();
        assert!(status.get("/q/repo").is_some());
        assert!(status.get("/p/repo-two").is_some(), "a sibling must not be swept along");
        let prs: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("pr-cache.json")).unwrap(),
        )
        .unwrap();
        assert!(prs.get(format!("/q/repo{}main", PATH_BRANCH_SEP)).is_some());
        assert!(!dir.path().join("status-cache.polygit-tmp").exists());
    }

    #[test]
    fn a_store_with_nothing_to_change_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status-cache.json");
        std::fs::write(&path, r#"{"/somewhere/else":{}}"#).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(rewrite_own_state(dir.path(), Path::new("/p/repo"), Path::new("/q/repo")).is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn usage_history_is_rewritten_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, "1\t/p/repo\n2\t/p/other\n").unwrap();
        let update =
            rewrite_history_file(&path, Path::new("/p/repo"), Path::new("/q/repo")).unwrap();
        assert_eq!(update.changed, 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1\t/q/repo\n2\t/p/other\n");
    }
}
