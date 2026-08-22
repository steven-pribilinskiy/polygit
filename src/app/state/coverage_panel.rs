//! State operations for the org-coverage panel (`CoverageState`): open/close, tab + row navigation,
//! the `topic:` / `-topic:` filter, and the transient multi-select used by the clone action. The
//! panel data itself is produced by `crate::coverage::compute` in a worker; this file only drives
//! the interaction.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::app::{AppState, CoverageState};
use crate::coverage::{CoverageRepo, OwnerCoverage};

/// One panel row as the selector engine sees it. The panel and `polygit select` therefore share a
/// single grammar rather than each carrying its own — the old panel filter understood only names
/// and `topic:`, which is a subset of this.
/// `RepoFacts` derives "cloned" from the path, so this relies on `CoverageRepo` keeping its
/// `cloned` flag and `local_path` in step — which `coverage::build_owner_coverage` does on both of
/// its branches.
pub fn facts_for(owner: &str, repo: &CoverageRepo) -> crate::select::RepoFacts {
    crate::select::RepoFacts {
        owner: owner.to_string(),
        name: repo.name.clone(),
        topics: repo.topics.clone(),
        language: repo.language.clone(),
        is_fork: repo.is_fork,
        is_archived: repo.is_archived,
        private: repo.private,
        size_kb: repo.size_kb,
        local_path: repo.local_path.clone(),
    }
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

/// A repo the user ticked for cloning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedRepo {
    pub owner: String,
    pub name: String,
    pub is_fork: bool,
    pub size_kb: u64,
}

impl CoverageState {
    /// The active owner tab, if any.
    pub fn active_owner(&self) -> Option<&OwnerCoverage> {
        self.owners.get(self.active_tab)
    }

    /// Rows visible in the active tab: the fork/archived toggles, then the selector expression, then
    /// (when asked) every repo sharing a project stem with what matched.
    pub fn visible_rows(&self) -> Vec<&CoverageRepo> {
        let Some(owner) = self.active_owner() else {
            return Vec::new();
        };
        let candidates = owner.visible(self.include_forks, self.include_archived);
        let Ok(selector) = crate::select::parse(&self.filter) else {
            // A half-typed expression shows everything rather than blanking the list; the parse
            // error is rendered beside the input.
            return candidates;
        };
        let facts: Vec<crate::select::RepoFacts> =
            candidates.iter().map(|repo| facts_for(&owner.owner, repo)).collect();
        let mut chosen = crate::select::select(&facts, &selector);
        if self.with_siblings {
            chosen = crate::select::expand_siblings(
                &chosen,
                &facts,
                &crate::select::ClusterOpts::default(),
            );
        }
        chosen.into_iter().filter_map(|index| candidates.get(index).copied()).collect()
    }

    /// The parse error for the current expression, when it has one.
    pub fn filter_error(&self) -> Option<String> {
        crate::select::parse(&self.filter).err()
    }

    /// The checked-set key for a repo in the active tab.
    fn row_key(&self, repo: &CoverageRepo) -> String {
        let owner = self.active_owner().map(|owner| owner.owner.as_str()).unwrap_or("");
        format!("{owner}/{}", repo.name)
    }

