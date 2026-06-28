use super::*;

/// Maximum lines in the per-repo ring buffer.
pub const RING_BUFFER_CAPACITY: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoStatus {
    Queued,
    Running { pid: u32 },
    UpToDate,
    Updated,
    /// The checked-out branch has no upstream — nothing to pull. Not an error.
    NoUpstream,
    Skipped,
    /// The remote throttled us (rate limit / connection throttling). Retryable; the app backs
    /// off concurrency and re-queues these with exponential backoff.
    Throttled,
    Failed,
}

impl RepoStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RepoStatus::UpToDate
                | RepoStatus::Updated
                | RepoStatus::NoUpstream
                | RepoStatus::Skipped
                | RepoStatus::Throttled
                | RepoStatus::Failed
        )
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, RepoStatus::Failed)
    }

    /// A pull is in flight for this repo.
    pub fn is_running(&self) -> bool {
        matches!(self, RepoStatus::Running { .. })
    }

    /// A repo "has an issue" worth retrying: it failed, was skipped (dirty), or was throttled.
    /// No-upstream is intentionally excluded — it's not an error, just unconfigured tracking.
    pub fn is_retryable(&self) -> bool {
        matches!(self, RepoStatus::Failed | RepoStatus::Skipped | RepoStatus::Throttled)
    }

    /// Rank for status-column sorting (issues first, then idle, then clean).
    pub fn sort_rank(&self) -> u8 {
        match self {
            RepoStatus::Failed => 0,
            RepoStatus::Throttled => 1,
            RepoStatus::Skipped => 2,
            RepoStatus::Running { .. } => 3,
            RepoStatus::Queued => 4,
            RepoStatus::NoUpstream => 5,
            RepoStatus::Updated => 6,
            RepoStatus::UpToDate => 7,
        }
    }
}

/// What the right pane shows for the selected repo. The info block is an additive overlay
/// (`info_pinned`) drawn above whichever of these is active, not a separate variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightView {
    #[default]
    Log,
    Diff,
}

/// Extra per-repo facts fetched lazily for the info panel (one git call each).
/// Serde-able so the status cache can persist them between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoDetails {
    /// Commits ahead/behind upstream; None when there's no upstream.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub dirty_count: u32,
    pub stash_count: u32,
    /// Local branches excluding `main`/`dev`.
    pub branch_count: u32,
    pub commit_hash: String,
    pub commit_subject: String,
    pub commit_author: String,
    pub commit_rel_date: String,
    /// Committer Unix timestamp of HEAD (for last-commit sorting); 0 when unknown.
    pub commit_timestamp: i64,
}

/// Lifecycle state of a detected PR. Mirrors `gh`'s `state` field. `Open` is the serde default so
/// pre-`state` cache entries (which only ever recorded open PRs) deserialize as open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum PrState {
    #[default]
    Open,
    Merged,
    Closed,
}

impl PrState {
    /// Parse `gh`'s `state` value ("OPEN"/"MERGED"/"CLOSED"); unknown → `Closed` (inert).
    pub fn from_gh(value: &str) -> PrState {
        match value.to_ascii_uppercase().as_str() {
            "OPEN" => PrState::Open,
            "MERGED" => PrState::Merged,
            _ => PrState::Closed,
        }
    }

    /// Lowercase label for the info panel / repo page ("open", "merged", "closed").
    pub fn label(self) -> &'static str {
        match self {
            PrState::Open => "open",
            PrState::Merged => "merged",
            PrState::Closed => "closed",
        }
    }
}

/// A pull request for a repo's current branch, detected via `gh` (most-recent, preferring an open
/// one). Cached (with a TTL) in `pr-cache.json` so the column + info panel don't re-hit the network
/// every frame/launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrInfo {
    pub number: u32,
    pub title: String,
    pub url: String,
    /// Open / merged / closed — drives the state badge + column color. Defaults to `Open` for
    /// cache entries written before this field existed.
    #[serde(default)]
    pub state: PrState,
}

/// One collapsible block in the PR viewer below the meta header: the description, or a single
/// review / comment. `body` is raw markdown; `kind` colors the byline (review state or "commented").
#[derive(Debug, Clone)]
pub struct PrSection {
    /// Byline author login (empty for the Description block).
    pub author: String,
    /// What this block is: "description" · "approved" · "changes_requested" · "commented" · "review".
    pub kind: String,
    /// Day stamp (`YYYY-MM-DD`), empty when unknown.
    pub day: String,
    /// Raw markdown body.
    pub body: String,
}

/// The full data for the PR viewer modal — structured (not one markdown blob) so the modal can show
/// a real meta header, render the description + each review/comment as its own collapsible section,
/// and search across them. The title/number live only in the modal's title bar (not repeated here).
#[derive(Debug, Clone, Default)]
pub struct PrView {
    pub title: String,
    pub url: String,
    pub state: String,
    pub head: String,
    pub base: String,
    pub author: String,
    pub created: String,
    pub additions: i64,
    pub deletions: i64,
    pub labels: Vec<String>,
    /// The description body (markdown); already "_No description._" when the PR has none.
    pub description: String,
    /// Reviews then issue comments, in API order. Each is a collapsible section.
    pub comments: Vec<PrSection>,
}

impl PrInfo {
    /// Whether to surface this PR given the "Merged PRs" setting. Open PRs always show; merged and
    /// closed PRs show only when `show_merged` is on (the setting is off by default). Detection
    /// always finds all states — this is a display-time gate, so toggling is instant (no re-query).
    pub fn shown(&self, show_merged: bool) -> bool {
        show_merged || self.state == PrState::Open
    }
}

/// What the most recent pull delivered. `None` until a pull *updates* the repo; cleared at the
/// start of every pull, so up-to-date repos carry no result. Serde-able for the status cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PullResult {
    /// Short sha before the pull (`HEAD@{1}`); empty when unavailable (shallow / first pull).
    pub prev_head: String,
    /// Short sha after the pull (`HEAD`).
    pub new_head: String,
    /// Commits newly on the current branch (`HEAD@{1}..HEAD`).
    pub commits: u32,
    pub files: u32,
    pub insertions: u32,
    pub deletions: u32,
    /// Best-effort counts parsed from the pull's fetch output (English-git heuristic).
    pub new_tags: u32,
    pub new_branches: u32,
}

impl PullResult {
    /// Whether this result represents an actual delta worth surfacing.
    pub fn has_delta(&self) -> bool {
        self.commits > 0 || self.files > 0 || self.new_tags > 0 || self.new_branches > 0
    }
}

/// Per-branch change counts vs the merge-base with the repo's default branch. `None` on a
/// `BranchInfo` means the stats haven't been computed yet (loaded in a detached worker phase).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BranchStats {
    pub added: u32,
    pub modified: u32,
    pub deleted: u32,
}

impl BranchStats {
    pub fn total(&self) -> u32 {
        self.added + self.modified + self.deleted
    }
}

/// One local branch on the repo page.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub last_commit_rel: String,
    /// Committer Unix timestamp of this branch's tip (for chronological age sorting); 0 if unknown.
    pub last_commit_secs: i64,
    pub subject: String,
    /// Short HEAD sha of this branch (info panel).
    pub commit_sha: String,
    /// Author of this branch's tip commit (info panel).
    pub author: String,
    /// Change counts vs the base branch — `None` until the stats worker fills them in.
    pub stats: Option<BranchStats>,
    /// Short sha of the merge-base with the base branch (info panel).
    pub merge_base_short: Option<String>,
    /// The resolved base branch this branch's stats diff against — `None` until the stats worker
    /// resolves it (detected fork parent, or the user's override).
    pub base: Option<String>,
    /// The resolved `base` came from a user override rather than auto-detection.
    pub base_is_override: bool,
}

impl BranchInfo {
    /// Deletable from the UI: not the current branch, and no unpushed commits (ahead 0 or
    /// no upstream). `git branch -d` (merged-only) is the final safety net.
    pub fn deletable(&self) -> bool {
        !self.is_head && self.ahead.is_none_or(|ahead| ahead == 0)
    }
}

/// One worktree on the repo page.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub branch: String,
    pub path: PathBuf,
}

/// One entry from `git stash list`.
#[derive(Debug, Clone)]
pub struct StashInfo {
    pub index: usize,
    pub label: String,
    /// Relative creation time ("3 days ago"), from the stash commit's committer date.
    pub date_rel: String,
    /// Committer Unix timestamp of the stash commit (for sorting); 0 when unknown.
    pub created_secs: i64,
    /// Change counts (vs the stash's parent) — `None` until the stash-stats worker fills them in.
    pub stats: Option<BranchStats>,
}

/// A recent commit on the repo page's Commits tab (read-only).
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub rel_date: String,
}

/// Which diff a dirty row's modal shows. (Stash rows ignore this.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    /// Uncommitted work vs the branch's own HEAD (`git diff HEAD`).
    Uncommitted,
    /// Everything the branch changed since it forked from its base branch.
    BaseBranch,
}

/// How the diff modal renders the selected file: `Raw` keeps git's own colored output; `Unified`
/// and `Split` are structured, syntax-highlighted, GitHub-PR-style views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffView {
    #[default]
    Raw,
    Unified,
    Split,
}

impl DiffView {
    /// Cycle Raw → Unified → Split → Raw.
    pub fn cycle(self) -> Self {
        match self {
            DiffView::Raw => DiffView::Unified,
            DiffView::Unified => DiffView::Split,
            DiffView::Split => DiffView::Raw,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            DiffView::Raw => "raw",
            DiffView::Unified => "unified",
            DiffView::Split => "split",
        }
    }
}

/// What a diff modal is showing.
#[derive(Debug, Clone)]
pub enum DiffSource {
    /// A stash entry: `git stash show -p stash@{index}` at `path`.
    Stash { path: PathBuf, index: usize, label: String },
    /// A dirty branch/worktree at `path` (toggle between uncommitted and base-branch diff).
    Dirty { path: PathBuf, name: String },
    /// A clean branch — its diff vs the base branch (the changes the branch introduces).
    Branch { path: PathBuf, name: String },
    /// A single commit — `git show <sha>` (the changes that commit introduced).
    Commit { path: PathBuf, sha: String, label: String },
}

/// One changed file shown in the diff modal's file-list panel.
#[derive(Debug, Clone)]
pub struct DiffFile {
    /// Single-char git status: M(odified) A(dded) D(eleted) R(enamed) ?(untracked) …
    pub status: String,
    /// Path relative to the repo root.
    pub path: String,
    /// Untracked file — its per-file diff needs `git diff --no-index`.
    pub untracked: bool,
}

/// Which diff-modal panel has keyboard focus (`Tab` toggles it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffFocus {
    #[default]
    Files,
    Diff,
}

/// Which main panel has keyboard focus. `Tab`/`Shift-Tab` cycle the *visible* panels; `1`-`4` jump.
/// The number labels are stable (List=1, Info=2, Result=3, RepoPage=4) even when a panel is hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    List,
    Info,
    Result,
    RepoPage,
}

/// The full-screen-ish (90%) diff modal state: a file-list panel over the selected file's diff.
#[derive(Debug, Clone)]
pub struct DiffModal {
    pub source: DiffSource,
    pub mode: DiffMode,
    /// Render style: raw (git color) / unified / split. Persisted across opens.
    pub view: DiffView,
    /// Which panel `j/k/g/G` drive (Tab toggles).
    pub focus: DiffFocus,
    /// The changed files (top panel). `None` while the list is still loading.
    pub files: Vec<DiffFile>,
    /// Index of the selected file in `files`.
    pub selected: usize,
    /// Scroll offset of the file-list panel.
    pub file_scroll: usize,
    /// Diff lines of the selected file (bottom panel).
    pub lines: Vec<String>,
    /// Scroll offset of the diff panel.
    pub scroll: usize,
    /// The file list is being (re)fetched.
    pub loading: bool,
    /// The selected file's diff is being fetched.
    pub diff_loading: bool,
    /// Active status filter (a canonical status bucket char); `None` = all files.
    pub status_filter: Option<char>,
}

