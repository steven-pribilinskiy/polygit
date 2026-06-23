use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Semaphore;

use crate::app::WorktreeEntry;
use crate::git::{classify_pull_output, diff_stat, discover_worktrees, get_branch, is_dirty, PullOutcome};

/// Streaming (non-TUI) output. For a single-level scan (`--depth 1` / a flat directory) the
/// output matches the bash reference byte-for-byte; a recursive scan additionally lists repos
/// found in nested folders, named by their path relative to the scan root.
pub async fn run_plain(
    roots: &[PathBuf],
    max_jobs: usize,
    max_depth: usize,
    timeout_secs: u64,
    no_worktrees: bool,
    profiling: bool,
    profile_out: Option<&Path>,
) -> Result<i32> {
    let where_label = if roots.len() == 1 {
        roots[0]
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string())
    } else {
        format!("{} folders", roots.len())
    };

    println!("🔄 Pulling all repositories in {where_label}...");

    // Discover repos across every root (recursively, pruned; `--depth 1` keeps the legacy
    // single-level scan). Each repo is paired with the root it was found under (for relative paths)
    // and deduped across overlapping roots.
    let mut repos: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for root in roots {
        for path in crate::git::discover_repos_recursive(root, max_depth).await? {
            if seen.insert(path.clone()) {
                repos.push((root.clone(), path));
            }
        }
    }

    if repos.is_empty() {
        println!();
        println!("🎉 Pull completed!");
        println!();
        println!("   No git repositories found in {where_label}.");
        return Ok(0);
    }

    // Start worktree discovery concurrently across all roots.
    let worktrees_future = if no_worktrees {
        tokio::spawn(async { Vec::<WorktreeEntry>::new() })
    } else {
        let roots: Vec<PathBuf> = roots.to_vec();
        tokio::spawn(async move {
            let mut out = Vec::new();
            for root in &roots {
                if let Ok(entries) = discover_worktrees(root).await {
                    out.extend(
                        entries.into_iter().map(|(repo, branch)| WorktreeEntry { repo, branch }),
                    );
                }
            }
            out
        })
    };

    // Structure to hold per-repo results, ordered alphabetically
    struct RepoResult {
        name: String,
        branch: String,
        output: String,
        state: &'static str,
        elapsed: std::time::Duration,
        last_log: String,
    }

    let semaphore = Arc::new(Semaphore::new(max_jobs));
    let results: Arc<Mutex<Vec<Option<RepoResult>>>> = {
        let mut initial = Vec::with_capacity(repos.len());
        for _ in 0..repos.len() {
            initial.push(None);
        }
        Arc::new(Mutex::new(initial))
    };

    let mut handles = Vec::new();

    for (idx, (root, path)) in repos.iter().enumerate() {
        let path = path.clone();
        let name = crate::git::relative_path(root, &path);
        let semaphore = Arc::clone(&semaphore);
        let results = Arc::clone(&results);
        let timeout = timeout_secs;

        let handle = tokio::spawn(async move {
            let _permit = semaphore.acquire_owned().await.unwrap();

            let started = std::time::Instant::now();

            let branch = get_branch(&path).await.unwrap_or_else(|_| "?".to_string());

            // Check dirty
            let dirty = is_dirty(&path).await.unwrap_or(false);
            if dirty {
                let output = format!("⚠️  Skipping {name} (has uncommitted changes)\n");
                let mut guard = results.lock().unwrap();
                guard[idx] = Some(RepoResult {
                    name,
                    branch,
                    output,
                    state: "skipped",
                    elapsed: std::time::Duration::ZERO,
                    last_log: "uncommitted changes".to_string(),
                });
                return;
            }

            // Run git pull
            let mut child = match Command::new("git")
                .args(["-C", path.to_str().unwrap_or("."), "pull", "--ff-only"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(err) => {
                    let output = format!("❌ Failed: {name}\n   {err}\n\n");
                    let mut guard = results.lock().unwrap();
                    guard[idx] = Some(RepoResult {
                        name,
                        branch,
                        output,
                        state: "failed",
                        elapsed: started.elapsed(),
                        last_log: err.to_string(),
                    });
                    return;
                }
            };

            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();

            let stdout_task = tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                let mut collected = String::new();
                while let Ok(Some(line)) = lines.next_line().await {
                    collected.push_str(&line);
                    collected.push('\n');
                }
                collected
            });

            let stderr_task = tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                let mut collected = String::new();
                while let Ok(Some(line)) = lines.next_line().await {
                    collected.push_str(&line);
                    collected.push('\n');
                }
                collected
            });

            // Bound the pull with tokio's timer (cross-platform; no external `timeout` coreutil).
            let mut timed_out = false;
            let status =
                match tokio::time::timeout(std::time::Duration::from_secs(timeout), child.wait())
                    .await
                {
                    Ok(res) => res.unwrap(),
                    Err(_) => {
                        timed_out = true;
                        let _ = child.start_kill();
                        child.wait().await.unwrap()
                    }
                };
            let exit_success = status.success() && !timed_out;

            let stdout_output = stdout_task.await.unwrap_or_default();
            let stderr_output = stderr_task.await.unwrap_or_default();
            let mut combined = format!("{stdout_output}{stderr_output}");
            if timed_out {
                combined.push_str(&format!("pull timed out after {timeout}s\n"));
            }

            let outcome = classify_pull_output(&combined, exit_success);

            let last_log = combined
                .lines()
                .rev()
                .map(|line| line.trim())
                .find(|line| !line.is_empty())
                .unwrap_or("")
                .to_string();

            let (output, state) = match outcome {
                PullOutcome::AlreadyUpToDate => {
                    (format!("✅ {name}\n"), "uptodate")
                }
                PullOutcome::NoUpstream => {
                    (format!("🔌 {name} (no upstream)\n"), "noupstream")
                }
                PullOutcome::Throttled => {
                    // Plain mode is a one-shot batch — no auto-retry/backoff (TUI-only).
                    (format!("🐢 {name} (throttled)\n"), "throttled")
                }
                PullOutcome::Updated => {
                    let stat = diff_stat(&path).await.unwrap_or_default();
                    let stat_indented = if stat.is_empty() {
                        String::new()
                    } else {
                        format!("{stat}\n\n")
                    };
                    (format!("✅ {name}\n{stat_indented}"), "updated")
                }
                PullOutcome::Failed => {
                    // Indent log output with "   " prefix
                    let log_indented: String = combined
                        .lines()
                        .map(|line| format!("   {line}\n"))
                        .collect();
                    (format!("❌ Failed: {name}\n{log_indented}\n"), "failed")
                }
            };

            let mut guard = results.lock().unwrap();
            guard[idx] = Some(RepoResult {
                name,
                branch,
                output,
                state,
                elapsed: started.elapsed(),
                last_log,
            });
        });

        handles.push(handle);
    }

    // Wait for all and print in alphabetical order
    for handle in handles {
        let _ = handle.await;
    }

    // Scope the lock so it is released before the later `.await` (no lock held across await).
    let (updated, up_to_date, skipped, no_upstream, throttled, failed) = {
        let guard = results.lock().unwrap();
        let mut updated = Vec::new();
        let mut up_to_date = Vec::new();
        let mut skipped = Vec::new();
        let mut no_upstream = Vec::new();
        let mut throttled = Vec::new();
        let mut failed = Vec::new();

        for result in guard.iter().flatten() {
            print!("{}", result.output);
            match result.state {
                "updated" => updated.push((result.name.clone(), result.branch.clone())),
                "uptodate" => up_to_date.push((result.name.clone(), result.branch.clone())),
                "skipped" => skipped.push((result.name.clone(), result.branch.clone())),
                "noupstream" => no_upstream.push((result.name.clone(), result.branch.clone())),
                "throttled" => throttled.push((result.name.clone(), result.branch.clone())),
                "failed" => failed.push((result.name.clone(), result.branch.clone())),
                _ => {}
            }
        }
        (updated, up_to_date, skipped, no_upstream, throttled, failed)
    };

    println!();
    println!("🎉 Pull completed!");

    let total = updated.len()
        + up_to_date.len()
        + skipped.len()
        + no_upstream.len()
        + throttled.len()
        + failed.len();
    let mut parts = Vec::new();
    if !updated.is_empty() {
        parts.push(format!("{} updated", updated.len()));
    }
    if !up_to_date.is_empty() {
        parts.push(format!("{} up-to-date", up_to_date.len()));
    }
    if !skipped.is_empty() {
        parts.push(format!("{} skipped", skipped.len()));
    }
    if !no_upstream.is_empty() {
        parts.push(format!("{} no-upstream", no_upstream.len()));
    }
    if !throttled.is_empty() {
        parts.push(format!("{} throttled", throttled.len()));
    }
    if !failed.is_empty() {
        parts.push(format!("{} failed", failed.len()));
    }

    println!();
    println!("   {total} total: {}", parts.join(", "));

    // Wait for worktree discovery
    let worktrees = worktrees_future.await.unwrap_or_default();

    // Compute padding: max name length across all repos and worktree repos
    let mut pad = 0;
    for result in results.lock().unwrap().iter().flatten() {
        if result.name.len() > pad {
            pad = result.name.len();
        }
    }
    for wt in &worktrees {
        if wt.repo.len() > pad {
            pad = wt.repo.len();
        }
    }

    let print_section =
        |header: &str, repos: &[(String, String)]| {
            if repos.is_empty() {
                return;
            }
            println!();
            println!("{header}");
            for (name, branch) in repos {
                println!("   - {name:<pad$}  {branch}");
            }
        };

    print_section("✨ Updated repositories:", &updated);
    print_section("📦 Unchanged repositories:", &up_to_date);
    print_section("⚠️  Skipped repositories (uncommitted changes):", &skipped);
    print_section("🔌 No-upstream repositories (nothing to pull):", &no_upstream);
    print_section("🐢 Throttled repositories (rate-limited):", &throttled);
    print_section("❌ Failed repositories:", &failed);

    if !worktrees.is_empty() {
        println!();
        println!("🌳 Active worktrees:");
        for wt in &worktrees {
            println!("   - {:<pad$}  {}", wt.repo, wt.branch);
        }
    }

    // Flush stdout
    io::stdout().flush()?;

    if profiling {
        let rows: Vec<crate::profile::ProfileRow> = results
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .map(|result| {
                let status = match result.state {
                    "updated" => "updated",
                    "uptodate" => "uptodate",
                    "skipped" => "skipped",
                    "throttled" => "throttled",
                    _ => "failed",
                };
                crate::profile::ProfileRow {
                    name: result.name.clone(),
                    branch: result.branch.clone(),
                    status,
                    elapsed: result.elapsed,
                    last_log_line: result.last_log.clone(),
                }
            })
            .collect();
        let report = crate::profile::format_report(rows);
        match profile_out {
            Some(path) => std::fs::write(path, report)?,
            None => eprint!("{report}"),
        }
    }

    if !failed.is_empty() {
        Ok(1)
    } else {
        Ok(0)
    }
}
