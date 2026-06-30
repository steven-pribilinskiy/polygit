//! Self-update: list published versions and install a chosen one over the running binary. The
//! build-info "pin version" picker uses this to download a release's prebuilt binary from GitHub
//! and atomically replace the executable; the new-build watcher / auto-reload then takes the
//! process into it. Network + install only — the UI lives in `render`/`main`.

use std::cmp::Ordering;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use tokio::process::Command;

use crate::changelog::version_cmp;

/// The GitHub repo releases are published to (matches `docs/public/install.sh`).
pub const REPO: &str = "steven-pribilinskiy/polygit";

/// The floor the version picker offers by default. Two reasons gate it: (1) these builds ship the
/// in-app switcher (so you can never strand yourself on a build with no picker), and (2) since
/// v3.0.0 they share the NESTED `state.json` schema — a pre-v3 (flat-schema) build can't read a
/// nested file and would reset your settings to defaults. Pre-v3 builds are still reachable behind
/// the picker's `a` "show older" toggle + the below-floor warning. Bump when the on-disk contract
/// changes again.
pub const VERSION_SELECT_MIN: &str = "3.0.0";

/// Whether `version` ships the in-app picker (at or above the floor).
pub fn supports_in_app_switch(version: &str) -> bool {
    version_cmp(version, VERSION_SELECT_MIN) != Ordering::Less
}

/// The copyable shell command shown in the below-floor warning confirm — the only way back to the
/// latest build once you've pinned a version that predates the picker. `exe_dir` is the install
/// dir of the running binary, so the script overwrites the same file polygit runs from.
pub fn return_to_latest_cmd(exe_dir: &str) -> String {
    format!(
        "curl -fsSL https://steven-pribilinskiy.github.io/polygit/install.sh | POLYGIT_INSTALL={exe_dir} bash"
    )
}

/// The release-asset target triple for the running platform, or `None` when self-install isn't
/// supported there (the picker disables itself with a reason). Native Windows is excluded on
/// purpose: a running `.exe` can't be replaced by a plain rename, and `install.sh` already steers
/// Windows users to WSL (the linux target). Mirrors `install.sh`'s OS/arch mapping otherwise.
pub fn current_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

/// Strip the `" (deleted)"` suffix `current_exe()` reports after a rename-over install, so we
/// operate on the real file now living at the original path.
fn live_path(exe_path: &str) -> &str {
    exe_path.strip_suffix(" (deleted)").unwrap_or(exe_path)
}

/// All published releases, newest-first, as `(version_without_v, iso_date)`. Tries the `gh` CLI
/// first (authenticated, no rate limit), then the public GitHub API. Errors only if both fail.
pub async fn fetch_releases() -> Result<Vec<(String, String)>> {
    match fetch_releases_gh().await {
        Ok(list) if !list.is_empty() => Ok(list),
        _ => fetch_releases_api().await.context("couldn't fetch releases (gh and GitHub API both failed)"),
    }
}

async fn fetch_releases_gh() -> Result<Vec<(String, String)>> {
    let output = Command::new("gh")
        .args(["release", "list", "--repo", REPO, "--json", "tagName,publishedAt", "--limit", "100"])
        .output()
        .await
        .context("gh not available")?;
    if !output.status.success() {
        return Err(anyhow!("gh release list failed"));
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(sorted(parse_release_json(&parsed)))
}

async fn fetch_releases_api() -> Result<Vec<(String, String)>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=100");
    let client = reqwest::Client::builder().user_agent("polygit").build()?;
    let text = client.get(url).send().await?.error_for_status()?.text().await?;
    let parsed: serde_json::Value = serde_json::from_str(&text)?;
    Ok(sorted(parse_release_json(&parsed)))
}