/// Canonical single-char bucket for a git status string (`M`, `A`, `D`, `R`, `?`, …).
pub fn status_bucket(status: &str) -> char {
    status.chars().next().unwrap_or('?').to_ascii_uppercase()
}

/// Display/grouping rank for a status bucket: modified, added, deleted, renamed, copied,
/// type-change, then anything else, with untracked last.
fn status_rank(bucket: char) -> u8 {
    match bucket {
        'M' => 0,
        'A' => 1,
        'D' => 2,
        'R' => 3,
        'C' => 4,
        'T' => 5,
        '?' => 7,
        _ => 6,
    }
}

impl DiffModal {
    /// Show the clickable status-filter chips: enough files to be worth filtering, and more
    /// than one distinct status to filter between.
    pub fn chips_active(&self) -> bool {
        self.files.len() > 10 && self.distinct_status_count() >= 2
    }

    fn distinct_status_count(&self) -> usize {
        let mut seen: Vec<char> = Vec::new();
        for file in &self.files {
            let bucket = status_bucket(&file.status);
            if !seen.contains(&bucket) {
                seen.push(bucket);
            }
        }
        seen.len()
    }

    /// `(bucket, count)` for each present status, in display order. Counts are over the full
    /// (unfiltered) list, so the chip badges stay stable while a filter is applied.
    pub fn status_chips(&self) -> Vec<(char, usize)> {
        let mut counts: Vec<(char, usize)> = Vec::new();
        for file in &self.files {
            let bucket = status_bucket(&file.status);
            match counts.iter_mut().find(|(existing, _)| *existing == bucket) {
                Some((_, count)) => *count += 1,
                None => counts.push((bucket, 1)),
            }
        }
        counts.sort_by_key(|(bucket, _)| status_rank(*bucket));
        counts
    }

    /// Indices into `files` in display order: filtered by `status_filter`, and (when the chips
    /// are active) grouped into status sections. The list is a pure reordering with no header
    /// rows, so display row N maps 1:1 to `visible_file_indices()[N]`.
    pub fn visible_file_indices(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.files.len())
            .filter(|&index| {
                self.status_filter
                    .is_none_or(|bucket| status_bucket(&self.files[index].status) == bucket)
            })
            .collect();
        if self.chips_active() {
            // Stable sort keeps each section in its original order.
            indices.sort_by_key(|&index| status_rank(status_bucket(&self.files[index].status)));
        }
        indices
    }
}

/// Data backing the dedicated repo page (branches + worktrees + fetch state).
#[derive(Debug, Clone, Default)]
pub struct RepoPageData {
    pub branches: Vec<BranchInfo>,
    pub worktrees: Vec<WorktreeInfo>,
    pub stashes: Vec<StashInfo>,
    /// Recent commits on the current branch (read-only Commits tab), newest first.
    pub commits: Vec<CommitInfo>,
    /// Uncommitted-change count in the main worktree (0 = clean; >0 marks the HEAD row diff-able).
    pub head_dirty_count: u32,
    /// Worktree paths with uncommitted changes + their change count.
    pub dirty_worktrees: Vec<(PathBuf, u32)>,
    /// True once `git fetch` finished (false during the instant pre-fetch phase).
    pub fetched: bool,
    pub fetch_error: Option<String>,
    /// The repo's default base branch (e.g. `origin/main`) — what per-branch stats diff against.
    pub base_branch: Option<String>,
}

/// A selectable row on the repo page (a branch, a worktree, or a stash).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRowKind {
    Branch,
    Worktree,
    Stash,
    Commit,
}

/// A flattened, selectable repo-page row carrying everything render + actions need.
#[derive(Debug, Clone)]
pub struct PageRow {
    pub kind: PageRowKind,
    pub branch: String,
    pub path: PathBuf,
    pub deletable: bool,
    pub is_head: bool,
    /// Has uncommitted changes (a diff modal can be opened on it).
    pub dirty: bool,
    /// Number of uncommitted changes (for the dirty column); 0 when clean/not applicable.
    pub dirty_count: u32,
    /// Set for stash rows: the `stash@{index}` number.
    pub stash_index: Option<usize>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub upstream: Option<String>,
    pub last_commit_rel: String,
    /// Committer Unix timestamp of the tip commit (for age sorting); 0 for stashes / unknown.
    pub last_commit_secs: i64,
    pub subject: String,
    /// Change stats vs the base branch (branch/worktree rows); `None` for stashes or while loading.
    pub stats: Option<BranchStats>,
    /// Short HEAD sha (info panel); empty for stash rows.
    pub commit_sha: String,
    /// Tip-commit author (info panel); empty for stash rows.
    pub author: String,
    /// Short merge-base sha vs the base branch (info panel).
    pub merge_base_short: Option<String>,
    /// The resolved base branch (detected fork parent or override); `None` while loading or for
    /// stash rows. Shown in the `base` column and clickable to override.
    pub base: Option<String>,
    /// The resolved `base` came from a user override rather than auto-detection.
    pub base_is_override: bool,
}

impl PageRow {
    /// The verb `d` performs on this row (for the dynamic footer hint), or None when `d` does
    /// nothing (a clean current branch can't be deleted/discarded).
    pub fn delete_action(&self) -> Option<&'static str> {
        match self.kind {
            PageRowKind::Stash => Some("drop"),
            PageRowKind::Worktree => Some("remove"),
            PageRowKind::Branch if self.is_head => self.dirty.then_some("discard"),
            PageRowKind::Branch => Some("delete"),
            PageRowKind::Commit => None, // commits are read-only — `d` does nothing
        }
    }

    /// The repo-page tab/section this row belongs to.
    pub fn tab(&self) -> RepoTab {
        match self.kind {
            PageRowKind::Branch => RepoTab::Branches,
            PageRowKind::Worktree => RepoTab::Worktrees,
            PageRowKind::Stash => RepoTab::Stashes,
            PageRowKind::Commit => RepoTab::Commits,
        }
    }
}

/// An optional list column the user can toggle on via the `t` leader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Status,
    AheadBehind,
    Dirty,
    LastCommit,
    Worktrees,
    Branches,
    Stashes,
    /// Commits the most recent pull landed on the current branch.
    PulledCommits,
    /// Files the most recent pull changed.
    PulledFiles,
    /// Open pull request for the current branch (via `gh`), shown as a clickable `#N`.
    PullRequest,
    /// Favorite marker (★/☆), clickable to toggle.
    Favorite,
}

/// Which optional list columns are enabled. `#[serde(default)]` keeps older state files
/// (missing newer fields) loadable instead of resetting every column.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnFlags {
    pub status: bool,
    pub ahead_behind: bool,
    pub dirty: bool,
    pub last_commit: bool,
    pub worktrees: bool,
    pub branches: bool,
    pub stashes: bool,
    pub pulled_commits: bool,
    pub pulled_files: bool,
    pub pull_request: bool,
    pub favorite: bool,
}

impl ColumnFlags {
    /// Any column that needs a per-repo `git` call (drives the background details pass).
    pub fn any_git(&self) -> bool {
        self.ahead_behind || self.dirty || self.last_commit || self.branches || self.stashes
    }
}

/// An optional repo-page branch column, toggled via the page-local `t` leader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoPageColumn {
    AheadBehind,
    Dirty,
    Added,
    Modified,
    Deleted,
    Total,
    Upstream,
    Base,
    Age,
    /// Open pull request for the current branch (via `gh`), shown as a clickable `#N` on the HEAD row.
    PullRequest,
    Subject,
}

/// A sortable repo-page column (the target of a header click). `Name` sorts by branch name; the
/// rest mirror the `RepoPageColumn` data columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoPageSort {
    Name,
    AheadBehind,
    Dirty,
    Added,
    Modified,
    Deleted,
    Total,
    Upstream,
    Base,
    Age,
    Subject,
}

impl RepoPageSort {
    /// Short label for the `[sort ▾]` trigger indicator.
    pub fn label(self) -> &'static str {
        match self {
            RepoPageSort::Name => "name",
            RepoPageSort::AheadBehind => "ahead/behind",
            RepoPageSort::Dirty => "dirty",
            RepoPageSort::Added => "added",
            RepoPageSort::Modified => "modified",
            RepoPageSort::Deleted => "deleted",
            RepoPageSort::Total => "total",
            RepoPageSort::Upstream => "upstream",
            RepoPageSort::Base => "base",
            RepoPageSort::Age => "age",
            RepoPageSort::Subject => "subject",
        }
    }
}

/// Order two repo-page rows by `sort` (ascending); name is the stable tiebreak. The caller applies
/// the direction. Worktree/stash rows missing a field sort as if zero/empty.
pub fn repo_page_row_cmp(sort: RepoPageSort, first: &PageRow, second: &PageRow) -> std::cmp::Ordering {
    let stat = |row: &PageRow, pick: fn(&BranchStats) -> u32| row.stats.as_ref().map(pick).unwrap_or(0);
    let name = |row: &PageRow| row.branch.to_lowercase();
    match sort {
        RepoPageSort::Name => name(first).cmp(&name(second)),
        RepoPageSort::AheadBehind => (first.behind.unwrap_or(0), first.ahead.unwrap_or(0))
            .cmp(&(second.behind.unwrap_or(0), second.ahead.unwrap_or(0))),
        RepoPageSort::Dirty => first.dirty_count.cmp(&second.dirty_count),
        RepoPageSort::Added => stat(first, |stat| stat.added).cmp(&stat(second, |stat| stat.added)),
        RepoPageSort::Modified => {
            stat(first, |stat| stat.modified).cmp(&stat(second, |stat| stat.modified))
        }
        RepoPageSort::Deleted => {
            stat(first, |stat| stat.deleted).cmp(&stat(second, |stat| stat.deleted))
        }
        RepoPageSort::Total => stat(first, |stat| stat.total()).cmp(&stat(second, |stat| stat.total())),
        RepoPageSort::Upstream => first
            .upstream
            .clone()
            .unwrap_or_default()
            .to_lowercase()
            .cmp(&second.upstream.clone().unwrap_or_default().to_lowercase()),
        RepoPageSort::Base => first
            .base
            .clone()
            .unwrap_or_default()
            .to_lowercase()
            .cmp(&second.base.clone().unwrap_or_default().to_lowercase()),
        RepoPageSort::Age => first.last_commit_secs.cmp(&second.last_commit_secs),
        RepoPageSort::Subject => first.subject.to_lowercase().cmp(&second.subject.to_lowercase()),
    }
    .then_with(|| name(first).cmp(&name(second)))
}

/// Which repo-page branch columns are shown. Defaults to all on; persisted in state.json.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoPageColumns {
    pub ahead_behind: bool,
    pub dirty: bool,
    pub added: bool,
    pub modified: bool,
    pub deleted: bool,
    pub total: bool,
    pub upstream: bool,
    pub base: bool,
    pub age: bool,
    pub pull_request: bool,
    pub subject: bool,
}

