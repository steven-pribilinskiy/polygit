//! Executing the clone rows of a plan: bounded concurrency, live progress, and a cancel that
//! leaves nothing half-written.
//!
//! Destinations are not computed here — [`crate::layout::plan`] already decided them, and having
//! two things that pick a path is how they come to disagree. This module only decides whether each
//! target should be attempted, runs the ones that should, and reports what happened to every one.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;

use crate::app::ThrottleControl;

/// How to clone. Full history by default: a clone under a projects tree is a working repo, and a
/// shallow one cannot serve `log`, `blame`, or a worktree off another branch.
#[derive(Debug, Clone, Default)]
pub struct CloneOptions {
    /// `--filter=blob:none`. Keeps full history while deferring file contents — the fast option at
    /// org scale, and unlike `--depth` it does not truncate anything.
    pub blobless: bool,
    /// `--depth N`. For throwaway clones only.
    pub depth: Option<u32>,
    /// Skip anything larger than this. Off by default, and every skip is reported.
    pub max_size_kb: Option<u64>,
}

impl CloneOptions {
    /// Extra arguments for `gh repo clone … -- <flags>`. Empty means a plain full clone.
    pub fn git_flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        if self.blobless {
            flags.push("--filter=blob:none".to_string());
        }
        if let Some(depth) = self.depth {
            flags.push(format!("--depth={depth}"));
        }
        flags
    }
}

/// One repo to clone, with the destination the plan chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneTarget {
    pub owner: String,
    pub name: String,
    pub dest: PathBuf,
    /// GitHub's reported size in kilobytes; 0 when unknown.
    pub size_kb: u64,
}

impl CloneTarget {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// Why a target was not attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// Something is already at the destination. Never overwritten.
    AlreadyExists,
    TooLarge { size_kb: u64, limit_kb: u64 },
    /// The run was cancelled before this target started.
    Cancelled,
}

impl SkipReason {
    pub fn label(&self) -> String {
        match self {
            SkipReason::AlreadyExists => "already there".to_string(),
            SkipReason::TooLarge { size_kb, limit_kb } => {
                format!("{} over the {} limit", format_size(*size_kb), format_size(*limit_kb))
            }
            SkipReason::Cancelled => "cancelled".to_string(),
        }
    }
}

/// What happened to one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneOutcome {
    Cloned,
    Skipped(SkipReason),
    Failed(String),
}

impl CloneOutcome {
    pub fn label(&self) -> String {
        match self {
            CloneOutcome::Cloned => "cloned".to_string(),
            CloneOutcome::Skipped(reason) => format!("skipped — {}", reason.label()),
            CloneOutcome::Failed(error) => format!("failed — {error}"),
        }
    }
}

/// Live state of a run, shared with whatever renders it.
#[derive(Debug, Clone, Default)]
pub struct CloneProgress {
    pub total: usize,
    pub done: usize,
    /// Slugs currently in flight, in start order.
    pub running: Vec<String>,
    pub results: Vec<(CloneTarget, CloneOutcome)>,
}

impl CloneProgress {
    pub fn new(total: usize) -> Self {
        Self { total, ..Default::default() }
    }

    pub fn cloned(&self) -> usize {
        self.results.iter().filter(|(_, outcome)| *outcome == CloneOutcome::Cloned).count()
    }

    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|(_, outcome)| matches!(outcome, CloneOutcome::Failed(_)))
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.results
            .iter()
            .filter(|(_, outcome)| matches!(outcome, CloneOutcome::Skipped(_)))
            .count()
    }

    /// One line summarising a finished run.
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("{} cloned", self.cloned())];
        if self.skipped() > 0 {
            parts.push(format!("{} skipped", self.skipped()));
        }
        if self.failed() > 0 {
            parts.push(format!("{} failed", self.failed()));
        }
        parts.join(", ")
    }
}

/// Human-readable size from kilobytes, matching how GitHub reports `diskUsage`.
pub fn format_size(kilobytes: u64) -> String {
    if kilobytes < 1024 {
        format!("{kilobytes} KB")
    } else if kilobytes < 1024 * 1024 {
        format!("{:.1} MB", kilobytes as f64 / 1024.0)
    } else {
        format!("{:.1} GB", kilobytes as f64 / (1024.0 * 1024.0))
    }
}

/// Parse `2GB` / `500MB` / `4096` into kilobytes. `0` disables the limit.
pub fn parse_size_to_kb(input: &str) -> Result<u64, String> {
    let trimmed = input.trim().to_uppercase();
    if trimmed.is_empty() {
        return Err("empty size".to_string());
    }
    let split = trimmed.find(|ch: char| !ch.is_ascii_digit() && ch != '.').unwrap_or(trimmed.len());
    let (digits, unit) = trimmed.split_at(split);
    if digits.is_empty() {
        return Err(format!("'{input}' has no number"));
    }
    if digits.contains('.') {
        return Err(format!(
            "'{input}': fractional sizes are not supported — use a smaller unit (500MB, not 0.5GB)"
        ));
    }
    let value: u64 = digits.parse().map_err(|_| format!("'{input}' is not a number"))?;
    match unit.trim() {
        "GB" | "G" => Ok(value * 1024 * 1024),
        "MB" | "M" => Ok(value * 1024),
        "KB" | "K" | "" => Ok(value),
        other => Err(format!("unknown size unit '{other}' — use GB, MB or KB")),
    }
}

