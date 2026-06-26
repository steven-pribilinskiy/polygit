//! The embedded `CHANGELOG.md`, parsed into release entries for the in-app changelog / What's New
//! modal. The file is baked in at compile time, so the installed binary (off `$PATH`, away from the
//! repo) carries its own notes. Each entry is `## vX.Y.Z — YYYY-MM-DD` followed by free-text notes.

/// The raw changelog markdown, embedded at build time.
pub const CHANGELOG_MD: &str = include_str!("../CHANGELOG.md");

/// One parsed release: version, ISO date, and its note lines (borrowed from the embedded markdown).
#[derive(Debug, Clone)]
pub struct Release {
    pub version: &'static str,
    pub date: &'static str,
    pub notes: Vec<&'static str>,
}

/// Parse the embedded changelog into releases, newest first (the file's own order).
pub fn releases() -> Vec<Release> {
    let mut out: Vec<Release> = Vec::new();
    for line in CHANGELOG_MD.lines() {
        if let Some(rest) = line.strip_prefix("## v") {
            // `2.60.0 — 2026-06-20`  (em-dash or hyphen separator).
            let (version, date) = rest
                .split_once('—')
                .or_else(|| rest.split_once(" - "))
                .map(|(version, date)| (version.trim(), date.trim()))
                .unwrap_or((rest.trim(), ""));
            out.push(Release { version, date, notes: Vec::new() });
        } else if let Some(current) = out.last_mut() {
            let trimmed = line.trim_end();
            // Skip blank lines that merely separate releases; keep intra-note blanks rarely matter.
            if !trimmed.is_empty() {
                current.notes.push(trimmed);
            }
        }
    }
    out
}

/// Compare two `x.y.z` version strings (missing/garbage components sort as 0).
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |version: &str| -> (u32, u32, u32) {
        let mut parts = version.trim_start_matches('v').split('.').map(|n| n.parse().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    };
    parse(a).cmp(&parse(b))
}

/// Unix seconds for midnight UTC of an ISO `YYYY-MM-DD` date (for "time ago"). `None` if unparsable.
pub fn date_to_unix(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.trim().parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400)
}

/// "X ago" for a release date, computed against the wall clock (display-only). Falls back to the
/// raw date string when it can't be parsed.
pub fn released_ago(date: &str) -> String {
    let Some(unix) = date_to_unix(date) else {
        return date.to_string();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(unix);
    let secs = (now - unix).max(0) as u64;
    // Guard absurd ages from a bogus upstream publish date (e.g. a release stamped near epoch):
    // fall back to the raw date rather than render "739792d ago".
    const HUNDRED_YEARS: u64 = 100 * 365 * 86_400;
    if secs > HUNDRED_YEARS {
        date.to_string()
    } else if secs < 86_400 {
        "today".to_string()
    } else {
        crate::app::format_ago(secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_embedded_changelog() {
        let rel = releases();
        assert!(!rel.is_empty(), "changelog has releases");
        // Newest first; every entry has a dotted version.
        assert!(rel[0].version.contains('.'));
        // Sorted descending by version.
        for pair in rel.windows(2) {
            assert!(version_cmp(pair[0].version, pair[1].version) != std::cmp::Ordering::Less);
        }
    }

    #[test]
    fn date_to_unix_known_epoch() {
        assert_eq!(date_to_unix("1970-01-01"), Some(0));
        assert_eq!(date_to_unix("2000-01-01"), Some(946_684_800));
        assert_eq!(date_to_unix("garbage"), None);
    }

    #[test]
    fn version_cmp_orders_numerically() {
        use std::cmp::Ordering;
        assert_eq!(version_cmp("2.10.0", "2.9.0"), Ordering::Greater);
        assert_eq!(version_cmp("2.60.0", "2.60.0"), Ordering::Equal);
    }
}