impl Default for RepoPageColumns {
    fn default() -> Self {
        Self {
            ahead_behind: true,
            dirty: true,
            added: true,
            modified: true,
            deleted: true,
            total: true,
            upstream: true,
            base: true,
            age: true,
            pull_request: true,
            subject: true,
        }
    }
}

/// The open base-branch picker: choose which branch a target branch's stats diff against.
/// The chosen value becomes a persisted per-repo+branch override; the "detected" entry clears it.
#[derive(Debug, Clone)]
pub struct BasePicker {
    /// Repo this picker targets (index into `AppState::repos`).
    pub repo_index: usize,
    /// The branch whose base is being overridden.
    pub branch: String,
    /// The auto-detected base (shown first, marked) — selecting it clears any override.
    pub detected: Option<String>,
    /// The override currently in effect for this branch, if any.
    pub current: Option<String>,
    /// Candidate branch refs to choose from (local heads + remote-tracking branches).
    pub candidates: Vec<String>,
    /// Highlighted row: 0 = the "detected" entry, then `candidates` by index + 1.
    pub selected: usize,
}

impl BasePicker {
    /// Total rows: the "detected (auto)" entry plus every candidate.
    pub fn row_count(&self) -> usize {
        self.candidates.len() + 1
    }

    /// The base ref a given row selects: row 0 → `None` (clear override → auto-detect), otherwise
    /// the candidate at `row - 1`.
    pub fn ref_at(&self, row: usize) -> Option<String> {
        if row == 0 {
            None
        } else {
            self.candidates.get(row - 1).cloned()
        }
    }
}

/// Which menu a `[… ▾]` header dropdown drives. A mouse-friendly companion to the `t`/`s`/`f`
/// leader chords (which still work). Columns dropdowns multi-toggle (stay open); sort/filter
/// dropdowns pick one (close on select).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropdownKind {
    ListColumns,
    ListSort,
    ListFilter,
    PageColumns,
    PageSort,
}

/// One row in an open dropdown: its label, whether it's currently on/active, its mnemonic key, and
/// whether it's selectable (a column with no data anywhere renders dim + inert).
pub struct DropdownItem {
    pub label: String,
    pub on: bool,
    pub mnemonic: char,
    pub enabled: bool,
}

/// One navigable position in the accordion settings layout: a section header or a setting row.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AccPos {
    Header(usize),
    Row(usize),
}

/// An open header dropdown overlay: anchored so its right edge lines up under the `[… ▾]` chip
/// that opened it (the overlay floats just below).
#[derive(Debug, Clone, Copy)]
pub struct Dropdown {
    pub kind: DropdownKind,
    /// Screen column of the chip's right edge — the overlay's right edge aligns to it.
    pub anchor_right: u16,
    pub anchor_row: u16,
    /// Highlighted item, or `None` when nothing is highlighted yet (the state on open).
    pub selected: Option<usize>,
}

/// A pending two-key chord: `v` then a key picks a view mode; `z` then a key folds. (Columns, sort,
/// and the status filter are driven by the header dropdowns, not a chord.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leader {
    /// `v` then a key picks a view mode (`g` grouped, `t` tree).
    View,
    /// `z` then a key folds (`a` toggle, `o`/`c` open/close, `O`/`M`/`R` recursive/all).
    Fold,
}

/// Which column the repo list is sorted by. The list is always sorted; `Name` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortColumn {
    #[default]
    Name,
    Branch,
    Status,
    AheadBehind,
    Dirty,
    LastCommit,
    Worktrees,
    Branches,
    Stashes,
    PulledCommits,
    PulledFiles,
    PullRequest,
}

impl SortColumn {
    /// Short header/label for this column.
    pub fn label(self) -> &'static str {
        match self {
            SortColumn::Name => "name",
            SortColumn::Branch => "branch",
            SortColumn::Status => "status",
            SortColumn::AheadBehind => "ahead/behind",
            SortColumn::Dirty => "dirty",
            SortColumn::LastCommit => "last-commit",
            SortColumn::Worktrees => "worktrees",
            SortColumn::Branches => "branches",
            SortColumn::PulledCommits => "pulled",
            SortColumn::PulledFiles => "changed",
            SortColumn::Stashes => "stashes",
            SortColumn::PullRequest => "pull-request",
        }
    }
}

/// Sort direction for the active sort column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    pub fn flip(self) -> Self {
        match self {
            SortDir::Asc => SortDir::Desc,
            SortDir::Desc => SortDir::Asc,
        }
    }

    /// The arrow glyph for this direction (used in the column header).
    pub fn arrow(self) -> &'static str {
        match self {
            SortDir::Asc => "▲",
            SortDir::Desc => "▼",
        }
    }
}

/// Which tab the `?` help modal shows. Persisted so the last tab reopens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HelpTab {
    #[default]
    Hotkeys,
    CliFlags,
    Legend,
    About,
    DesignSystem,
}

impl HelpTab {
    /// Next tab (Tab key): Hotkeys → CLI & Flags → Legend → Design System → About → Hotkeys.
    pub fn next(self) -> Self {
        match self {
            HelpTab::Hotkeys => HelpTab::CliFlags,
            HelpTab::CliFlags => HelpTab::Legend,
            HelpTab::Legend => HelpTab::DesignSystem,
            HelpTab::DesignSystem => HelpTab::About,
            HelpTab::About => HelpTab::Hotkeys,
        }
    }

    /// Previous tab (Shift+Tab).
    pub fn prev(self) -> Self {
        match self {
            HelpTab::Hotkeys => HelpTab::About,
            HelpTab::CliFlags => HelpTab::Hotkeys,
            HelpTab::Legend => HelpTab::CliFlags,
            HelpTab::DesignSystem => HelpTab::Legend,
            HelpTab::About => HelpTab::DesignSystem,
        }
    }
}

/// Filter the repo list by pull outcome. Applied on top of the `/` name filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    #[default]
    All,
    Updated,
    UpToDate,
    Skipped,
    Failed,
    /// Failed or skipped — the repos that need attention.
    Issues,
}

impl StatusFilter {
    /// Whether a repo with `status` passes this filter.
    pub fn matches(&self, status: &RepoStatus) -> bool {
        match self {
            StatusFilter::All => true,
            StatusFilter::Updated => matches!(status, RepoStatus::Updated),
            StatusFilter::UpToDate => matches!(status, RepoStatus::UpToDate),
            StatusFilter::Skipped => matches!(status, RepoStatus::Skipped),
            StatusFilter::Failed => matches!(status, RepoStatus::Failed),
            StatusFilter::Issues => status.is_retryable(),
        }
    }

    /// Short tag shown in the status bar when the filter is active (None for All).
    pub fn tag(&self) -> Option<&'static str> {
        match self {
            StatusFilter::All => None,
            StatusFilter::Updated => Some("updated"),
            StatusFilter::UpToDate => Some("up-to-date"),
            StatusFilter::Skipped => Some("skipped"),
            StatusFilter::Failed => Some("failed"),
            StatusFilter::Issues => Some("issues"),
        }
    }
}

/// Which glyph set the UI draws for status / column / marker icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IconStyle {
    #[default]
    Unicode,
    Emoji,
}

impl IconStyle {
    /// The glyph set for this style.
    pub fn icons(self) -> &'static IconSet {
        match self {
            IconStyle::Unicode => &UNICODE_ICONS,
            IconStyle::Emoji => &EMOJI_ICONS,
        }
    }
}

/// Color theme. `Auto` detects whether the terminal background is dark or light at startup
/// and applies the matching palette; `Dark`/`Light` force one explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Auto,
    Dark,
    Light,
}

impl Theme {
    /// Cycle Auto → Dark → Light → Auto.
    pub fn cycle(self) -> Self {
        match self {
            Theme::Auto => Theme::Dark,
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Auto,
        }
    }
}

/// How the draggable pane splitters are presented. `Dedicated` reserves a real 1-cell lane (a
/// column between the panes, a row between stacked panes) filled with a persistent grip — clearly
/// visible, at the cost of one cell of space. `Hover` keeps the panes flush (zero-width boundary)
/// and only shows a thin grip under the cursor when it crosses a splitter hotspot. The two are
/// mutually exclusive; the hotspots work in both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SplitterMode {
    #[default]
    Dedicated,
    Hover,
}

impl SplitterMode {
    /// Cycle Dedicated ⇄ Hover.
    pub fn cycle(self) -> Self {
        match self {
            SplitterMode::Dedicated => SplitterMode::Hover,
            SplitterMode::Hover => SplitterMode::Dedicated,
        }
    }
}

/// Which AI coding-agent CLI the `c` hotkey launches in the selected repo. The binary is run in
/// the repo dir; when "Skip permissions" is on, the agent's bypass-all-prompts flag is appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeAgent {
    #[default]
    Claude,
    Codex,
    Gemini,
}

impl ClaudeAgent {
    /// Cycle Claude → Codex → Gemini → Claude.
    pub fn cycle(self) -> Self {
        match self {
            ClaudeAgent::Claude => ClaudeAgent::Codex,
            ClaudeAgent::Codex => ClaudeAgent::Gemini,
            ClaudeAgent::Gemini => ClaudeAgent::Claude,
        }
    }

    /// The CLI binary name (must be on `PATH`).
    pub fn binary(self) -> &'static str {
        match self {
            ClaudeAgent::Claude => "claude",
            ClaudeAgent::Codex => "codex",
            ClaudeAgent::Gemini => "gemini",
        }
    }

    /// The agent's "bypass all approval prompts" flag, appended when Skip permissions is on.
    pub fn danger_flag(self) -> &'static str {
        match self {
            ClaudeAgent::Claude => "--dangerously-skip-permissions",
            ClaudeAgent::Codex => "--dangerously-bypass-approvals-and-sandbox",
            ClaudeAgent::Gemini => "--yolo",
        }
    }
}

/// Contrast level for the active palette. `Soft` narrows the foreground/background distance
/// and desaturates accents for a gentler look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Contrast {
    #[default]
    Normal,
    Soft,
}

impl Contrast {
    /// Toggle Normal ↔ Soft.
    pub fn cycle(self) -> Self {
        match self {
            Contrast::Normal => Contrast::Soft,
            Contrast::Soft => Contrast::Normal,
        }
    }
}

/// How the selected row is highlighted. `Blue` is a solid blue bar with white text (high contrast,
/// but it overrides per-column colors). `Subtle` is a soft tint that keeps each column's own color
/// readable — better for the repo list / repo page / diff list, whose values are color-coded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionStyle {
    #[default]
    Blue,
    Subtle,
}

impl SelectionStyle {
    /// Toggle Blue ↔ Subtle.
    pub fn cycle(self) -> Self {
        match self {
            SelectionStyle::Blue => SelectionStyle::Subtle,
            SelectionStyle::Subtle => SelectionStyle::Blue,
        }
    }
}

/// How a *button* (footer hint, modal hint, tab, radio chip, keyboard key, close button) is
/// highlighted on hover. `Subtle` is the soft background tint (the original behavior); `Inverted`
/// is reverse-video (swap fg/bg) — the punchier look the selected-row `Blue` style has. Independent
/// of `SelectionStyle`, which governs the selected *row* in lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ButtonHoverStyle {
    #[default]
    Subtle,
    Inverted,
}

impl ButtonHoverStyle {
    /// Toggle Subtle ↔ Inverted.
    pub fn cycle(self) -> Self {
        match self {
            ButtonHoverStyle::Subtle => ButtonHoverStyle::Inverted,
            ButtonHoverStyle::Inverted => ButtonHoverStyle::Subtle,
        }
    }
}

