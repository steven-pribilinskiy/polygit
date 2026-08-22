//! GitHub "org coverage": for each distinct GitHub owner/org found among the `origin` remotes of
//! the repos under a set of scan roots, report which of that owner's repos are cloned locally and
//! which are missing.
//!
//! Owner identity comes ONLY from the descendant repos' remotes, never from the folder name — so a
//! mis-typed root (`aylthi`, `ayl`) still resolves to the real org. Owners split two ways:
//! - **mirror** (your own account, or an org you're a member of) — enumerated in full via
//!   `gh repo list`; every repo is flagged cloned/missing.
//! - **partial** (a third-party owner where you keep only a slice, e.g. under `~/projects/oss`) —
//!   never enumerated (it could be thousands of repos); we show your local count over the owner's
//!   reported total and a "partial" marker instead of flooding the view with missing rows.
//!
//! Owner listings are cached under `~/.config/polygit/coverage-cache.json` with a TTL; the local
//! diff (which of those repos you actually have) is recomputed every run since it changes as you
//! clone.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::git;

/// How long a fetched owner listing stays fresh (seconds). Orgs change slowly and the TUI/CLI can
/// force a refresh; one hour keeps repeat runs instant without going stale for long.
pub const COVERAGE_TTL_SECS: i64 = 3600;

/// Current wall-clock time in Unix seconds. `0` if the clock is before the epoch (never in practice).
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether this owner is one you mirror in full (your account or a member org) or one you keep only
/// a partial slice of (a third-party owner).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerKind {
    /// Your own GitHub account (owner == login).
    User,
    /// An org you are an active member of.
    MemberOrg,
    /// A third-party owner — you keep only a slice, so it's never enumerated.
    Partial,
}

impl OwnerKind {
    pub fn is_partial(self) -> bool {
        matches!(self, OwnerKind::Partial)
    }
}

/// One repo in an owner's coverage, with its local-clone state resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageRepo {
    pub name: String,
    /// True when a local clone of this repo exists under the scan roots (rename-aware).
    pub cloned: bool,
    /// Absolute path of the local clone, when cloned.
    #[serde(default)]
    pub local_path: Option<PathBuf>,
    #[serde(default)]
    pub is_fork: bool,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub topics: Vec<String>,
    /// GitHub's `diskUsage`, in kilobytes.
    #[serde(default)]
    pub size_kb: u64,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    /// ISO-8601 timestamp of the last push, as `gh` reports it.
    #[serde(default)]
    pub pushed_at: Option<String>,
    /// Browsable / clone URL: `https://github.com/<owner>/<name>`.
    pub url: String,
}

impl CoverageRepo {
    /// A repo matches the fork/archived inclusion toggles when it isn't a hidden fork or a hidden
    /// archived repo. A cloned repo is always shown (you have it) regardless of the toggles.
    fn passes(&self, include_forks: bool, include_archived: bool) -> bool {
        self.cloned
            || ((include_forks || !self.is_fork) && (include_archived || !self.is_archived))
    }
}

/// Coverage for one owner/org.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerCoverage {
    pub owner: String,
    pub kind: OwnerKind,
    /// Mirror owner: the owner's full repo list, each flagged cloned/missing. Partial owner: only
    /// the repos you have locally (we never enumerate a third-party owner).
    pub repos: Vec<CoverageRepo>,
    /// Total repos in scope. Mirror: `repos.len()`. Partial: the owner's reported public-repo count.
    pub total: usize,
}

impl OwnerCoverage {
    /// Repos visible under the given fork/archived toggles.
    pub fn visible(&self, include_forks: bool, include_archived: bool) -> Vec<&CoverageRepo> {
        self.repos
            .iter()
            .filter(|repo| repo.passes(include_forks, include_archived))
            .collect()
    }

    /// Cloned repos among the visible set.
    pub fn cloned_count(&self, include_forks: bool, include_archived: bool) -> usize {
        self.visible(include_forks, include_archived)
            .iter()
            .filter(|repo| repo.cloned)
            .count()
    }

