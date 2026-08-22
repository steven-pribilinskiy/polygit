//! State operations for the org-coverage panel (`CoverageState`): open/close, tab + row navigation,
//! the `topic:` / `-topic:` filter, and the transient multi-select used by the clone action. The
//! panel data itself is produced by `crate::coverage::compute` in a worker; this file only drives
//! the interaction.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::app::{AppState, CoverageState};
use crate::coverage::{CoverageRepo, OwnerCoverage};

/// Whether a repo passes the live filter query. Empty query matches everything. Each whitespace-
/// separated token is either `topic:<t>` (has that topic), `-topic:<t>` (does NOT have it), a plain
/// word (name contains it), or `-word` (name does NOT contain it). All tokens must hold (AND).
fn filter_matches(repo: &CoverageRepo, query: &str) -> bool {
    for raw in query.split_whitespace() {
        let (negate, token) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        if token.is_empty() {
            continue;
        }
        let hit = if let Some(topic) = token.strip_prefix("topic:") {
            let topic = topic.to_lowercase();
            !topic.is_empty() && repo.topics.iter().any(|owned| owned.to_lowercase() == topic)
        } else {
            repo.name.to_lowercase().contains(&token.to_lowercase())
        };
        if hit == negate {
            return false;
        }
    }
    true
}

/// Accept an owner as `owner`, `owner/repo`, or a GitHub URL — all three are things people have in
/// hand when they want an org's repos. Returns None for anything with no owner segment.
pub fn parse_owner_input(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let without_scheme = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("github.com/");
    let owner = without_scheme.split('/').next()?.trim();
    if owner.is_empty() || owner.contains(char::is_whitespace) {
        return None;
    }
    Some(owner.to_string())
}

impl CoverageState {
    /// The active owner tab, if any.
    pub fn active_owner(&self) -> Option<&OwnerCoverage> {
        self.owners.get(self.active_tab)
    }

    /// Rows visible in the active tab: the fork/archived toggles then the filter query.
    pub fn visible_rows(&self) -> Vec<&CoverageRepo> {
        let Some(owner) = self.active_owner() else {
            return Vec::new();
        };
        owner
            .visible(self.include_forks, self.include_archived)
            .into_iter()
            .filter(|repo| filter_matches(repo, &self.filter))
            .collect()
    }

    /// The checked-set key for a repo in the active tab.
    fn row_key(&self, repo: &CoverageRepo) -> String {
        let owner = self.active_owner().map(|owner| owner.owner.as_str()).unwrap_or("");
        format!("{owner}/{}", repo.name)
    }

    /// Missing (not-cloned) repos currently checked in the active tab, as `(url, name, is_fork)` —
    /// the payload the clone action needs.
    pub fn checked_missing(&self) -> Vec<(String, String, bool)> {
        self.visible_rows()
            .into_iter()
            .filter(|repo| !repo.cloned && self.checked.contains(&self.row_key(repo)))
            .map(|repo| (repo.url.clone(), repo.name.clone(), repo.is_fork))
            .collect()
    }
}

impl AppState {
    /// Open the coverage panel in its loading state, scoped to `roots`. The caller kicks off the
    /// async scan (see `worker::run_coverage_scan`).
    pub fn open_coverage(&mut self, roots: Vec<PathBuf>, max_depth: usize) {
        self.coverage_modal = Some(CoverageState {
            owners: Vec::new(),
            loading: true,
            error: None,
            active_tab: 0,
            selected: 0,
            scroll: 0,
            filter: String::new(),
            filter_focused: false,
            include_forks: false,
            include_archived: false,
            checked: HashSet::new(),
            extra_owners: Vec::new(),
            owner_input: None,
            viewport_rows: 0,
            roots,
            max_depth,
            refresh: false,
            cloning: false,
            clone_status: String::new(),
        });
    }

    pub fn close_coverage(&mut self) {
        self.coverage_modal = None;
    }

    /// Re-run the scan against the same roots, bypassing the listing cache.
    pub fn coverage_refresh(&mut self) {
        if let Some(state) = self.coverage_modal.as_mut() {
            state.loading = true;
            state.error = None;
            state.refresh = true;
        }
    }

    /// Switch owner tab, resetting the row cursor.
    pub fn coverage_cycle_tab(&mut self, forward: bool) {
        if let Some(state) = self.coverage_modal.as_mut() {
            let count = state.owners.len();
            if count == 0 {
                return;
            }
            state.active_tab = if forward {
                (state.active_tab + 1) % count
            } else {
                (state.active_tab + count - 1) % count
            };
            state.selected = 0;
            state.scroll = 0;
        }
    }

    /// Move the row cursor within the active tab's visible rows.
    /// Move the row cursor, pulling the window along so the cursor stays visible. The wheel moves
    /// `scroll` on its own; this is the only thing that ties the two together.
    pub fn coverage_move(&mut self, delta: i32) {
        if let Some(state) = self.coverage_modal.as_mut() {
            let count = state.visible_rows().len();
            if count == 0 {
                state.selected = 0;
                state.scroll = 0;
                return;
            }
            let next = (state.selected as i32 + delta).clamp(0, count as i32 - 1);
            state.selected = next as usize;
            let height = state.viewport_rows.max(1);
            if state.selected < state.scroll {
                state.scroll = state.selected;
            } else if state.selected >= state.scroll + height {
                state.scroll = state.selected + 1 - height;
            }
        }
    }