/// Layout of the settings modal: `Tabbed` shows IDE-style vertical tabs (one section at a time);
/// `Accordion` stacks every section with a collapsible header; `Flat` stacks every section
/// expanded (the original layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsLayout {
    #[default]
    Tabbed,
    Accordion,
    Flat,
}

impl SettingsLayout {
    /// Cycle Tabbed → Accordion → Flat → Tabbed.
    pub fn cycle(self) -> Self {
        match self {
            SettingsLayout::Tabbed => SettingsLayout::Accordion,
            SettingsLayout::Accordion => SettingsLayout::Flat,
            SettingsLayout::Flat => SettingsLayout::Tabbed,
        }
    }

    /// Short label for the *next* layout (footer hint: "press v for …").
    pub fn next_label(self) -> &'static str {
        match self {
            SettingsLayout::Tabbed => " accordion view",
            SettingsLayout::Accordion => " flat view",
            SettingsLayout::Flat => " tabbed view",
        }
    }
}

/// The Design System tab's section names, in order — the single source for the vertical-tab labels
/// (render) and the section count (key navigation). Keep in sync with `design_sections`.
pub const DESIGN_SECTIONS: [&str; 6] =
    ["Theming", "Palette", "Buttons", "List rows", "Radios", "Dialogs"];

/// Layout of the help modal's Design System tab: `Flat` stacks every section in one scroll (the
/// original), `Tabbed` shows the sections as a vertical tab column with the active one beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesignLayout {
    #[default]
    Flat,
    Tabbed,
}

impl DesignLayout {
    /// Cycle Flat → Tabbed → Flat.
    pub fn cycle(self) -> Self {
        match self {
            DesignLayout::Flat => DesignLayout::Tabbed,
            DesignLayout::Tabbed => DesignLayout::Flat,
        }
    }

    /// Short label for the *next* layout (footer hint).
    pub fn next_label(self) -> &'static str {
        match self {
            DesignLayout::Flat => " tabbed view",
            DesignLayout::Tabbed => " flat view",
        }
    }
}

/// A flag in the interactive CLI builder (the help modal's "CLI & Flags" tab).
pub enum CliFlagKind {
    /// A boolean flag (present or absent), e.g. `--no-tui`.
    Toggle,
    /// A flag that takes a value, with a placeholder shown when empty, e.g. `--depth N`.
    Value(&'static str),
    /// The positional `[DIR]` argument.
    Positional(&'static str),
}

pub struct CliFlag {
    /// The long form (`--depth`, `--jobs`, …), or "" for the positional.
    pub flag: &'static str,
    /// The short form (`-j`, `-w`), when the real clap arg has one — lets the builder offer it.
    pub short: Option<&'static str>,
    pub kind: CliFlagKind,
    pub help: &'static str,
    /// A related "parent" flag (index into `CLI_FLAGS`): this flag renders indented beneath it and
    /// is disabled (and force-unchecked) while the parent is unchecked — e.g. `--no-recursive`
    /// under `--depth`, `--profile-out` under `--profile`.
    pub parent: Option<usize>,
}

/// The CLI builder's flag catalog, in display order. Source-driven — mirrors the real clap `Cli`
/// args (`src/main.rs`): keep it in sync when adding/removing a flag.
pub static CLI_FLAGS: &[CliFlag] = &[
    CliFlag { flag: "", short: None, kind: CliFlagKind::Positional("DIR"), help: "directory to scan (default: cwd)", parent: None },
    CliFlag { flag: "--workspace", short: Some("-w"), kind: CliFlagKind::Value("NAME"), help: "open a saved workspace by name", parent: None },
    CliFlag { flag: "--jobs", short: Some("-j"), kind: CliFlagKind::Value("N"), help: "concurrency (default: nproc)", parent: None },
    CliFlag { flag: "--depth", short: None, kind: CliFlagKind::Value("N"), help: "max scan depth (default: 16; 1 = flat)", parent: None },
    CliFlag { flag: "--no-recursive", short: None, kind: CliFlagKind::Toggle, help: "single-level scan (same as --depth 1)", parent: Some(3) },
    CliFlag { flag: "--no-tui", short: None, kind: CliFlagKind::Toggle, help: "plain streaming output (no TUI)", parent: None },
    CliFlag { flag: "--no-worktrees", short: None, kind: CliFlagKind::Toggle, help: "skip worktree discovery", parent: None },
    CliFlag { flag: "--timeout", short: None, kind: CliFlagKind::Value("S"), help: "per-pull timeout seconds (default: 30)", parent: None },
    CliFlag { flag: "--profile", short: None, kind: CliFlagKind::Toggle, help: "per-repo timing report (slowest first)", parent: None },
    CliFlag { flag: "--profile-out", short: None, kind: CliFlagKind::Value("FILE"), help: "write the profile report to FILE", parent: Some(8) },
];

/// When the CLI builder shows each flag's help text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CliHelpMode {
    Always,
    #[default]
    OnHover,
    Never,
}

impl CliHelpMode {
    pub const ALL: [CliHelpMode; 3] = [CliHelpMode::Always, CliHelpMode::OnHover, CliHelpMode::Never];
    pub fn label(self) -> &'static str {
        match self {
            CliHelpMode::Always => "always",
            CliHelpMode::OnHover => "on hover",
            CliHelpMode::Never => "never",
        }
    }
    /// Cycle Always → OnHover → Never → Always (for the `h` key).
    pub fn cycle(self) -> Self {
        match self {
            CliHelpMode::Always => CliHelpMode::OnHover,
            CliHelpMode::OnHover => CliHelpMode::Never,
            CliHelpMode::Never => CliHelpMode::Always,
        }
    }
}

/// Mutable state of the interactive CLI builder: which flags are selected/edited.
#[derive(Default)]
pub struct CliBuilder {
    /// Selected flag row (index into `CLI_FLAGS`).
    pub selected: usize,
    /// Per-flag checkbox state — index-aligned with `CLI_FLAGS`. Every flag (toggle, value, and
    /// positional) has one; a value flag is only emitted when checked.
    pub on: Vec<bool>,
    /// Per-flag value (value flags / positional) — index-aligned with `CLI_FLAGS`.
    pub values: Vec<String>,
    /// Per-flag preference to render the short form (`-j`) instead of the long (`--jobs`). Only
    /// meaningful where `CliFlag::short` is `Some`.
    pub use_short: Vec<bool>,
    /// When editing a value flag, the in-progress input buffer (auto-applied to `values` live).
    pub editing: Option<String>,
    /// When the per-flag help text is shown (always / on hover / never). Persisted.
    pub help_mode: CliHelpMode,
}

impl CliBuilder {
    fn flag_on(&self, idx: usize) -> bool {
        self.on.get(idx).copied().unwrap_or(false)
    }

    /// Whether a flag is interactive: a parent-less flag always is; a child only while its parent
    /// is checked (otherwise it renders disabled and dimmed, and is force-unchecked).
    pub fn enabled(&self, idx: usize) -> bool {
        match CLI_FLAGS.get(idx).and_then(|flag| flag.parent) {
            Some(parent) => self.flag_on(parent),
            None => true,
        }
    }

    /// The flag form to render/emit: the short form when chosen and available, else the long form.
    pub fn form(&self, idx: usize) -> &'static str {
        let flag = &CLI_FLAGS[idx];
        match flag.short {
            Some(short) if self.use_short.get(idx).copied().unwrap_or(false) => short,
            _ => flag.flag,
        }
    }

    /// Set a flag's checkbox. Unchecking a parent cascades: every child is force-unchecked too
    /// (and will render disabled). No-op on a disabled (parent-off) flag.
    pub fn set_on(&mut self, idx: usize, value: bool) {
        if value && !self.enabled(idx) {
            return;
        }
        if let Some(slot) = self.on.get_mut(idx) {
            *slot = value;
        }
        if !value {
            let children: Vec<usize> = CLI_FLAGS
                .iter()
                .enumerate()
                .filter(|(_, flag)| flag.parent == Some(idx))
                .map(|(child, _)| child)
                .collect();
            for child in children {
                self.set_on(child, false);
            }
        }
    }

    /// Toggle a flag's checkbox (with the same cascade as [`Self::set_on`]).
    pub fn toggle(&mut self, idx: usize) {
        self.set_on(idx, !self.flag_on(idx));
    }

    /// Toggle the short/long form preference for a flag (no-op if it has no short form).
    pub fn toggle_short(&mut self, idx: usize) {
        if CLI_FLAGS.get(idx).and_then(|flag| flag.short).is_some() {
            if let Some(slot) = self.use_short.get_mut(idx) {
                *slot = !*slot;
            }
        }
    }

    /// The emitted tokens after `polygit`, each tagged with its flag index (for the clickable,
    /// hoverable multiline preview). A value flag with no value yet emits just its form.
    pub fn command_tokens(&self) -> Vec<(usize, String)> {
        let mut tokens = Vec::new();
        for (idx, flag) in CLI_FLAGS.iter().enumerate() {
            if !self.flag_on(idx) || !self.enabled(idx) {
                continue;
            }
            let value = self.values.get(idx).map(String::as_str).unwrap_or("");
            match flag.kind {
                CliFlagKind::Toggle => tokens.push((idx, self.form(idx).to_string())),
                CliFlagKind::Value(_) => {
                    let token = if value.is_empty() {
                        self.form(idx).to_string()
                    } else {
                        format!("{} {value}", self.form(idx))
                    };
                    tokens.push((idx, token));
                }
                CliFlagKind::Positional(_) => {
                    if !value.is_empty() {
                        tokens.push((idx, value.to_string()));
                    }
                }
            }
        }
        tokens
    }

    /// Build the `polygit …` command string from the current selections.
    pub fn command(&self) -> String {
        let mut parts = vec!["polygit".to_string()];
        parts.extend(self.command_tokens().into_iter().map(|(_, token)| token));
        parts.join(" ")
    }
}

/// Periodic local branch/status refresh (no network). `Auto` re-checks every repo on an interval
/// that scales with the repo count (~repo_count/10 seconds, clamped 1..60).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BranchCheck {
    #[default]
    Off,
    Auto,
}

impl BranchCheck {
    pub fn cycle(self) -> Self {
        match self {
            BranchCheck::Off => BranchCheck::Auto,
            BranchCheck::Auto => BranchCheck::Off,
        }
    }
}

/// A repo-page tab. Branches/Worktrees/Stashes map to a `PageRowKind`; Commits is a read-only
/// list rendered separately (it doesn't flow through the row machinery).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoTab {
    Branches,
    Worktrees,
    Stashes,
    Commits,
}

impl RepoTab {
    /// The page-row kind this tab filters to.
    pub fn row_kind(self) -> Option<PageRowKind> {
        match self {
            RepoTab::Branches => Some(PageRowKind::Branch),
            RepoTab::Worktrees => Some(PageRowKind::Worktree),
            RepoTab::Stashes => Some(PageRowKind::Stash),
            RepoTab::Commits => Some(PageRowKind::Commit),
        }
    }

    /// The section's display name — also the key for the flat-view collapse set.
    pub fn section_name(self) -> &'static str {
        match self {
            RepoTab::Branches => "Branches",
            RepoTab::Worktrees => "Worktrees",
            RepoTab::Stashes => "Stashes",
            RepoTab::Commits => "Commits",
        }
    }
}

/// Whether the repo page splits its branches / worktrees / stashes into tabs instead of one
/// scrolling list. `Auto` tabs only when at least two of those sections are non-empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoTabsMode {
    #[default]
    Off,
    Auto,
}