    /// Badge denominator: mirror = visible in-scope count; partial = the owner's reported total
    /// (so the badge honestly reads e.g. `1/1043`).
    pub fn badge_total(&self, include_forks: bool, include_archived: bool) -> usize {
        if self.kind.is_partial() {
            self.total.max(self.repos.len())
        } else {
            self.visible(include_forks, include_archived).len()
        }
    }

    /// Missing (not-cloned) repos among the visible set. Always empty for a partial owner.
    pub fn missing(&self, include_forks: bool, include_archived: bool) -> Vec<&CoverageRepo> {
        if self.kind.is_partial() {
            return Vec::new();
        }
        self.visible(include_forks, include_archived)
            .into_iter()
            .filter(|repo| !repo.cloned)
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Local scan
// ---------------------------------------------------------------------------------------------

/// A local repo discovered under the scan roots: its GitHub `(owner, repo)` plus its path.
struct LocalRepo {
    name: String,
    path: PathBuf,
}

/// Scan the roots and group every local repo with a github.com `origin` by owner. Deduped across
/// overlapping roots. Repos whose remote isn't a github.com URL are skipped (can't be enumerated).
async fn scan_local(roots: &[PathBuf], max_depth: usize) -> BTreeMap<String, Vec<LocalRepo>> {
    let mut by_owner: BTreeMap<String, Vec<LocalRepo>> = BTreeMap::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for root in roots {
        let Ok(paths) = git::discover_repos_recursive(root, max_depth).await else {
            continue;
        };
        for path in paths {
            if !seen.insert(path.clone()) {
                continue;
            }
            let Some(url) = git::get_remote_url(&path).await else {
                continue;
            };
            let Some((owner, name)) = git::parse_owner_repo(&url) else {
                continue;
            };
            by_owner.entry(owner).or_default().push(LocalRepo { name, path });
        }
    }
    by_owner
}

// ---------------------------------------------------------------------------------------------
// GitHub queries (via the already-authenticated `gh` CLI)
// ---------------------------------------------------------------------------------------------

/// GitHub-side data for one repo (no local state), as cached per owner.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrgRepoData {
    name: String,
    #[serde(default)]
    is_fork: bool,
    #[serde(default)]
    is_archived: bool,
    #[serde(default)]
    private: bool,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    size_kb: u64,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    pushed_at: Option<String>,
}

/// Raw `gh repo list --json` row. Topics come back as `[{"name": "..."}]`; visibility is uppercase.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRepo {
    name: String,
    #[serde(default)]
    is_fork: bool,
    #[serde(default)]
    is_archived: bool,
    #[serde(default)]
    visibility: String,
    #[serde(default)]
    repository_topics: Option<Vec<RawTopic>>,
    #[serde(default)]
    disk_usage: u64,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    primary_language: Option<RawLanguage>,
    #[serde(default)]
    pushed_at: Option<String>,
}

#[derive(Deserialize)]
struct RawTopic {
    name: String,
}

#[derive(Deserialize)]
struct RawLanguage {
    name: String,
}

impl From<RawRepo> for OrgRepoData {
    fn from(raw: RawRepo) -> Self {
        OrgRepoData {
            name: raw.name,
            is_fork: raw.is_fork,
            is_archived: raw.is_archived,
            private: raw.visibility.eq_ignore_ascii_case("private"),
            topics: raw
                .repository_topics
                .unwrap_or_default()
                .into_iter()
                .map(|topic| topic.name)
                .collect(),
            size_kb: raw.disk_usage,
            description: raw.description.filter(|text| !text.is_empty()),
            language: raw.primary_language.map(|language| language.name),
            pushed_at: raw.pushed_at,
        }
    }
}

/// One owner you can enumerate: your own account, or an org you belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRef {
    pub login: String,
    pub kind: OwnerKind,
}

