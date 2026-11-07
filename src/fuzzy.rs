//! Fuzzy matching for polygit's inline filters (the repo list, the settings search), over tuilith's
//! `pick` — the same matcher the repo finder and the folder picker use, so one query ranks alike
//! everywhere.

/// Match `query` against `haystack`, case-insensitively. Returns the score (higher is better) and the
/// **character** indices of the matched characters, for highlighting. An empty query matches
/// everything with score 0 and no indices.
pub fn fuzzy_match(haystack: &str, query: &str) -> Option<(i32, Vec<usize>)> {
    let matched = tuilith::pick::score(query, haystack)?;
    let at = matched.at();
    let indices = haystack
        .char_indices()
        .enumerate()
        .filter(|(_, (byte, _))| at.contains(byte))
        .map(|(index, _)| index)
        .collect();
    Some((matched.score(), indices))
}

/// Whether `query` matches `haystack` at all.
pub fn fuzzy_matches(haystack: &str, query: &str) -> bool {
    fuzzy_match(haystack, query).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_are_characters_not_bytes() {
        let (_, indices) = fuzzy_match("café-bar", "fb").expect("a match");
        assert_eq!(indices, vec![2, 5], "the é is one character, not two bytes");
        assert!(fuzzy_match("alpha", "zzz").is_none());
        assert_eq!(fuzzy_match("anything", ""), Some((0, Vec::new())));
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_matches("PolyGit", "polygit"));
        assert!(fuzzy_matches("polygit", "PG"));
    }
}