impl RepoTabsMode {
    pub fn cycle(self) -> Self {
        match self {
            RepoTabsMode::Off => RepoTabsMode::Auto,
            RepoTabsMode::Auto => RepoTabsMode::Off,
        }
    }
}

/// The settings sections, in global row order: `(tab label, number of rows)`. Drives the tab
/// labels + which rows belong to each tab + the nav helpers (tab ranges). Row labels + tips live
/// in `SETTINGS`; the option values/handlers in `set_setting_option`/`settings_active_option`. The
/// `settings_tables_stay_consistent` test asserts these counts cover every `SETTINGS` row, so a
/// missed count bump fails the build. Appending a setting = add a `SETTINGS` entry, bump the count
/// here, and add the option/handler arms.
pub const SETTINGS_TABS: &[(&str, usize)] = &[
    ("Agent", 2),
    ("Interaction", 3),
    ("Layout", 6),
    ("Lists", 3),
    ("Pull requests", 1),
    ("Sync", 3),
    ("Theming", 7),
    ("Tooltips", 6),
];

/// Static descriptive data for one settings row: its label, its one-line tooltip, and any
/// option-specific tips (keyed by option index; empty for most rows). Co-locating the label and
/// its tip in ONE table is the single source of truth — they cannot drift apart on a reorder /
/// insert (which is exactly how every tooltip once came to describe the wrong setting). The
/// *behavior* (reading / writing the underlying field) still lives in `set_setting_option` /
/// `settings_active_option` keyed by the same global row index; the `settings_tables_consistent`
/// test guards that, the option labels, and the section counts against drift.
pub struct SettingInfo {
    pub label: &'static str,
    pub tip: &'static str,
    /// Per-option tips (index = option index). Empty when the single `tip` covers the whole row.
    pub option_tips: &'static [&'static str],
}

/// Every settings row in global (alphabetical-section) order — see `SETTINGS_TABS`. The ONLY place
/// row labels + tips live; everything else derives from this or is asserted against it.
pub const SETTINGS: [SettingInfo; 31] = [
    // Agent
    SettingInfo { label: "AI agent", tip: "Which AI agent `c` launches for the selected repo, run in its directory", option_tips: &[] },
    SettingInfo { label: "Skip permissions", tip: "Launch the agent with its skip-permissions flag (e.g. claude's --dangerously-skip-permissions)", option_tips: &[] },
    // Interaction
    SettingInfo { label: "Hover effects", tip: "Highlight actionable elements under the cursor (enables all-motion mouse tracking, which takes over terminal text selection)", option_tips: &[] },
    SettingInfo { label: "Changed-row flash", tip: "Pulse a row's changed cells after a pull. The status text column (t u) also marks what changed.", option_tips: &[] },
    SettingInfo { label: "Changed-row highlight", tip: "Steadily highlight a row's changed cells. The status text column (t u) also marks what changed.", option_tips: &[] },
    // Layout
    SettingInfo { label: "Panel padding", tip: "A 1-cell inner padding inside every bordered panel and modal", option_tips: &[] },
    SettingInfo { label: "Borders", tip: "Draw the rounded borders around the two main panes", option_tips: &[] },
    SettingInfo { label: "Pane splitter", tip: "Draw the draggable splitter grip between the panes", option_tips: &[] },
    SettingInfo { label: "Repo page tabs", tip: "Split the repo page into Branches/Worktrees/Stashes tabs (auto = when 2+ sections have rows)", option_tips: &[] },
    SettingInfo { label: "Repo page", tip: "Show the repo page as a docked bottom panel instead of full-screen (toggle with b)", option_tips: &[] },
    SettingInfo { label: "Auto branch-check", tip: "Periodically refresh each repo's local branch/status (no pull) — auto scales the interval with the repo count", option_tips: &[] },
    // Lists
    SettingInfo { label: "Grouping", tip: "Render the repo list as named group sections (from groups.json)", option_tips: &[] },
    SettingInfo { label: "Tree view", tip: "Render the repos as a collapsible directory tree", option_tips: &[] },
    SettingInfo { label: "Hide folder lines", tip: "Hide the dash-fill leader lines in group / folder headers", option_tips: &[] },
    // Pull requests
    SettingInfo { label: "Merged PRs", tip: "Show merged / closed PRs in the list's Pull Request column too (open PRs always show there; detail views always show the PR regardless)", option_tips: &[] },
    // Sync
    SettingInfo { label: "Auto-pull on launch", tip: "Pull every repo automatically on launch (off = pull on demand with e / E)", option_tips: &[] },
    SettingInfo { label: "Auto-pull limit", tip: "Skip the launch auto-pull above this many repos (∞ = no limit)", option_tips: &[] },
    SettingInfo { label: "Auto-pull in tree", tip: "Allow the launch auto-pull while the directory-tree view is active", option_tips: &[] },
    // Theming
    SettingInfo { label: "Icons", tip: "Glyph set for statuses, columns, and markers", option_tips: &[
        "Unicode glyphs can be colorized per type (e.g. the branch icon gets its own color); emoji use the font's own fixed colors",
        "Emoji glyphs render 2 cells wide and use the font's fixed colors",
    ] },
    SettingInfo { label: "Hide zeros", tip: "Hide zero-count column cells (a dim 0 becomes blank). Emoji mode always hides them.", option_tips: &[] },
    SettingInfo { label: "Theme", tip: "Color theme: auto-detect the terminal, or force dark / light", option_tips: &[] },
    SettingInfo { label: "Background", tip: "Surface tone: normal, soft, or terminal (let the terminal background show)", option_tips: &[] },
    SettingInfo { label: "Contrast", tip: "Strength of text + accent colors. normal = full-contrast text, vivid accents; soft = dimmer text, desaturated accents (gentler, lower contrast)", option_tips: &[] },
    SettingInfo { label: "List selection", tip: "Selected list-row highlight: a solid blue bar, or a subtle tint that keeps each column's own color", option_tips: &[] },
    SettingInfo { label: "Button hover", tip: "Button hover: reverse-video (inverted) or a soft tint, for footer/modal hints, tabs, radio chips, and keyboard keys", option_tips: &[] },
    // Tooltips
    SettingInfo { label: "All tooltips", tip: "Master switch for all hover tooltips. Off hides every tooltip; the per-area toggles below have no effect. (Tooltips still need Hover effects on.)", option_tips: &[] },
    SettingInfo { label: "Footer commands", tip: "Tooltips for the footer / status-bar commands (what each command does)", option_tips: &[] },
    SettingInfo { label: "Column headers", tip: "Tooltips for the column-title headers — also the source of the clickable [x] hide-column button", option_tips: &[] },
    SettingInfo { label: "Group counts", tip: "Tooltips for a group / folder header's right-corner count (its breakdown)", option_tips: &[] },
    SettingInfo { label: "Settings rows", tip: "Tooltips for the settings rows (what each setting does)", option_tips: &[] },
    SettingInfo { label: "Help links", tip: "Tooltips for the help / About links (the link's URL, browser-style)", option_tips: &[] },
];

/// Every settings row's label in global row order — DERIVED from `SETTINGS`, so the label list the
/// search filter / reset plan match against can never drift from the table.
pub const SETTINGS_LABELS: [&str; SETTINGS.len()] = {
    let mut out = [""; SETTINGS.len()];
    let mut index = 0;
    while index < SETTINGS.len() {
        out[index] = SETTINGS[index].label;
        index += 1;
    }
    out
};

/// Background tone for the active palette, independent of `Contrast`. `Soft` uses a gentler
/// surface; `Terminal` paints no base background, letting the terminal's own background show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    #[default]
    Normal,
    Soft,
    Terminal,
}

impl Background {
    /// Cycle Normal → Soft → Terminal → Normal.
    pub fn cycle(self) -> Self {
        match self {
            Background::Normal => Background::Soft,
            Background::Soft => Background::Terminal,
            Background::Terminal => Background::Normal,
        }
    }
}

/// What clicking an interactive element in the info block does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfoAction {
    /// Open a URL in the browser (clickable branch / commit / remote link).
    OpenUrl(String),
    /// Open the selected repo's pull request in the PR viewer modal (the "Pull Request" value).
    OpenPr,
    /// Copy text to the clipboard (a `⧉` button or a value).
    CopyText(String),
    /// Expand/collapse a truncated field, keyed by its label (e.g. "Path").
    ToggleExpand(String),
}

/// The semantic glyphs the UI renders, swappable between Unicode and emoji via `IconStyle`.
/// Only the recognizable status/column/marker icons live here — git file-status letters,
/// result-summary symbols, placeholders, and structural chars stay fixed.
pub struct IconSet {
    pub spinner: &'static [&'static str],
    pub queued: &'static str,
    pub up_to_date: &'static str,
    pub updated: &'static str,
    pub no_upstream: &'static str,
    pub skipped: &'static str,
    pub throttled: &'static str,
    pub failed: &'static str,
    /// Success check, distinct from `updated` — used for the all-ok Result row.
    pub ok: &'static str,
    pub dirty: &'static str,
    pub branches: &'static str,
    pub worktrees: &'static str,
    pub stashes: &'static str,
    /// The repo page's Commits section/tab.
    pub commits: &'static str,
    /// Commits the last pull landed (pulled-commits column).
    pub pulled: &'static str,
    /// Files the last pull changed (changed-files column).
    pub changed: &'static str,
    pub ahead: &'static str,
    pub behind: &'static str,
    pub warning: &'static str,
    pub skip_log: &'static str,
    pub retry_log: &'static str,
    /// Favorited / not-favorited star (favorites column).
    pub fav_on: &'static str,
    pub fav_off: &'static str,
    /// Pane window-control glyphs (top-border buttons): maximize, restore-from-maximized, close.
    pub maximize: &'static str,
    pub restore: &'static str,
    pub close: &'static str,
    /// Copy-to-clipboard button glyph (the result pane's log-copy button).
    pub copy: &'static str,
    /// External-link button glyph (open-in-browser; e.g. the info-panel PR's GitHub button).
    pub external: &'static str,
}

// Status glyphs are drawn from Geometric Shapes (U+25xx), which terminal fonts like Cascadia Code
// cover at a true single cell. Earlier circled-operator glyphs (⊘ ⊝, Math Operators) were missing
// from those fonts, so terminals substituted a double-width fallback and shifted the repo name.
pub static UNICODE_ICONS: IconSet = IconSet {
    spinner: &["◐", "◓", "◑", "◒"],
    queued: "◯",
    up_to_date: "◌",
    updated: "✓",
    no_upstream: "▽",
    skipped: "◇",
    throttled: "↯",
    failed: "✗",
    ok: "✓",
    dirty: "•",
    branches: "⑂",
    // Distinct from `branches` (inverted fork) — same OCR block so it renders at the same width.
    worktrees: "⑃",
    stashes: "≡",
    commits: "◉",
    pulled: "⇣",
    changed: "±",
    ahead: "↑",
    behind: "↓",
    warning: "⚠",
    skip_log: "◇",
    retry_log: "↻",
    fav_on: "★",
    fav_off: "☆",
    // Geometric Shapes (U+25xx), single-cell like the status glyphs above: hollow square = maximize,
    // square-in-square = restore, multiplication-X = close. Distinct shapes, reliable single-cell
    // width across terminal fonts (✕ is U+2715, distinct from the `failed` ✗).
    maximize: "▢",
    restore: "▣",
    close: "✕",
    copy: "⧉",
    external: "↗",
};