/// Whether a target should be attempted, deciding every skip up front so the reason is reportable
/// rather than surfacing as a clone failure. `exists` answers "is anything at this path".
pub fn skip_reason(
    target: &CloneTarget,
    options: &CloneOptions,
    exists: impl Fn(&Path) -> bool,
) -> Option<SkipReason> {
    if exists(&target.dest) {
        return Some(SkipReason::AlreadyExists);
    }
    match options.max_size_kb {
        Some(limit) if limit > 0 && target.size_kb > limit => {
            Some(SkipReason::TooLarge { size_kb: target.size_kb, limit_kb: limit })
        }
        _ => None,
    }
}

/// Clones one repo. Boxed so a test can substitute one that touches nothing.
pub type CloneFn =
    Arc<dyn Fn(CloneTarget, CloneOptions) -> BoxFuture<'static, Result<(), String>> + Send + Sync>;

/// The real clone: `gh repo clone`, which carries auth for private repos without depending on a
/// locally-configured credential helper.
pub fn gh_clone_fn() -> CloneFn {
    Arc::new(|target: CloneTarget, options: CloneOptions| {
        Box::pin(async move {
            crate::git::gh_clone_repo(&target.slug(), &target.dest, &options.git_flags()).await
        })
    })
}

/// Run every target, bounded by the shared pull semaphore so clones and pulls answer to one cap and
/// one throttle response. Cancelling stops the queue; clones already in flight are left to finish,
/// because killing `git clone` midway leaves a half-written directory behind.
pub async fn run_clone(
    targets: Vec<CloneTarget>,
    options: CloneOptions,
    progress: Arc<Mutex<CloneProgress>>,
    cancel: Arc<AtomicBool>,
    control: Arc<ThrottleControl>,
    clone_fn: CloneFn,
) {
    {
        let mut state = progress.lock().unwrap();
        state.total = targets.len();
    }
    let mut running = futures::stream::FuturesUnordered::new();

    for target in targets {
        if cancel.load(Ordering::Relaxed) {
            record(&progress, target, CloneOutcome::Skipped(SkipReason::Cancelled));
            continue;
        }
        if let Some(reason) = skip_reason(&target, &options, |path| path.exists()) {
            record(&progress, target, CloneOutcome::Skipped(reason));
            continue;
        }
        let permit = match Arc::clone(&control.semaphore).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };
        {
            let mut state = progress.lock().unwrap();
            state.running.push(target.slug());
        }
        let progress_handle = Arc::clone(&progress);
        let options_handle = options.clone();
        let clone_handle = Arc::clone(&clone_fn);
        running.push(tokio::spawn(async move {
            let _permit = permit;
            let slug = target.slug();
            let outcome = match clone_handle(target.clone(), options_handle).await {
                Ok(()) => CloneOutcome::Cloned,
                Err(error) => CloneOutcome::Failed(error),
            };
            let mut state = progress_handle.lock().unwrap();
            state.running.retain(|entry| entry != &slug);
            state.done += 1;
            state.results.push((target, outcome));
        }));

        // Drain finished tasks so `running` cannot grow without bound on a long queue.
        while running.len() >= control.effective().max(1) {
            use futures::StreamExt;
            if running.next().await.is_none() {
                break;
            }
        }
    }
    use futures::StreamExt;
    while running.next().await.is_some() {}
}

