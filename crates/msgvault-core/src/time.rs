//! msgvault stores timestamps as text in several layouts: go-sqlite3's
//! `time.Time` encoding, SQLite's `CURRENT_TIMESTAMP`, and RFC 3339 from APIs.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

const OFFSET_LAYOUTS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S%.f%:z",
    "%Y-%m-%dT%H:%M:%S%.f%:z",
    "%Y-%m-%d %H:%M:%S%.f %z",
    "%Y-%m-%d %H:%M:%S%.f%z",
];

const NAIVE_LAYOUTS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%d %H:%M",
];

/// Parses any timestamp layout msgvault writes. Naive values are UTC.
pub fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.with_timezone(&Utc));
    }
    // Go's default `String()` appends a zone abbreviation: "... +0000 UTC".
    let trimmed = strip_zone_abbreviation(raw);
    for layout in OFFSET_LAYOUTS {
        if let Ok(parsed) = DateTime::parse_from_str(trimmed, layout) {
            return Some(parsed.with_timezone(&Utc));
        }
    }
    let naive = trimmed.trim_end_matches('Z');
    for layout in NAIVE_LAYOUTS {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(naive, layout) {
            return Some(Utc.from_utc_datetime(&parsed));
        }
    }
    NaiveDate::parse_from_str(naive, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| Utc.from_utc_datetime(&date))
}

/// Seconds or milliseconds since the epoch, as some importers store them.
pub fn from_epoch(value: i64) -> Option<DateTime<Utc>> {
    if value.abs() > 100_000_000_000 {
        Utc.timestamp_millis_opt(value).single()
    } else {
        Utc.timestamp_opt(value, 0).single()
    }
}

fn strip_zone_abbreviation(raw: &str) -> &str {
    match raw.rsplit_once(' ') {
        Some((head, tail))
            if !tail.is_empty()
                && tail.chars().all(|c| c.is_ascii_uppercase())
                && head.contains(['+', '-']) =>
        {
            head
        },
        _ => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(raw: &str) -> String {
        parse_timestamp(raw)
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "none".into())
    }

    #[test]
    fn parses_every_msgvault_layout() {
        assert_eq!(
            at("2024-03-05 14:30:00.123456789-07:00"),
            "2024-03-05T21:30:00.123456789+00:00"
        );
        assert_eq!(at("2024-03-05 14:30:00"), "2024-03-05T14:30:00+00:00");
        assert_eq!(at("2024-03-05T14:30:00Z"), "2024-03-05T14:30:00+00:00");
        assert_eq!(
            at("2024-03-05 14:30:00 +0100 CET"),
            "2024-03-05T13:30:00+00:00"
        );
        assert_eq!(at("2024-03-05"), "2024-03-05T00:00:00+00:00");
        assert_eq!(at("garbage"), "none");
        assert_eq!(at(""), "none");
    }

    #[test]
    fn epoch_units_are_detected() {
        assert_eq!(from_epoch(1_700_000_000), from_epoch(1_700_000_000_000));
    }
}