pub static EMOJI_ICONS: IconSet = IconSet {
    spinner: &["🌑", "🌓", "🌕", "🌗"],
    queued: "⏳",
    up_to_date: "✅",
    updated: "✨",
    // Single-codepoint Emoji_Presentation glyphs only — variation-selector emoji (⏭️, ⚠️) are
    // 2-char sequences that terminals render at inconsistent widths, breaking column alignment
    // and desyncing the cursor (garbled/ghosted UI). 🚫 / 🛑 are reliably 2 cells everywhere.
    no_upstream: "🔌",
    skipped: "🚫",
    throttled: "🐢",
    failed: "❌",
    ok: "✅",
    dirty: "📝",
    branches: "🌿",
    worktrees: "🌳",
    stashes: "📦",
    commits: "📜",
    pulled: "📥",
    changed: "📄",
    // Keep the compact 1-cell arrows for the tight ahead/behind numeric column — emoji arrows
    // are double-width and misalign it (and terminals disagree on their width).
    ahead: "↑",
    behind: "↓",
    warning: "🛑",
    // Log markers stay Unicode even in emoji mode: the marker is baked into the stored log line
    // at write time, so always using the clean Unicode glyph keeps logs consistent regardless of
    // the active icon style (or a style change after the line was written).
    skip_log: "⊘",
    retry_log: "↻",
    // The star stays a compact 1-cell symbol in both sets (like the ahead/behind arrows) so the
    // favorites column keeps a fixed width regardless of icon style.
    fav_on: "★",
    fav_off: "☆",
    // Window controls as single-codepoint Emoji_Presentation glyphs (2 cells, no variation selector):
    // white square button = maximize, black square button = restore, cross mark = close. The button
    // renderer measures display width, so the 2-cell glyphs lay out + hit-test correctly.
    maximize: "🗖",
    restore: "🗗",
    close: "❌",
    copy: "📋",
    external: "🔗",
};

/// A mouse-clickable command region in the status bar (rebuilt each render).
#[derive(Debug, Clone)]
pub struct ClickRegion {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub command: Command,
}

/// Which part of the UI a captured hover-tooltip region belongs to — so the per-area Tooltips
/// settings can gate it. (Footer-command, settings-row, and help-link tooltips don't go through
/// `hover_tooltips`; they're gated at their own dwell branches.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TooltipArea {
    /// A column-title header (also the source of the `[x]` hide-column button).
    Header,
    /// A group / folder header's right-corner count tail.
    Count,
}

/// A captured dwell-tooltip region: the hover hit-area, the text, the element the popup anchors to,
/// and the preferred side (the floating engine flips/shifts to keep it on-screen). Column headers
/// anchor to the full header cell with `bottom-start` (drop below, flipping above when cramped).
#[derive(Debug, Clone)]
pub struct TooltipRegion {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub text: String,
    pub anchor: Rect,
    pub placement: tui_pick::Placement,
    /// When set, the tooltip shows a clickable `[x]` that hides this list column.
    pub hide_column: Option<Column>,
    /// Which UI area this region belongs to (for the per-area Tooltips settings).
    pub area: TooltipArea,
}

fn default_true() -> bool {
    true
}

/// Per-area tooltip enablement. `enabled` is the master switch; the rest gate individual areas.
/// All default on (tooltips still require `hover_effects`, which provides the cursor tracking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TooltipPrefs {
    /// Footer / status-bar command tooltips (what each command does).
    #[serde(default = "default_true")]
    pub footer: bool,
    /// Column-title header tooltips (also carries the `[x]` hide-column button).
    #[serde(default = "default_true")]
    pub headers: bool,
    /// Group / folder header count-tail breakdown tooltips.
    #[serde(default = "default_true")]
    pub counts: bool,
    /// Settings-row tooltips (what each setting does).
    #[serde(default = "default_true")]
    pub settings: bool,
    /// Help / About link URL tooltips.
    #[serde(default = "default_true")]
    pub links: bool,
}

impl Default for TooltipPrefs {
    fn default() -> Self {
        Self { footer: true, headers: true, counts: true, settings: true, links: true }
    }
}

impl TooltipPrefs {
    /// Every area is on (the "All tooltips" master shows `on`).
    pub fn all_on(self) -> bool {
        self.footer && self.headers && self.counts && self.settings && self.links
    }
    /// Every area is off (the master shows `off`; mixed → neither radio is selected).
    pub fn all_off(self) -> bool {
        !self.footer && !self.headers && !self.counts && !self.settings && !self.links
    }
    /// Set every area on/off (the "All tooltips" bulk toggle).
    pub fn set_all(&mut self, value: bool) {
        self.footer = value;
        self.headers = value;
        self.counts = value;
        self.settings = value;
        self.links = value;
    }
}

/// The active dwell tooltip (after the ~1s dwell): text + the element it anchors to + preferred side.
#[derive(Debug, Clone)]
pub struct HoverTip {
    pub text: String,
    pub anchor: Rect,
    pub placement: tui_pick::Placement,
    /// When set, the tooltip shows a clickable `[x]` that hides this list column.
    pub hide_column: Option<Column>,
}

/// The keystroke a clickable hint stands in for. Clicking the hint injects this key, so it runs
/// through the exact same handler as the real key press — no per-action duplication. Used by the
/// repo-page and modal footers, which act through context-specific key matches, not `Command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintKey {
    Char(char),
    Enter,
    ShiftEnter,
    Tab,
    Esc,
}

/// A mouse-clickable hint region (repo page + modal footers), mapped to the key it triggers.
/// Rebuilt each render, in the same screen-space as `ClickRegion`.
#[derive(Debug, Clone, Copy)]
pub struct HintClick {
    pub row: u16,
    pub col_start: u16,
    pub col_end: u16,
    pub key: HintKey,
}

/// Which scrollable region a scrollbar drag targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollKind {
    List,
    Info,
    Preview,
    DiffFiles,
    DiffBody,
    Help,
    RepoPage,
    Keyboard,
    Settings,
    Changelog,
    BuildInfo,
    PrModal,
}

/// A draggable scrollbar registered at render time: where its track is + how much it scrolls.
#[derive(Debug, Clone, Copy)]
pub struct ScrollHit {
    pub kind: ScrollKind,
    /// The area the scrollbar was drawn on (its track sits on the right column).
    pub track: Rect,
    pub total: usize,
    pub viewport: usize,
}

/// A command dispatchable by key OR by clicking its status-bar hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Retry,
    RetryAll,
    Refetch,
    RefetchAll,
    Info,
    /// Toggle the result/log panel — the bottom half of the preview pane (same as `I`). Hidden, the
    /// info panel spans the whole pane.
    ToggleResultPanel,
    Help,
    OpenPage,
    SetFilter(StatusFilter),
    /// Close the active leader menu (the clickable `esc` in the filter/view/fold rows).
    LeaderCancel,
    /// Enter the name-filter input mode (same as `/`).
    NameFilter,
    /// Clear the active name filter (the clickable `[needle]` tag).
    ClearNameFilter,
    /// Toggle the Result overlay (same as Space).
    ResultOverlay,
    /// Cycle focus across the visible panels (same as Tab).
    FocusToggle,
    /// Narrow / widen the left pane (the clickable `[` / `]` hints).
    SplitNarrow,
    SplitWiden,
    /// Toggle the grouped list view (`v g`; hint shown only when groups exist).
    GroupingToggle,
    /// Toggle the directory-tree view (`v t`; hint shown only when nested folders exist).
    TreeToggle,
    /// Toggle the "★ Favorites" pinned-at-top section (`M`; hint shown only when favorites exist).
    FavoritesFirst,
    /// Collapse/expand a group by index (the group preview's clickable footer hint).
    ToggleGroupCollapsed(usize),
    /// Collapse every folder + collapsible group (`-` / `z M`).
    FoldCollapseAll,
    /// Expand every folder + group (`+`/`=` / `z R`).
    FoldExpandAll,
    /// Expand the selected header's subtree recursively (`*` / `z O`).
    FoldExpandSubtree,
    /// Toggle the per-repo diff view in the preview pane (same as `d`).
    DiffView,
    /// Start claude code in the selected repo (same as `c`).
    Claude,
    /// Open lazygit in the selected repo (same as `l`).
    Lazygit,
    /// Open the selected repo's remote in the browser (same as `o`).
    OpenRemote,
    /// Copy the selected repo's absolute path (same as `y`).
    CopyPath,
    /// Copy the selected repo's remote URL (same as `Y`).
    CopyRemote,
    Settings,
    /// Open the build-info modal (the clickable "built … ago" status-bar tag).
    ShowBuildInfo,
    /// Open the changelog modal (the clickable `vX.Y.Z` status-bar tag).
    ShowChangelog,
    /// Move the selection down / up (the clickable `j` / `k` move hints).
    NavDown,
    NavUp,
    /// Collapse-or-jump-to-parent / expand the selected header (the clickable `←` / `→` fold hints).
    NavLeft,
    NavRight,
    Quit,
}

impl Command {
    /// A one-line description shown as a tooltip after dwelling on the command's status-bar hint.
    pub fn tooltip(self) -> &'static str {
        match self {
            Command::Retry => {
                "Retry the selected repo (or every repo in the selected folder/group) that failed \
                 or was skipped"
            }
            Command::RetryAll => "Retry every repo that failed or was skipped",
            Command::Refetch => "Re-pull the selected repo (or every repo in the selected folder/group)",
            Command::RefetchAll => "Re-pull every repo from scratch",
            Command::Info => "Toggle the info panel for the selected repo",
            Command::ToggleResultPanel => {
                "Toggle the result/log panel (the bottom of the preview); hidden, the info panel \
                 fills the pane"
            }
            Command::Help => "Open the help modal (keys, flags, glyphs, about)",
            Command::OpenPage => "Open the selected repo's page: branches, worktrees, stashes",
            Command::SetFilter(StatusFilter::All) => "Clear the status filter",
            Command::SetFilter(_) => "Filter by this status",
            Command::LeaderCancel => "Close this menu",
            Command::NameFilter => "Filter repos by name (type to match)",
            Command::ClearNameFilter => "Clear the name filter",
            Command::ResultOverlay => "Show the Result / Errors summary",
            Command::FocusToggle => "Cycle focus across the visible panels",
            Command::SplitNarrow => "Narrow the left pane",
            Command::SplitWiden => "Widen the left pane",
            Command::GroupingToggle => "Toggle the grouped list view",
            Command::TreeToggle => "Toggle the directory-tree view",
            Command::FavoritesFirst => "Pin a ★ Favorites section to the top of the list",
            Command::ToggleGroupCollapsed(_) => "Collapse or expand this group",
            Command::FoldCollapseAll => "Collapse all folders and groups",
            Command::FoldExpandAll => "Expand all folders and groups",
            Command::FoldExpandSubtree => "Expand the selected subtree",
            Command::DiffView => "Toggle the diff view in the preview pane",
            Command::Claude => "Start claude code in the selected repo's directory",
            Command::Lazygit => "Open lazygit in the selected repo",
            Command::OpenRemote => "Open the selected repo's remote in your browser",
            Command::CopyPath => "Copy the selected repo's absolute path",
            Command::CopyRemote => "Copy the selected repo's remote (origin) URL",
            Command::Settings => "Open settings",
            Command::ShowBuildInfo => "Show when this build was made + reload to a newer one",
            Command::ShowChangelog => "Show the changelog (every release, newest first)",
            Command::NavDown => "Move the selection down",
            Command::NavUp => "Move the selection up",
            Command::NavLeft => "Collapse the selected folder/group (or jump to its parent)",
            Command::NavRight => "Expand the selected folder/group",
            Command::Quit => "Quit polygit",
        }
    }
}