/// Your account plus every org you belong to.
///
/// Deliberately the REST endpoint: `gh api user/orgs` returns memberships that the GraphQL
/// `viewer.organizations` connection omits (measured: 4 versus 3 on the same token), and an org
/// missing from the picker is an org you cannot select.
pub async fn list_my_owners() -> Vec<OwnerRef> {
    let mut out = Vec::new();
    if let Some(login) = current_login().await {
        out.push(OwnerRef { login, kind: OwnerKind::User });
    }
    let output = Command::new("gh").args(["api", "user/orgs", "--paginate", "--jq", ".[].login"]).output().await;
    if let Ok(output) = output
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let login = line.trim();
            if !login.is_empty() {
                out.push(OwnerRef { login: login.to_string(), kind: OwnerKind::MemberOrg });
            }
        }
    }
    out
}

/// One org inside an enterprise, with the repo counts GitHub reports for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterpriseOrg {
    pub login: String,
    pub name: String,
    pub total: usize,
    pub archived: usize,
}

/// An enterprise and the orgs it contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enterprise {
    pub slug: String,
    pub name: String,
    pub orgs: Vec<EnterpriseOrg>,
}

/// Why enterprise discovery produced nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnterpriseError {
    /// The token lacks `read:enterprise`. Carries the exact command that fixes it, because an API
    /// error here reads as "no enterprises" and sends people looking in the wrong place.
    ScopeMissing { remedy: String },
    Other(String),
}

impl EnterpriseError {
    pub fn message(&self) -> String {
        match self {
            EnterpriseError::ScopeMissing { remedy } => {
                format!("needs the read:enterprise scope — run: {remedy}")
            }
            EnterpriseError::Other(detail) => detail.clone(),
        }
    }
}

/// The command that grants the scope enterprise discovery needs.
pub const ENTERPRISE_SCOPE_REMEDY: &str = "gh auth refresh -h github.com -s read:enterprise";

const ENTERPRISE_QUERY: &str = r#"
query($ecursor: String) {
  viewer {
    enterprises(first: 25, after: $ecursor) {
      pageInfo { hasNextPage endCursor }
      nodes {
        slug
        name
        organizations(first: 100) {
          nodes {
            login
            name
            repositories(first: 1) { totalCount }
            repositoriesArchived: repositories(isArchived: true, first: 1) { totalCount }
          }
        }
      }
    }
  }
}
"#;

/// Every enterprise you belong to and the orgs inside it, following the enterprise cursor.
pub async fn list_enterprises() -> Result<Vec<Enterprise>, EnterpriseError> {
    let mut out: Vec<Enterprise> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut args: Vec<String> = vec![
            "api".into(),
            "graphql".into(),
            "-f".into(),
            format!("query={ENTERPRISE_QUERY}"),
        ];
        if let Some(after) = &cursor {
            args.push("-f".into());
            args.push(format!("ecursor={after}"));
        }
        let output = Command::new("gh")
            .args(&args)
            .output()
            .await
            .map_err(|err| EnterpriseError::Other(format!("running gh api graphql: {err}")))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stdout.contains("INSUFFICIENT_SCOPES") || stderr.contains("INSUFFICIENT_SCOPES") {
            return Err(EnterpriseError::ScopeMissing {
                remedy: ENTERPRISE_SCOPE_REMEDY.to_string(),
            });
        }
        if !output.status.success() {
            return Err(EnterpriseError::Other(stderr.trim().to_string()));
        }
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|err| EnterpriseError::Other(format!("parsing graphql reply: {err}")))?;
        let connection = &parsed["data"]["viewer"]["enterprises"];
        for node in connection["nodes"].as_array().unwrap_or(&Vec::new()) {
            out.push(Enterprise {
                slug: node["slug"].as_str().unwrap_or_default().to_string(),
                name: node["name"].as_str().unwrap_or_default().to_string(),
                orgs: node["organizations"]["nodes"]
                    .as_array()
                    .unwrap_or(&Vec::new())
                    .iter()
                    .map(|org| EnterpriseOrg {
                        login: org["login"].as_str().unwrap_or_default().to_string(),
                        name: org["name"].as_str().unwrap_or_default().to_string(),
                        total: org["repositories"]["totalCount"].as_u64().unwrap_or(0) as usize,
                        archived: org["repositoriesArchived"]["totalCount"].as_u64().unwrap_or(0)
                            as usize,
                    })
                    .collect(),
            });
        }
        if connection["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false) {
            cursor = connection["pageInfo"]["endCursor"].as_str().map(String::from);
            if cursor.is_none() {
                break;
            }
        } else {
            break;
        }
    }
    Ok(out)
}