fn record(progress: &Arc<Mutex<CloneProgress>>, target: CloneTarget, outcome: CloneOutcome) {
    let mut state = progress.lock().unwrap();
    state.done += 1;
    state.results.push((target, outcome));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str, size_kb: u64) -> CloneTarget {
        CloneTarget {
            owner: "acme".into(),
            name: name.into(),
            dest: PathBuf::from("/root").join(name),
            size_kb,
        }
    }

    #[test]
    fn size_parsing_matches_the_units_people_type() {
        assert_eq!(parse_size_to_kb("2GB").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_size_to_kb("500MB").unwrap(), 512_000);
        assert_eq!(parse_size_to_kb("4096").unwrap(), 4096);
        assert_eq!(parse_size_to_kb("4096KB").unwrap(), 4096);
        assert_eq!(parse_size_to_kb(" 1 g ").unwrap(), 1024 * 1024);
        assert_eq!(parse_size_to_kb("0").unwrap(), 0);
    }

    #[test]
    fn size_parsing_rejects_what_it_cannot_represent() {
        // Fractions are rejected rather than truncated: 0.5GB silently becoming 0 would skip
        // nothing at all, which looks exactly like the limit working.
        assert!(parse_size_to_kb("0.5GB").unwrap_err().contains("fractional"));
        assert!(parse_size_to_kb("GB").unwrap_err().contains("no number"));
        assert!(parse_size_to_kb("12TB").unwrap_err().contains("unknown size unit"));
        assert!(parse_size_to_kb("").unwrap_err().contains("empty"));
    }

    #[test]
    fn size_formatting_reads_like_github() {
        assert_eq!(format_size(512), "512 KB");
        assert_eq!(format_size(1536), "1.5 MB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn an_occupied_destination_is_a_skip_not_a_failure() {
        let subject = target("billing", 10);
        let reason = skip_reason(&subject, &CloneOptions::default(), |_| true);
        assert_eq!(reason, Some(SkipReason::AlreadyExists));
        assert_eq!(skip_reason(&subject, &CloneOptions::default(), |_| false), None);
    }

    #[test]
    fn the_size_limit_is_off_until_asked_for_and_reports_when_it_fires() {
        let big = target("huge", 5 * 1024 * 1024);
        // Default: no limit, so a 5 GB repo is attempted.
        assert_eq!(skip_reason(&big, &CloneOptions::default(), |_| false), None);

        let limited = CloneOptions { max_size_kb: Some(1024 * 1024), ..CloneOptions::default() };
        let reason = skip_reason(&big, &limited, |_| false).unwrap();
        assert!(reason.label().contains("5.0 GB"));
        assert!(reason.label().contains("1.0 GB"));

        // A zero limit disables the check rather than skipping everything.
        let zeroed = CloneOptions { max_size_kb: Some(0), ..CloneOptions::default() };
        assert_eq!(skip_reason(&big, &zeroed, |_| false), None);
    }

    #[test]
    fn flags_default_to_a_plain_full_clone() {
        assert!(CloneOptions::default().git_flags().is_empty());
        let blobless = CloneOptions { blobless: true, ..CloneOptions::default() };
        assert_eq!(blobless.git_flags(), vec!["--filter=blob:none"]);
        let shallow = CloneOptions { depth: Some(1), ..CloneOptions::default() };
        assert_eq!(shallow.git_flags(), vec!["--depth=1"]);
    }

    #[tokio::test]
    async fn every_target_is_attempted_and_recorded() {
        let targets = vec![target("one", 1), target("two", 1)];
        let progress = Arc::new(Mutex::new(CloneProgress::new(targets.len())));
        let cancel = Arc::new(AtomicBool::new(false));
        let control = ThrottleControl::new(4);
        let clone_fn: CloneFn = Arc::new(|_, _| Box::pin(async { Ok(()) }));

        run_clone(targets, CloneOptions::default(), Arc::clone(&progress), cancel, control, clone_fn)
            .await;

        let state = progress.lock().unwrap();
        assert_eq!(state.done, 2);
        assert_eq!(state.cloned(), 2);
        assert!(state.running.is_empty());
        assert_eq!(state.summary(), "2 cloned");
    }

    #[tokio::test]
    async fn a_failure_is_kept_rather_than_lost_to_a_toast() {
        let targets = vec![target("bad", 1)];
        let progress = Arc::new(Mutex::new(CloneProgress::new(1)));
        let clone_fn: CloneFn =
            Arc::new(|_, _| Box::pin(async { Err("remote hung up".to_string()) }));

        run_clone(
            targets,
            CloneOptions::default(),
            Arc::clone(&progress),
            Arc::new(AtomicBool::new(false)),
            ThrottleControl::new(2),
            clone_fn,
        )
        .await;

        let state = progress.lock().unwrap();
        assert_eq!(state.failed(), 1);
        assert!(state.results[0].1.label().contains("remote hung up"));
        assert_eq!(state.summary(), "0 cloned, 1 failed");
    }

    #[tokio::test]
    async fn cancelling_stops_the_queue_and_says_so_per_target() {
        let targets = vec![target("one", 1), target("two", 1), target("three", 1)];
        let progress = Arc::new(Mutex::new(CloneProgress::new(targets.len())));
        let cancel = Arc::new(AtomicBool::new(true));
        let clone_fn: CloneFn =
            Arc::new(|_, _| Box::pin(async { panic!("must not clone once cancelled") }));

        run_clone(
            targets,
            CloneOptions::default(),
            Arc::clone(&progress),
            cancel,
            ThrottleControl::new(2),
            clone_fn,
        )
        .await;

        let state = progress.lock().unwrap();
        assert_eq!(state.skipped(), 3);
        assert!(
            state
                .results
                .iter()
                .all(|(_, outcome)| *outcome == CloneOutcome::Skipped(SkipReason::Cancelled))
        );
    }
}