/// What a confirmation dialog will do when accepted.
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteBranch { repo_idx: usize, branch: String, force: bool },
    DropStash { repo_idx: usize, index: usize },
    RemoveWorktree { repo_idx: usize, path: PathBuf, force: bool },
    DiscardChanges { repo_idx: usize, path: PathBuf },
    /// Reset every settings-modal preference to its default.
    ResetSettings,
    /// Download + install a specific released version over the running binary, then auto-reload.
    /// The dialog itself (danger flag + copyable return-to-latest line) carries the below-floor
    /// warning, so the action only needs the target version.
    PinVersion { version: String },
    /// A no-op accept — used by the design-system preview so "yes" just closes the dialog.
    Preview,
}

/// One row in the version picker: a published release merged with the embedded changelog notes
/// (present only for versions this build knows about, i.e. ≤ current).
#[derive(Debug, Clone)]
pub struct PinRelease {
    pub version: String,
    pub date: String,
    pub notes: Vec<String>,
    /// The running version — rendered `(current)` and inert.
    pub is_current: bool,
    /// Ships the in-app picker (≥ floor) — pinnable without the below-floor warning.
    pub is_supported: bool,
}

/// A yes/no confirmation modal.
#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub message: String,
    pub action: ConfirmAction,
    /// Destructive (loses uncommitted/unmerged work) — rendered with a scarier dialog.
    pub danger: bool,
    /// Tracked files a discard would revert (shown in the dialog body).
    pub restore_files: Vec<String>,
    /// Untracked files a discard would delete (shown in the dialog body).
    pub delete_files: Vec<String>,
    /// Generic pre-formatted body lines (e.g. the settings a reset will change), shown verbatim
    /// under the message below an optional `detail_title` header.
    pub detail_lines: Vec<String>,
    pub detail_title: Option<String>,
    /// A single click-to-copy line (e.g. the return-to-latest shell command for a below-floor pin),
    /// rendered highlighted with a copy glyph below the body. `None` = no copyable line.
    pub copy_line: Option<String>,
}

impl ConfirmDialog {
    /// A dialog with no detail body.
    pub fn simple(message: String, action: ConfirmAction, danger: bool) -> Self {
        Self {
            message,
            action,
            danger,
            restore_files: Vec::new(),
            delete_files: Vec::new(),
            detail_lines: Vec::new(),
            detail_title: None,
            copy_line: None,
        }
    }
}

/// Ring buffer capped at `RING_BUFFER_CAPACITY` lines.
#[derive(Debug, Default)]
pub struct LogBuffer {
    lines: VecDeque<String>,
}