/// Flatten resolved coverage into the records the selector engine matches against.
pub fn repo_facts(owners: &[OwnerCoverage]) -> Vec<crate::select::RepoFacts> {
    owners
        .iter()
        .flat_map(|owner| {
            owner.repos.iter().map(|repo| crate::select::RepoFacts {
                owner: owner.owner.clone(),
                name: repo.name.clone(),
                topics: repo.topics.clone(),
                language: repo.language.clone(),
                is_fork: repo.is_fork,
                is_archived: repo.is_archived,
                private: repo.private,
                size_kb: repo.size_kb,
                local_path: repo.local_path.clone(),
            })
        })
        .collect()
}

/// `gh api user --jq .login` → the authenticated user's login.
async fn current_login() -> Option<String> {
    let output = Command::new("gh").args(["api", "user", "--jq", ".login"]).output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!login.is_empty()).then_some(login)
}

/// Classify an owner: your account, an org you're a member of, or a third-party (partial) owner.
async fn classify_owner(owner: &str, login: &str) -> OwnerKind {
    if owner.eq_ignore_ascii_case(login) {
        return OwnerKind::User;
    }
    let active = Command::new("gh")
        .args(["api", &format!("user/memberships/orgs/{owner}"), "--jq", ".state"])
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "active")
        .unwrap_or(false);
    if active {
        OwnerKind::MemberOrg
    } else {
        OwnerKind::Partial
    }
}