/// Parse a JSON array of releases — `gh`'s `tagName`/`publishedAt` *or* the API's
/// `tag_name`/`published_at` — into `(version, date)`. Skips entries with no tag, strips a leading
/// `v`, and truncates the timestamp to `YYYY-MM-DD`.
fn parse_release_json(value: &serde_json::Value) -> Vec<(String, String)> {
    let Some(array) = value.as_array() else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|entry| {
            let tag = entry.get("tagName").or_else(|| entry.get("tag_name"))?.as_str()?;
            let version = tag.trim_start_matches('v').to_string();
            let date = entry
                .get("publishedAt")
                .or_else(|| entry.get("published_at"))
                .and_then(|date| date.as_str())
                .map(|date| date.split('T').next().unwrap_or(date).to_string())
                .unwrap_or_default();
            Some((version, date))
        })
        .collect()
}

/// Sort releases newest-version-first (defensive — both sources already return newest-first).
fn sorted(mut list: Vec<(String, String)>) -> Vec<(String, String)> {
    list.sort_by(|a, b| version_cmp(&b.0, &a.0));
    list
}

/// Download the release binary for `version`+`target` and atomically install it over the running
/// executable. Mirrors the Makefile's write-then-rename so the new-build watcher / auto-reload
/// picks it up. Errors clearly when the asset is missing or the install dir isn't writable.
pub async fn download_and_install(version: &str, target: &str, exe_path: &str) -> Result<()> {
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/polygit-{target}");
    let client = reqwest::Client::builder().user_agent("polygit").build()?;
    let bytes = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("download failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("no prebuilt binary for {target} in v{version}"))?
        .bytes()
        .await
        .context("download incomplete")?;
    if bytes.len() < 1024 {
        return Err(anyhow!("downloaded file is implausibly small ({} bytes)", bytes.len()));
    }
    let exe = Path::new(live_path(exe_path)).to_path_buf();
    let dir = exe.parent().ok_or_else(|| anyhow!("can't resolve install dir"))?.to_path_buf();
    let tmp = dir.join("polygit.new");
    // std fs on a blocking thread, then an atomic rename over the running binary (same dir).
    tokio::task::spawn_blocking(move || -> Result<()> {
        std::fs::write(&tmp, &bytes).with_context(|| format!("can't write {}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&tmp, &exe).with_context(|| format!("can't install to {}", exe.display()))?;
        Ok(())
    })
    .await
    .context("install task panicked")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_known_on_unix_dev_platforms() {
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            assert!(current_target().is_some(), "self-install target maps on linux/macos");
        }
    }

    #[test]
    fn floor_gates_versions() {
        assert!(supports_in_app_switch(VERSION_SELECT_MIN));
        assert!(supports_in_app_switch("3.0.0"));
        assert!(supports_in_app_switch("3.1.0"));
        // Below the v3.0.0 nested-schema floor — gated behind the picker's "show older" toggle.
        assert!(!supports_in_app_switch("2.109.0"));
        assert!(!supports_in_app_switch("2.71.1"));
        assert!(!supports_in_app_switch("2.5.2"));
    }

    #[test]
    fn parses_both_json_shapes_and_sorts() {
        let gh = serde_json::json!([
            {"tagName": "v2.5.2", "publishedAt": "2026-06-15T01:00:00Z"},
            {"tagName": "v2.72.0", "publishedAt": "2026-06-26T10:00:00Z"},
        ]);
        assert_eq!(
            sorted(parse_release_json(&gh)),
            vec![
                ("2.72.0".to_string(), "2026-06-26".to_string()),
                ("2.5.2".to_string(), "2026-06-15".to_string()),
            ]
        );
        let api = serde_json::json!([{"tag_name": "v2.71.1", "published_at": "2026-06-26T00:00:00Z"}]);
        assert_eq!(parse_release_json(&api), vec![("2.71.1".to_string(), "2026-06-26".to_string())]);
    }

    #[test]
    fn live_path_strips_deleted_suffix() {
        assert_eq!(live_path("/home/u/bin/polygit (deleted)"), "/home/u/bin/polygit");
        assert_eq!(live_path("/home/u/bin/polygit"), "/home/u/bin/polygit");
    }

    #[test]
    fn return_to_latest_targets_exe_dir() {
        let cmd = return_to_latest_cmd("/home/u/bin");
        assert!(cmd.contains("POLYGIT_INSTALL=/home/u/bin"));
        assert!(cmd.contains("install.sh"));
    }
}