impl LogBuffer {
    pub fn push(&mut self, line: String) {
        if self.lines.len() >= RING_BUFFER_CAPACITY {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn lines(&self) -> &VecDeque<String> {
        &self.lines
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

#[derive(Debug)]
pub struct RepoState {
    pub name: String,
    /// Path relative to THIS repo's discovery root, with `/` separators (e.g. "personal/polygit").
    /// Equals `name` for depth-1 repos. Drives display, name-filter, name-sort, and the tree.
    pub rel_path: String,
    /// The root (one of `AppState::root_dirs`) this repo was discovered under — disambiguates
    /// `rel_path` across multiple roots and groups repos into per-root sections in the tree.
    pub root: PathBuf,
    /// Hidden from the list (its root was removed from the workspace). The repos vec is append-only
    /// — indices must stay stable for in-flight workers — so removal hides rather than deletes.
    pub hidden: bool,
    /// Absolute index into `AppState::repos` (set at discovery). Lets a worker schedule its own
    /// backoff retry by index without threading the index through every call.
    pub index: usize,
    /// How many automatic throttle-backoff retries this repo has already had (capped).
    pub throttle_retries: u8,
    pub path: PathBuf,
    pub branch: Option<String>,
    /// Browsable https URL of the `origin` remote, discovered asynchronously.
    pub remote_url: Option<String>,
    pub status: RepoStatus,
    /// Short qualifier for the status column: the failure kind for a failed pull ("not found",
    /// "auth", "diverged", …) or "ref gone" for a deleted-upstream no-upstream. Cleared at the
    /// start of every pull.
    pub status_note: Option<&'static str>,
    /// What the most recent pull delivered (commits/files/sha delta + best-effort tag/branch
    /// counts). `Some` only after a pull *updated* the repo this session; cleared at pull start.
    pub pull_result: Option<PullResult>,
    /// Set when a pull updates the repo so the info panel re-fetches `details` (fresh sha,
    /// ahead/behind, …) the next time it's viewed. Cleared once details are refreshed.
    pub details_stale: bool,
    /// This repo's status/details were seeded from the persisted cache and it has NOT been
    /// pulled or refreshed this session — render it dimmed with an age. Cleared on any pull or
    /// fresh detail load.
    pub stale: bool,
    /// Unix seconds the cached entry was written (for the staleness age); `None` when not cached.
    pub cached_at: Option<i64>,
    /// Log ring buffer (stdout + stderr from git pull).
    pub log: LogBuffer,
    /// Whether the preview pane should auto-scroll to bottom.
    pub auto_scroll: bool,
    /// Preview pane scroll offset (lines from top).
    pub preview_scroll: usize,
    /// Info pane ([2]) scroll offset (lines from top) — independent per repo, like `preview_scroll`.
    pub info_scroll: usize,
    /// When this repo's pull began (after acquiring the concurrency permit).
    pub start: Option<Instant>,
    /// Wall-clock time spent on this repo, set when a terminal status is assigned.
    pub elapsed: Option<Duration>,
    /// Lazily-fetched info-panel details (last commit, ahead/behind, dirty/stash counts).
    pub details: Option<RepoDetails>,
    /// Guard so the details fetch is spawned at most once per repo.
    pub details_loading: bool,
    /// Open PR for the current branch (via `gh`), fetched lazily for the selected repo only.
    /// `None` when unchecked, when there's no open PR, or when `gh` is unavailable.
    pub pr: Option<PrInfo>,
    /// Guard: a `gh pr` lookup is in flight for this repo.
    pub pr_loading: bool,
    /// Unix seconds the current `pr` was resolved (drives the cache TTL, the re-query decision, and
    /// the info-panel age). `None` until resolved this session or seeded from a fresh cache entry;
    /// cleared after a pull so a newly-opened/closed PR is re-checked.
    pub pr_checked_at: Option<i64>,
    /// Transient diff-view buffer (filled lazily when the Diff view is opened).
    pub diff: Option<Vec<String>>,
    /// Dedicated repo-page data (branches + worktrees), filled lazily when the page opens.
    pub page: Option<RepoPageData>,
    /// Guard so the repo-page fetch is spawned at most once per open.
    pub page_loading: bool,
    /// True while a repo-page pull (`p`/`P`) is in flight, for the page spinner.
    pub pull_loading: bool,
    /// Which list cells changed in the last refetch (drives the attention flash).
    pub flash: CellFlash,
    /// When the current flash expires; None when not flashing.
    pub flash_until: Option<Instant>,
    /// Per-branch base-branch overrides for this repo (branch name → base ref). Seeded from the
    /// persisted global map when the page opens; the stats worker reads it to resolve each base.
    pub base_overrides: HashMap<String, String>,
}

/// Per-column "value just changed in the last refetch" flags. Cells with a flag set pulse
/// briefly (while `RepoState::flash_until` is in the future) to draw the eye to what changed.
#[derive(Debug, Clone, Copy, Default)]
pub struct CellFlash {
    pub status: bool,
    pub ahead_behind: bool,
    pub dirty: bool,
    pub last_commit: bool,
    pub branches: bool,
    pub stashes: bool,
    pub worktrees: bool,
}

impl CellFlash {
    pub fn any(&self) -> bool {
        self.status
            || self.ahead_behind
            || self.dirty
            || self.last_commit
            || self.branches
            || self.stashes
            || self.worktrees
    }
}

impl RepoState {
    pub fn new(name: impl Into<String>, path: PathBuf) -> Self {
        let name = name.into();
        RepoState {
            rel_path: name.clone(),
            root: PathBuf::new(),
            hidden: false,
            name,
            index: 0,
            throttle_retries: 0,
            path,
            branch: None,
            remote_url: None,
            status: RepoStatus::Queued,
            status_note: None,
            pull_result: None,
            details_stale: false,
            stale: false,
            cached_at: None,
            log: LogBuffer::default(),
            auto_scroll: true,
            preview_scroll: 0,
            info_scroll: 0,
            start: None,
            elapsed: None,
            details: None,
            details_loading: false,
            pr: None,
            pr_loading: false,
            pr_checked_at: None,
            diff: None,
            page: None,
            page_loading: false,
            pull_loading: false,
            flash: CellFlash::default(),
            flash_until: None,
            base_overrides: HashMap::new(),
        }
    }

    /// Whether the refetch flash should be visible *this instant*. Pulses on/off every 250ms
    /// while `flash_until` is in the future, so changed cells blink a few times then settle.
    pub fn flash_on(&self) -> bool {
        match self.flash_until {
            Some(until) => {
                let now = Instant::now();
                now < until && ((until - now).as_millis() / 250) % 2 == 1
            }
            None => false,
        }
    }

    /// Whether the post-change attention window is still active (the whole ~1s, not just the
    /// pulse-on phase) — drives the steady "highlight" change indicator.
    pub fn flash_active(&self) -> bool {
        self.flash_until.is_some_and(|until| Instant::now() < until)
    }

    /// Seed this repo's display from a cached entry (last-known status/branch/details). Marks it
    /// `stale` so it renders dimmed with an age until pulled or its details are freshly loaded.
    pub fn seed_from_cache(&mut self, cached: &crate::cache::CachedRepo) {
        self.status = cached.status.to_status();
        if let Some(branch) = &cached.branch {
            self.branch = Some(branch.clone());
        }
        self.details = cached.details.clone();
        self.pull_result = cached.pull_result.clone();
        self.stale = true;
        self.cached_at = Some(cached.updated_at);
    }
}

pub type SharedRepoState = Arc<Mutex<RepoState>>;

/// Shared concurrency gate + throttle bookkeeping for all pull paths (initial, retry, refetch).
/// The single `semaphore` caps concurrent pulls; the governor (in `worker`) holds "ballast"
/// permits to enforce a reduced `effective` cap when the remote throttles us, and restores the
/// full cap once things are quiet again. No `AppState` lock is ever held across its `.await`s.
pub struct ThrottleControl {
    pub semaphore: Arc<tokio::sync::Semaphore>,
    configured: usize,
    effective: std::sync::atomic::AtomicUsize,
    last_throttle: Mutex<Option<Instant>>,
    last_reduction: Mutex<Option<Instant>>,
    /// Backoff retries scheduled by workers, drained by the event loop into its retry queue.
    pending_retries: Mutex<Vec<(Instant, usize)>>,
}

impl ThrottleControl {
    /// How long a remote must stay quiet before the full concurrency cap is restored.
    pub const RECOVER_AFTER: Duration = Duration::from_secs(60);
    /// Debounce window: a burst of throttle errors within this span only halves the cap once.
    const DEBOUNCE: Duration = Duration::from_secs(5);

    pub fn new(max_jobs: usize) -> Arc<Self> {
        use std::sync::atomic::AtomicUsize;
        let cap = max_jobs.max(1);
        Arc::new(Self {
            semaphore: Arc::new(tokio::sync::Semaphore::new(cap)),
            configured: cap,
            effective: AtomicUsize::new(cap),
            last_throttle: Mutex::new(None),
            last_reduction: Mutex::new(None),
            pending_retries: Mutex::new(Vec::new()),
        })
    }

    pub fn configured(&self) -> usize {
        self.configured
    }

    pub fn effective(&self) -> usize {
        self.effective.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn reduced(&self) -> bool {
        self.effective() < self.configured
    }

    /// Whether a throttle was observed within the last minute (drives the warning banner).
    pub fn recently_throttled(&self) -> bool {
        self.last_throttle
            .lock()
            .unwrap()
            .is_some_and(|at| at.elapsed() < Self::RECOVER_AFTER)
    }

    /// Record a throttle event and halve the effective cap (min 1), debounced so one burst
    /// doesn't collapse it to 1 instantly. Returns the new effective cap.
    pub fn on_throttle(&self) -> usize {
        use std::sync::atomic::Ordering;
        let now = Instant::now();
        *self.last_throttle.lock().unwrap() = Some(now);
        {
            let mut last_reduction = self.last_reduction.lock().unwrap();
            if last_reduction.is_some_and(|prev| now.duration_since(prev) < Self::DEBOUNCE) {
                return self.effective();
            }
            *last_reduction = Some(now);
        }
        let new = (self.effective().max(1) / 2).max(1);
        self.effective.store(new, Ordering::Relaxed);
        new
    }

    /// Restore the full cap once the remote has been quiet for `RECOVER_AFTER`. Returns true
    /// when it actually changed (so the governor releases ballast).
    pub fn try_recover(&self) -> bool {
        let quiet = self
            .last_throttle
            .lock()
            .unwrap()
            .is_none_or(|at| at.elapsed() >= Self::RECOVER_AFTER);
        if quiet && self.reduced() {
            self.effective.store(self.configured, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Schedule `repo_idx` to be re-pulled at `at` (worker-side, on a throttle).
    pub fn schedule_retry(&self, repo_idx: usize, at: Instant) {
        self.pending_retries.lock().unwrap().push((at, repo_idx));
    }

    /// Drain and return the repo indices whose backoff has elapsed (event-loop side).
    pub fn take_due_retries(&self) -> Vec<usize> {
        let now = Instant::now();
        let mut pending = self.pending_retries.lock().unwrap();
        let mut due = Vec::new();
        pending.retain(|(at, idx)| {
            if *at <= now {
                due.push(*idx);
                false
            } else {
                true
            }
        });
        due
    }
}

/// A coarse "… ago" age for footer display ("just now", "5m ago", "3h ago", "2d ago").
pub fn format_ago(secs: u64) -> String {
    match secs {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{}m ago", secs / 60),
        3_600..=86_399 => format!("{}h ago", secs / 3_600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// A build/elapsed duration in seconds as "Ns" / "Nm Ms" / "Nm" (used by the build-info modal).
pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

/// Compact staleness age ("now"/"3m"/"5h"/"2d") for a status-cache entry stamped at `cached_at`
/// (Unix seconds). Reads the wall clock — display-only, never used in pure logic.
pub fn format_cache_age(cached_at: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(cached_at);
    let secs = (now - cached_at).max(0);
    match secs {
        0..=59 => "now".to_string(),
        60..=3_599 => format!("{}m", secs / 60),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// Cycle the auto-pull repo limit through its settings choices: 50 → 100 → 250 → ∞ (0) → 50.
pub fn next_auto_pull_limit(current: u32) -> u32 {
    match current {
        50 => 100,
        100 => 250,
        250 => 0,
        _ => 50,
    }
}

/// Whether `(col,row)` lands inside a `(row, col_start, col_end)` click region.
pub fn region_hit(region: Option<(u16, u16, u16)>, col: u16, row: u16) -> bool {
    region.is_some_and(|(region_row, start, end)| region_row == row && col >= start && col < end)
}

/// State.json key for a per-repo+branch base override: absolute repo path + US separator + branch.
pub fn base_override_key(repo_path: &std::path::Path, branch: &str) -> String {
    format!("{}\u{1f}{}", repo_path.display(), branch)
}

/// Whether `(col,row)` is inside `rect` (mouse hit-testing against captured modal areas).
pub fn point_in(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}

/// Worktree entry discovered from `<repo>.worktrees/<branch>/.git`.
#[derive(Debug, Clone)]
pub struct WorktreeEntry {
    pub repo: String,
    pub branch: String,
}

/// One row of the repo list. The list's logical selection space is `visible_rows()` indices,
/// then Result, then optional Errors. `depth` drives indentation in the tree view (and the
/// nesting of group headers / repos within a folder); it's 0 in the flat and grouped views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListRow {
    /// A directory node in the tree view. `node_idx` indexes `AppState::tree_nodes`.
    FolderHeader { node_idx: usize, depth: u16 },
    /// A group section header. `group_idx` indexes `AppState::groups`; `groups.len()` is the
    /// implicit ungrouped section. `parent` is the enclosing folder node in the tree+groups
    /// view (None at the top level). Static (non-collapsible) headers aren't selectable.
    GroupHeader { group_idx: usize, parent: Option<usize>, collapsible: bool, depth: u16 },
    /// A repo row. `repo_idx` is the absolute index into `AppState::repos`.
    Repo { repo_idx: usize, depth: u16 },
    /// The pinned "★ Favorites" section header (favorites-first mode). Not collapsible.
    FavoritesHeader,
    /// A blank line between sections — never selectable or clickable.
    Spacer,
}

impl ListRow {
    /// A top-level repo row (flat / grouped view) at depth 0.
    pub fn repo(repo_idx: usize) -> Self {
        ListRow::Repo { repo_idx, depth: 0 }
    }

    /// A top-level group header (grouped view) at depth 0, no enclosing folder.
    #[cfg(test)]
    pub fn group(group_idx: usize, collapsible: bool) -> Self {
        ListRow::GroupHeader { group_idx, parent: None, collapsible, depth: 0 }
    }
}

/// One folder node in the directory tree (the scan root has no node; its direct repos render
/// at the top of the tree view). Built from the repos' relative paths by `build_tree`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    /// Folder path relative to the scan root (e.g. "work/clients") — the collapse key + identity.
    pub rel_path: String,
    /// The folder's own name (last path component).
    pub name: String,
    /// Depth from the root (0 for top-level folders).
    pub depth: u16,
    /// Index of the enclosing folder node, if any.
    pub parent: Option<usize>,
    /// Child folder node indices, sorted by name.
    pub children: Vec<usize>,
    /// Absolute indices of repos that live directly in this folder.
    pub repos: Vec<usize>,
}

/// Build the folder-tree node model from `(repo_idx, rel_path)` pairs. Repos whose `rel_path`
/// has no `/` belong to the implicit root and get no node (they render at the top of the tree).
/// Nodes are returned in a stable pre-order, children sorted by name. Pure + unit-tested.
/// Favorite key for a repo: its absolute path as a string (unambiguous across roots).
pub(crate) fn favorite_key(path: &std::path::Path) -> String {
    path.display().to_string()
}

/// The last path component of a root (its display name), or the full path when it has none.
pub(crate) fn root_basename(root: &std::path::Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| root.display().to_string())
}

/// A root rendered relative to `$HOME` as `~/…`, falling back to the absolute path. Used to
/// disambiguate root labels in the tree forest when two roots share a basename.
pub(crate) fn home_relative(root: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = root.strip_prefix(&home) {
            return format!("~/{}", rest.display().to_string().replace(std::path::MAIN_SEPARATOR, "/"));
        }
    }
    root.display().to_string()
}

pub fn build_tree(repos: &[(usize, String)]) -> Vec<TreeNode> {
    use std::collections::BTreeMap;
    // Map folder rel_path → node index, creating ancestors on demand.
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    let mut nodes: Vec<TreeNode> = Vec::new();

    // Ensure a node (and all its ancestors) exist for `folder`, returning its index.
    fn ensure(
        folder: &str,
        index: &mut BTreeMap<String, usize>,
        nodes: &mut Vec<TreeNode>,
    ) -> usize {
        if let Some(&idx) = index.get(folder) {
            return idx;
        }
        let (parent, name) = match folder.rsplit_once('/') {
            Some((parent, name)) => (Some(ensure(parent, index, nodes)), name.to_string()),
            None => (None, folder.to_string()),
        };
        let depth = parent.map_or(0, |parent_idx| nodes[parent_idx].depth + 1);
        let idx = nodes.len();
        nodes.push(TreeNode {
            rel_path: folder.to_string(),
            name,
            depth,
            parent,
            children: Vec::new(),
            repos: Vec::new(),
        });
        if let Some(parent_idx) = parent {
            nodes[parent_idx].children.push(idx);
        }
        index.insert(folder.to_string(), idx);
        idx
    }

    for (repo_idx, rel_path) in repos {
        if let Some((folder, _)) = rel_path.rsplit_once('/') {
            let node_idx = ensure(folder, &mut index, &mut nodes);
            nodes[node_idx].repos.push(*repo_idx);
        }
    }

    // Sort each node's child folders by name for a stable display order.
    let mut order: Vec<(usize, Vec<usize>)> = Vec::new();
    for (idx, node) in nodes.iter().enumerate() {
        let mut children = node.children.clone();
        children.sort_by(|&a, &b| nodes[a].name.cmp(&nodes[b].name));
        order.push((idx, children));
    }
    for (idx, children) in order {
        nodes[idx].children = children;
    }
    nodes
}

/// Runtime state of one configured group (config + resolved membership + resolution status).
#[derive(Debug)]
pub struct GroupRuntime {
    pub name: String,
    pub source: GroupSource,
    /// Resolved member names, lowercased. None = dynamic source not resolved yet
    /// (pattern sources match by name and keep this None).
    pub members: Option<Vec<String>>,
    /// A dynamic resolve is in flight (drives the header spinner).
    pub resolving: bool,
    /// Last resolution (or config-validation) error; cached members stay in effect.
    pub error: Option<String>,
    /// Unix seconds of the last successful dynamic resolve (drives cache freshness/age).
    pub resolved_at: Option<u64>,
}

impl GroupRuntime {
    /// Whether a repo belongs to this group. A pattern containing `/` matches the repo's
    /// relative path (e.g. `work/*`); a pattern without `/` matches the basename (the legacy
    /// behavior, so existing configs are unaffected). Static/dynamic member lists match the
    /// basename. Both arguments are lowercased.
    pub fn contains(&self, name_lower: &str, rel_lower: &str) -> bool {
        match &self.source {
            GroupSource::Pattern(pattern) => {
                let target = if pattern.contains('/') { rel_lower } else { name_lower };
                groups::wildcard_match(pattern, target)
            }
            _ => self
                .members
                .as_ref()
                .is_some_and(|members| members.iter().any(|member| member == name_lower)),
        }
    }
}

/// A transient toast: a headline plus optional dimmed preview lines (e.g. the start of
/// just-copied clipboard text). Auto-dismisses after `AppState::TOAST_DURATION`.
#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub preview: Vec<String>,
    pub shown_at: Instant,
}

/// The first `COPY_PREVIEW_LINES` lines of `copied` for a copy-confirmation toast, with a
/// trailing "+N more lines" marker when the text is longer.
pub fn copy_preview(copied: &str) -> Vec<String> {
    let total = copied.lines().count();
    let mut preview: Vec<String> = copied
        .lines()
        .take(AppState::COPY_PREVIEW_LINES)
        .map(str::to_string)
        .collect();
    if total > AppState::COPY_PREVIEW_LINES {
        preview.push(format!("… +{} more lines", total - AppState::COPY_PREVIEW_LINES));
    }
    preview
}