/// Full repo listing for a mirror owner via `gh repo list` (includes forks & archived; we filter at
/// display time).
async fn fetch_owner_repos(owner: &str) -> Result<Vec<OrgRepoData>> {
    let output = Command::new("gh")
        .args([
            "repo",
            "list",
            owner,
            "--limit",
            "10000",
            "--json",
            "name,isFork,isArchived,visibility,repositoryTopics,diskUsage,description,primaryLanguage,pushedAt",
        ])
        .output()
        .await
        .with_context(|| format!("running gh repo list {owner}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "gh repo list {owner} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let raw: Vec<RawRepo> =
        serde_json::from_slice(&output.stdout).context("parsing gh repo list JSON")?;
    Ok(raw.into_iter().map(OrgRepoData::from).collect())
}

/// The owner's reported public-repo count — used only to give a partial owner an honest denominator
/// without enumerating it. Works for both users and orgs (orgs are users in the REST API).
async fn owner_public_repos(owner: &str) -> Option<usize> {
    let output = Command::new("gh")
        .args(["api", &format!("users/{owner}"), "--jq", ".public_repos"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Resolve a possibly-renamed local repo name to its canonical current name via GitHub's redirect
/// (`gh api repos/<owner>/<name> --jq .name`). Only called for a local name absent from the owner
/// listing, so it catches renames (e.g. `infra-hub` → `aylith-infra`) without a call per repo.
async fn resolve_canonical(owner: &str, name: &str) -> Option<String> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{owner}/{name}"), "--jq", ".name"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let canonical = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!canonical.is_empty()).then_some(canonical)
}

// ---------------------------------------------------------------------------------------------
// Cache (owner listings only; the local diff is always recomputed)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnerEntry {
    kind: OwnerKind,
    /// Mirror: the full listing. Partial: empty (never enumerated).
    repos: Vec<OrgRepoData>,
    /// Partial: the owner's reported public-repo total. Mirror: `repos.len()`.
    total: usize,
    checked_at: i64,
}

/// `owner` → cached listing. Renames are cached separately so a canonical lookup is done once.
#[derive(Debug, Default, Serialize, Deserialize)]
struct CoverageCache {
    #[serde(default)]
    owners: HashMap<String, OwnerEntry>,
    /// `"owner/oldname"` → canonical name.
    #[serde(default)]
    renames: HashMap<String, String>,
}

fn cache_path() -> Option<PathBuf> {
    Some(crate::persist::config_dir()?.join("coverage-cache.json"))
}

fn load_cache() -> CoverageCache {
    cache_path()
        .and_then(|path| std::fs::read_to_string(&path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

#[cfg_attr(test, allow(dead_code))]
fn save_cache(cache: &CoverageCache) {
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

fn is_fresh(checked_at: i64, now: i64) -> bool {
    now - checked_at < COVERAGE_TTL_SECS
}

// ---------------------------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------------------------

/// Compute coverage for every github owner found under `roots`, plus every owner in `extra_owners`
/// (which may have no local clones at all). When `only_owner` is set, restrict to that owner.
/// `force_refresh` bypasses the listing cache. Network access is via `gh`.
pub async fn compute(
    roots: &[PathBuf],
    max_depth: usize,
    only_owner: Option<&str>,
    force_refresh: bool,
    extra_owners: &[String],
) -> Result<Vec<OwnerCoverage>> {
    let login = current_login().await.unwrap_or_default();
    let mut by_owner = scan_local(roots, max_depth).await;
    // An explicitly named owner is enumerated even with nothing cloned from it — that is what makes
    // a cold org (every repo missing) a plannable target rather than an empty tab.
    for owner in extra_owners {
        by_owner.entry(owner.clone()).or_default();
    }
    let mut cache = load_cache();
    let now = now_unix();
    let mut out: Vec<OwnerCoverage> = Vec::new();

    for (owner, locals) in &by_owner {
        if let Some(only) = only_owner {
            if !owner.eq_ignore_ascii_case(only) {
                continue;
            }
        }

        // Owner listing: reuse a fresh cache entry, else fetch and store.
        let entry = match cache.owners.get(owner) {
            Some(entry) if !force_refresh && is_fresh(entry.checked_at, now) => entry.clone(),
            _ => {
                let named = extra_owners.iter().any(|wanted| wanted.eq_ignore_ascii_case(owner));
                let kind = match classify_owner(owner, &login).await {
                    OwnerKind::Partial if named => OwnerKind::MemberOrg,
                    other => other,
                };
                let (repos, total) = if kind.is_partial() {
                    (Vec::new(), owner_public_repos(owner).await.unwrap_or(locals.len()))
                } else {
                    let repos = fetch_owner_repos(owner).await.unwrap_or_default();
                    let total = repos.len();
                    (repos, total)
                };
                let entry = OwnerEntry { kind, repos, total, checked_at: now };
                cache.owners.insert(owner.clone(), entry.clone());
                entry
            }
        };

        out.push(build_owner_coverage(owner, &entry, locals, &mut cache.renames).await);
    }

    save_cache(&cache);

    // Mirror owners first (they carry actionable "missing" rows), each with the most-missing first;
    // partial owners after, alphabetically.
    out.sort_by(|left, right| {
        let partial = left.kind.is_partial().cmp(&right.kind.is_partial());
        let missing = right.missing(true, true).len().cmp(&left.missing(true, true).len());
        partial.then(missing).then_with(|| left.owner.cmp(&right.owner))
    });
    Ok(out)
}

/// Diff an owner's cached listing against the local clones to produce its coverage.
async fn build_owner_coverage(
    owner: &str,
    entry: &OwnerEntry,
    locals: &[LocalRepo],
    renames: &mut HashMap<String, String>,
) -> OwnerCoverage {
    if entry.kind.is_partial() {
        // Never enumerated: the visible repos are exactly your local slice.
        let repos = locals
            .iter()
            .map(|local| CoverageRepo {
                url: format!("https://github.com/{owner}/{}", local.name),
                name: local.name.clone(),
                cloned: true,
                local_path: Some(local.path.clone()),
                is_fork: false,
                is_archived: false,
                private: false,
                topics: Vec::new(),
                size_kb: 0,
                description: None,
                language: None,
                pushed_at: None,
            })
            .collect();
        return OwnerCoverage { owner: owner.to_string(), kind: entry.kind, repos, total: entry.total };
    }

    let org_names: HashSet<&str> = entry.repos.iter().map(|repo| repo.name.as_str()).collect();

    // Map each local clone to a canonical org name → path. A local name already in the listing is
    // canonical; otherwise resolve through the rename redirect (cached) once.
    let mut cloned_path: HashMap<String, PathBuf> = HashMap::new();
    for local in locals {
        let canonical = if org_names.contains(local.name.as_str()) {
            local.name.clone()
        } else {
            let key = format!("{owner}/{}", local.name);
            match renames.get(&key) {
                Some(canonical) => canonical.clone(),
                None => {
                    let canonical = resolve_canonical(owner, &local.name)
                        .await
                        .unwrap_or_else(|| local.name.clone());
                    renames.insert(key, canonical.clone());
                    canonical
                }
            }
        };
        cloned_path.entry(canonical).or_insert_with(|| local.path.clone());
    }

    let repos = entry
        .repos
        .iter()
        .map(|repo| {
            let local_path = cloned_path.get(&repo.name).cloned();
            CoverageRepo {
                cloned: local_path.is_some(),
                local_path,
                url: format!("https://github.com/{owner}/{}", repo.name),
                name: repo.name.clone(),
                is_fork: repo.is_fork,
                is_archived: repo.is_archived,
                private: repo.private,
                topics: repo.topics.clone(),
                size_kb: repo.size_kb,
                description: repo.description.clone(),
                language: repo.language.clone(),
                pushed_at: repo.pushed_at.clone(),
            }
        })
        .collect();

    OwnerCoverage {
        owner: owner.to_string(),
        kind: entry.kind,
        repos,
        total: entry.total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str, cloned: bool, is_fork: bool, is_archived: bool) -> CoverageRepo {
        CoverageRepo {
            name: name.to_string(),
            cloned,
            local_path: None,
            is_fork,
            is_archived,
            private: false,
            topics: Vec::new(),
            size_kb: 0,
            description: None,
            language: None,
            pushed_at: None,
            url: format!("https://github.com/acme/{name}"),
        }
    }

    #[test]
    fn mirror_counts_and_missing_respect_toggles() {
        let cov = OwnerCoverage {
            owner: "acme".into(),
            kind: OwnerKind::MemberOrg,
            total: 4,
            repos: vec![
                repo("a", true, false, false),
                repo("b", false, false, false),
                repo("fork", false, true, false),
                repo("old", false, false, true),
            ],
        };
        // Default: hide forks + archived.
        assert_eq!(cov.badge_total(false, false), 2);
        assert_eq!(cov.cloned_count(false, false), 1);
        let missing: Vec<_> = cov.missing(false, false).iter().map(|r| r.name.clone()).collect();
        assert_eq!(missing, vec!["b"]);
        // Include everything.
        assert_eq!(cov.badge_total(true, true), 4);
        let missing_all: Vec<_> = cov.missing(true, true).iter().map(|r| r.name.clone()).collect();
        assert_eq!(missing_all, vec!["b", "fork", "old"]);
    }

    #[test]
    fn partial_owner_has_no_missing_and_reports_owner_total() {
        let cov = OwnerCoverage {
            owner: "bigcorp".into(),
            kind: OwnerKind::Partial,
            total: 1043,
            repos: vec![repo("one-i-cloned", true, false, false)],
        };
        assert_eq!(cov.badge_total(false, false), 1043);
        assert_eq!(cov.cloned_count(false, false), 1);
        assert!(cov.missing(false, false).is_empty());
    }

    #[test]
    fn parse_owner_repo_matches_and_rejects() {
        assert_eq!(
            git::parse_owner_repo("https://github.com/aylith-labs/aylith-infra"),
            Some(("aylith-labs".to_string(), "aylith-infra".to_string()))
        );
        assert_eq!(
            git::parse_owner_repo("https://github.com/steven-pribilinskiy/notes"),
            Some(("steven-pribilinskiy".to_string(), "notes".to_string()))
        );
        // Non-github host → None.
        assert_eq!(git::parse_owner_repo("https://gitlab.com/foo/bar"), None);
        // Missing repo segment → None.
        assert_eq!(git::parse_owner_repo("https://github.com/only-owner"), None);
    }
}
