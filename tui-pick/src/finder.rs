//! The fzf-style fuzzy finder: the matcher used everywhere (inline filter + the overlay) plus the
//! overlay widget itself. The overlay state/render are added alongside the host integration.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Fuzzy-match `query` against `haystack` (case-insensitive, smart normalization). Returns the
/// match `score` (higher = better) and the byte-agnostic **char indices** of the matched chars in
/// `haystack` (for highlighting). An empty query matches everything with score 0 and no indices.
pub fn fuzzy_match(haystack: &str, query: &str) -> Option<(u32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut hbuf = Vec::new();
    let haystack = Utf32Str::new(haystack, &mut hbuf);
    let mut indices: Vec<u32> = Vec::new();
    pattern.indices(haystack, &mut matcher, &mut indices).map(|score| {
        indices.sort_unstable();
        indices.dedup();
        (score, indices.into_iter().map(|index| index as usize).collect())
    })
}

/// Whether `query` fuzzy-matches `haystack` at all (cheap membership test for filtering).
pub fn fuzzy_matches(haystack: &str, query: &str) -> bool {
    fuzzy_match(haystack, query).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_matches_and_reports_indices() {
        // "mfc" matches m..f..c as a subsequence of "microfrontends-calendar".
        let (_score, idx) = fuzzy_match("microfrontends-calendar", "mfc").expect("matches");
        assert!(!idx.is_empty());
        // Non-subsequence does not match.
        assert!(fuzzy_match("alpha", "zzz").is_none());
        // Empty query matches with no highlight.
        assert_eq!(fuzzy_match("anything", ""), Some((0, Vec::new())));
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_matches("PolyGit", "polygit"));
        assert!(fuzzy_matches("polygit", "PG"));
    }
}