    /// Toggle the checkbox on the cursor row.
    pub fn coverage_toggle_check(&mut self) {
        if let Some(state) = self.coverage_modal.as_mut() {
            let key = {
                let rows = state.visible_rows();
                let Some(repo) = rows.get(state.selected) else {
                    return;
                };
                state.row_key(repo)
            };
            if !state.checked.remove(&key) {
                state.checked.insert(key);
            }
        }
    }

    /// Check or uncheck every visible row in the active tab.
    pub fn coverage_set_all(&mut self, checked: bool) {
        if let Some(state) = self.coverage_modal.as_mut() {
            let keys: Vec<String> =
                state.visible_rows().iter().map(|repo| state.row_key(repo)).collect();
            for key in keys {
                if checked {
                    state.checked.insert(key);
                } else {
                    state.checked.remove(&key);
                }
            }
        }
    }

    pub fn coverage_toggle_forks(&mut self) {
        if let Some(state) = self.coverage_modal.as_mut() {
            state.include_forks = !state.include_forks;
            state.selected = 0;
            state.scroll = 0;
        }
    }

    pub fn coverage_toggle_archived(&mut self) {
        if let Some(state) = self.coverage_modal.as_mut() {
            state.include_archived = !state.include_archived;
            state.selected = 0;
            state.scroll = 0;
        }
    }

    /// Edit the filter query (a typed character), resetting the cursor.
    pub fn coverage_filter_push(&mut self, ch: char) {
        if let Some(state) = self.coverage_modal.as_mut() {
            state.filter.push(ch);
            state.selected = 0;
            state.scroll = 0;
        }
    }

    /// Open the add-owner prompt. An owner named here is enumerated even with nothing cloned from
    /// it, which is what makes an org you have never touched a plannable target.
    pub fn coverage_owner_prompt(&mut self) {
        if let Some(state) = self.coverage_modal.as_mut() {
            state.owner_input = Some(String::new());
            state.filter_focused = false;
        }
    }

    pub fn coverage_owner_push(&mut self, ch: char) {
        if let Some(input) = self.coverage_modal.as_mut().and_then(|state| state.owner_input.as_mut())
        {
            input.push(ch);
        }
    }

    pub fn coverage_owner_pop(&mut self) {
        if let Some(input) = self.coverage_modal.as_mut().and_then(|state| state.owner_input.as_mut())
        {
            input.pop();
        }
    }

    pub fn coverage_owner_cancel(&mut self) {
        if let Some(state) = self.coverage_modal.as_mut() {
            state.owner_input = None;
        }
    }

    /// Accept the typed owner. Returns true when a rescan is needed. Duplicates are a no-op rather
    /// than a second tab for the same owner.
    pub fn coverage_owner_commit(&mut self) -> bool {
        let Some(state) = self.coverage_modal.as_mut() else {
            return false;
        };
        let raw = state.owner_input.take().unwrap_or_default();
        let Some(owner) = parse_owner_input(&raw) else {
            return false;
        };
        if state.extra_owners.iter().any(|existing| existing.eq_ignore_ascii_case(&owner))
            || state.owners.iter().any(|existing| existing.owner.eq_ignore_ascii_case(&owner))
        {
            self.show_toast(format!("Already showing {owner}"));
            return false;
        }
        state.extra_owners.push(owner);
        state.loading = true;
        state.refresh = false;
        true
    }

    pub fn coverage_filter_pop(&mut self) {
        if let Some(state) = self.coverage_modal.as_mut() {
            state.filter.pop();
            state.selected = 0;
            state.scroll = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str, cloned: bool, topics: &[&str]) -> CoverageRepo {
        CoverageRepo {
            name: name.to_string(),
            cloned,
            local_path: None,
            is_fork: false,
            is_archived: false,
            private: false,
            topics: topics.iter().map(|topic| topic.to_string()).collect(),
            size_kb: 0,
            description: None,
            language: None,
            pushed_at: None,
            url: format!("https://github.com/acme/{name}"),
        }
    }

    #[test]
    fn owner_input_accepts_the_forms_people_have_in_hand() {
        assert_eq!(parse_owner_input("acme"), Some("acme".into()));
        assert_eq!(parse_owner_input("  acme  "), Some("acme".into()));
        assert_eq!(parse_owner_input("acme/widget"), Some("acme".into()));
        assert_eq!(parse_owner_input("https://github.com/acme/widget"), Some("acme".into()));
        assert_eq!(parse_owner_input("github.com/acme"), Some("acme".into()));
        assert_eq!(parse_owner_input("acme/"), Some("acme".into()));
        assert_eq!(parse_owner_input(""), None);
        assert_eq!(parse_owner_input("   "), None);
        assert_eq!(parse_owner_input("two words"), None);
    }

    #[test]
    fn filter_plain_and_negation() {
        let windows = repo("win-tool", false, &["windows"]);
        let linux = repo("linux-tool", false, &["linux"]);
        assert!(filter_matches(&windows, "win"));
        assert!(!filter_matches(&linux, "win"));
        // Topic include / exclude.
        assert!(filter_matches(&windows, "topic:windows"));
        assert!(!filter_matches(&linux, "topic:windows"));
        assert!(filter_matches(&linux, "-topic:windows"));
        assert!(!filter_matches(&windows, "-topic:windows"));
        // Empty query matches everything.
        assert!(filter_matches(&windows, ""));
        // AND across tokens.
        assert!(filter_matches(&windows, "win topic:windows"));
        assert!(!filter_matches(&windows, "win topic:linux"));
    }
}
