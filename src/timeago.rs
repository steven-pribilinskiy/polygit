//! A small dependency-free "time ago" helper: parse the ISO-8601 timestamps `gh` emits
//! (`2026-06-23T14:32:10Z`, always UTC), render a compact relative label ("6 days ago"), and a
//! readable absolute label for the hover tooltip. Pure functions — `now` is always passed in so the
//! buckets are unit-testable without touching the wall clock.

/// Parse a `YYYY-MM-DDTHH:MM:SS` ISO-8601 instant (with an optional fractional part and a trailing
/// `Z`/offset, which we ignore — `gh` returns UTC) into Unix epoch seconds. `None` on any shape we
/// don't recognize.
pub fn parse_iso8601(stamp: &str) -> Option<i64> {
    let stamp = stamp.trim();
    let (date, rest) = stamp.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    // Time runs until the first zone/fraction marker (`Z`, `+`, or `.`); `gh` always emits UTC `Z`.
    let time = rest.split(['Z', '+', '.']).next().unwrap_or(rest);
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    let second: i64 = time_parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since the Unix epoch for a civil (proleptic Gregorian) date — Howard Hinnant's algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400; // [0, 399]
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1; // [0, 365]
    let doe = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// A compact relative label ("just now", "5 minutes ago", "6 days ago", "in 2 hours" for the
/// future). Singular/plural aware. `then` and `now` are Unix epoch seconds.
pub fn relative(now: i64, then: i64) -> String {
    let delta = now - then;
    let future = delta < 0;
    let secs = delta.unsigned_abs();
    if secs < 45 {
        return "just now".to_string();
    }
    let (value, unit) = if secs < 90 {
        (1, "minute")
    } else if secs < 3_600 {
        (secs / 60, "minute")
    } else if secs < 86_400 {
        (secs / 3_600, "hour")
    } else if secs < 2_592_000 {
        (secs / 86_400, "day")
    } else if secs < 31_536_000 {
        (secs / 2_592_000, "month")
    } else {
        (secs / 31_536_000, "year")
    };
    let plural = if value == 1 { "" } else { "s" };
    if future { format!("in {value} {unit}{plural}") } else { format!("{value} {unit}{plural} ago") }
}

/// A readable absolute label for the tooltip: `2026-06-23 14:32 UTC` from the raw ISO string. Falls
/// back to the trimmed input when it isn't the expected shape.
pub fn absolute_label(stamp: &str) -> String {
    let stamp = stamp.trim();
    let Some((date, rest)) = stamp.split_once('T') else {
        return stamp.to_string();
    };
    let time = rest.split(['Z', '+', '.']).next().unwrap_or(rest);
    // Keep HH:MM (drop seconds for a calmer label).
    let hhmm: String = time.split(':').take(2).collect::<Vec<_>>().join(":");
    if hhmm.is_empty() { date.to_string() } else { format!("{date} {hhmm} UTC") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_to_epoch() {
        // 2026-06-23T14:32:10Z → known epoch.
        let secs = parse_iso8601("2026-06-23T14:32:10Z").expect("parse");
        assert_eq!(secs, 1_782_225_130);
        // 1970-01-01T00:00:00Z is the epoch.
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), Some(0));
        // Fractional seconds + offset are tolerated.
        assert_eq!(parse_iso8601("1970-01-01T00:00:01.500+00:00"), Some(1));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_iso8601(""), None);
        assert_eq!(parse_iso8601("2026-06-23"), None);
        assert_eq!(parse_iso8601("not-a-date"), None);
    }

    #[test]
    fn relative_buckets() {
        let now = 1_000_000_000;
        assert_eq!(relative(now, now), "just now");
        assert_eq!(relative(now, now - 10), "just now");
        assert_eq!(relative(now, now - 60), "1 minute ago");
        assert_eq!(relative(now, now - 300), "5 minutes ago");
        assert_eq!(relative(now, now - 3_600), "1 hour ago");
        assert_eq!(relative(now, now - 7_200), "2 hours ago");
        assert_eq!(relative(now, now - 86_400), "1 day ago");
        assert_eq!(relative(now, now - 6 * 86_400), "6 days ago");
        assert_eq!(relative(now, now - 40 * 86_400), "1 month ago");
        assert_eq!(relative(now, now - 400 * 86_400), "1 year ago");
    }

    #[test]
    fn relative_future() {
        let now = 1_000_000_000;
        assert_eq!(relative(now, now + 7_200), "in 2 hours");
    }

    #[test]
    fn absolute_is_readable() {
        assert_eq!(absolute_label("2026-06-23T14:32:10Z"), "2026-06-23 14:32 UTC");
        assert_eq!(absolute_label("2026-06-23T14:32:10+02:00"), "2026-06-23 14:32 UTC");
        assert_eq!(absolute_label("garbage"), "garbage");
    }
}