    /// Missing (not-cloned) repos currently checked in the active tab — the payload the clone
    /// action needs, carrying the owner so the destination and the `gh` slug both resolve.
    pub fn checked_missing(&self) -> Vec<CheckedRepo> {
        let owner = self.active_owner().map(|owner| owner.owner.clone()).unwrap_or_default();
        self.visible_rows()
            .into_iter()
            .filter(|repo| !repo.cloned && self.checked.contains(&self.row_key(repo)))
            .map(|repo| CheckedRepo {
                owner: owner.clone(),
                name: repo.name.clone(),
                is_fork: repo.is_fork,
                size_kb: repo.size_kb,
            })
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
            with_siblings: false,
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

    /// The plan the current selection and layout would produce — the panel's preview, and exactly
    /// what `polygit plan` computes for the same inputs.
    pub fn coverage_plan(&self) -> Option<crate::layout::Plan> {
        let state = self.coverage_modal.as_ref()?;
        let owner = state.active_owner()?;
        let facts: Vec<crate::select::RepoFacts> = state
            .visible_rows()
            .into_iter()
            .map(|repo| facts_for(&owner.owner, repo))
            .collect();
        if facts.is_empty() {
            return None;
        }
        let template =
            crate::layout::LayoutTemplate::parse(&self.coverage_prefs.layout).ok()?;
        let context = crate::layout::LayoutContext::build(
            &facts,
            self.coverage_prefs.prefix_depth.max(1),
            2,
            &crate::select::ClusterOpts::default(),
        );
        let root = self
            .coverage_prefs
            .clone_root
            .clone()
            .or_else(|| state.roots.first().cloned())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let selected: Vec<usize> = (0..facts.len()).collect();
        Some(crate::layout::plan(&facts, &selected, &template, &context, &root))
    }

    /// Where the axis menu hangs: under the filter row it edits, right-aligned to the panel.
    pub fn coverage_axis_anchor(&self) -> (u16, u16) {
        let area = self.coverage_area;
        (area.x + area.width.saturating_sub(2), area.y + 2)
    }

    /// The axis menu's rows: `(label, term)`. Ordered by how much each axis actually discriminates
    /// among the repos in view, with its coverage shown — because a topic filter is a strong
    /// primary axis in a well-tagged org and close to useless in one whose topics are all
    /// machine-generated markers, and no fixed ordering is right for both.
    pub fn coverage_axis_candidates(&self) -> Vec<(char, String, String)> {
        let Some(state) = self.coverage_modal.as_ref() else {
            return Vec::new();
        };
        let Some(owner) = state.active_owner() else {
            return Vec::new();
        };
        let facts: Vec<crate::select::RepoFacts> = owner
            .visible(state.include_forks, state.include_archived)
            .into_iter()
            .map(|repo| facts_for(&owner.owner, repo))
            .collect();
        if facts.is_empty() {
            return Vec::new();
        }
        let depth = self.coverage_prefs.prefix_depth.max(1);
        let families = crate::select::prefix_families(&facts, depth);
        let in_family: usize = families.iter().map(|(_, count)| count).sum();
        let (topics, regime) = crate::select::topic_stats(&facts);
        let languages = crate::select::language_stats(&facts);
        let clusters =
            crate::select::clusters(&facts, &crate::select::ClusterOpts::default());
        let clustered: usize = clusters.iter().map(|entry| entry.members.len()).sum();
        let percent = |part: usize| part * 100 / facts.len();

        // Each row carries the numbers that justify its position, so the ranking is legible rather
        // than magic.
        let mut rows: Vec<(usize, char, String, String)> = vec![
            (
                percent(in_family),
                'p',
                format!(
                    "prefix:   {}% of repos, {} families at depth {depth}",
                    percent(in_family),
                    families.len()
                ),
                families
                    .first()
                    .map(|(key, _)| format!("prefix:{key}"))
                    .unwrap_or_else(|| "prefix:".to_string()),
            ),
            (
                percent(topics.populated),
                't',
                format!(
                    "topic:    {}% tagged, {} distinct ({})",
                    topics.percent(),
                    topics.distinct,
                    match regime {
                        crate::select::Regime::Rich => "rich",
                        crate::select::Regime::Sparse => "sparse",
                        crate::select::Regime::Degenerate => "machine-generated",
                    }
                ),
                "topic:".to_string(),
            ),
            (
                percent(languages.populated),
                'l',
                format!(
                    "lang:     {}% set, {} distinct",
                    languages.percent(),
                    languages.distinct
                ),
                "lang:".to_string(),
            ),
            (
                percent(clustered),
                'j',
                format!("project:  {}% in {} clusters", percent(clustered), clusters.len()),
                "--siblings".to_string(),
            ),
        ];
        rows.sort_by_key(|(coverage, ..)| std::cmp::Reverse(*coverage));
        let mut out: Vec<(char, String, String)> =
            rows.into_iter().map(|(_, key, label, term)| (key, label, term)).collect();
        // State axes are exact rather than ranked, so they sit after the measured ones. Every row
        // carries its own mnemonic: deriving one from the first letter collided four ways here.
        for (key, label, term) in [
            ('m', "is:missing — not cloned locally", "is:missing"),
            ('c', "is:cloned — already here", "is:cloned"),
            ('a', "is:archived", "is:archived"),
            ('f', "is:fork", "is:fork"),
        ] {
            out.push((key, label.to_string(), term.to_string()));
        }
        out
    }

    /// Append a term to the expression, space-separated — adjacent terms are an implicit AND. The
    /// `--siblings` pseudo-term flips the clustering operator instead of joining the expression.
    pub fn coverage_append_term(&mut self, term: &str) {
        if term == "--siblings" {
            self.coverage_toggle_siblings();
            return;
        }
        if let Some(state) = self.coverage_modal.as_mut() {
            if !state.filter.is_empty() && !state.filter.ends_with(' ') {
                state.filter.push(' ');
            }
            state.filter.push_str(term);
            state.selected = 0;
            state.scroll = 0;
            // A key-prefixed term still needs its value, so leave the caret in the input.
            state.filter_focused = term.ends_with(':');
        }
    }

    /// Toggle the clustering operator: with it on, matching one repo of a project pulls in its
    /// infrastructure and deploy repos too.
    pub fn coverage_toggle_siblings(&mut self) {
        if let Some(state) = self.coverage_modal.as_mut() {
            state.with_siblings = !state.with_siblings;
            state.selected = 0;
            state.scroll = 0;
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

    /// `cloned` and `local_path` are one fact in two fields, and `coverage::build_owner_coverage`
    /// always sets them together — a fixture that disagrees would test something impossible.
    fn repo(name: &str, cloned: bool, topics: &[&str]) -> CoverageRepo {
        CoverageRepo {
            name: name.to_string(),
            cloned,
            local_path: cloned.then(|| std::path::PathBuf::from(format!("/local/{name}"))),
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
    fn the_panel_filter_speaks_the_selector_grammar() {
        // The old panel understood names and `topic:` only. Those still work, and everything the
        // CLI grammar adds now works here too — one language, not two.
        let windows = repo("win-tool", false, &["windows"]);
        let linux = repo("linux-tool", true, &["linux"]);
        let facts = [facts_for("acme", &windows), facts_for("acme", &linux)];
        let matched = |query: &str| {
            let selector = crate::select::parse(query).unwrap();
            crate::select::select(&facts, &selector)
        };
        assert_eq!(matched("win"), vec![0]);
        assert_eq!(matched("topic:windows"), vec![0]);
        assert_eq!(matched("-topic:windows"), vec![1]);
        assert_eq!(matched(""), vec![0, 1]);
        // New reach the panel did not have before.
        assert_eq!(matched("is:cloned"), vec![1]);
        assert_eq!(matched("suffix:tool"), vec![0, 1]);
        assert_eq!(matched("win OR is:cloned"), vec![0, 1]);
        assert!(crate::select::parse("bogus:x").is_err());
    }
}
