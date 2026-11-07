//! The two date forms rulebooks use, without a date library: UTC instants written exactly as
//! `YYYY-MM-DDTHH:MM:SSZ`, and calendar periods `YYYY-MM` (a month) and `YYYY-MM-DD` (a day).
//! One instant has one spelling, so the rules hash never depends on how a time was written.
//! Years are limited to 2000-2100, which keeps every computation far from overflow.

/// Seconds since 1970-01-01T00:00:00Z.
pub type Unix = i64;

const MIN_YEAR: i64 = 2000;
const MAX_YEAR: i64 = 2100;
/// Seconds in a day.
pub const DAY: Unix = 86_400;

fn digits(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a civil date (H. Hinnant's `days_from_civil`).
#[allow(clippy::arithmetic_side_effects)] // year in [2000, 2100], month in [1, 12], day in [1, 31]
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn year_month(y: &str, m: &str) -> Option<(i64, i64)> {
    let (year, month) = (digits(y)?, digits(m)?);
    (y.len() == 4
        && m.len() == 2
        && (MIN_YEAR..=MAX_YEAR).contains(&year)
        && (1..=12).contains(&month))
    .then_some((year, month))
}

fn date(s: &str) -> Option<(i64, i64, i64)> {
    let mut parts = s.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || d.len() != 2 {
        return None;
    }
    let (year, month) = year_month(y, m)?;
    let day = digits(d)?;
    (1..=days_in_month(year, month))
        .contains(&day)
        .then_some((year, month, day))
}

/// A UTC instant in the form `YYYY-MM-DDTHH:MM:SSZ`, as Unix seconds.
#[must_use]
#[allow(clippy::arithmetic_side_effects)] // bounded: see the module comment
pub fn instant(s: &str) -> Option<Unix> {
    let body = s.strip_suffix('Z')?;
    let (d, t) = body.split_once('T')?;
    let (year, month, day) = date(d)?;
    let mut hms = t.split(':');
    let (h, mi, se) = (hms.next()?, hms.next()?, hms.next()?);
    if hms.next().is_some() || h.len() != 2 || mi.len() != 2 || se.len() != 2 {
        return None;
    }
    let (h, mi, se) = (digits(h)?, digits(mi)?, digits(se)?);
    if h > 23 || mi > 59 || se > 59 {
        return None;
    }
    Some(days_from_civil(year, month, day) * DAY + h * 3600 + mi * 60 + se)
}

/// Which kind of period a series is observed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodKind {
    /// A calendar month, written `YYYY-MM`.
    Month,
    /// A calendar day, written `YYYY-MM-DD`.
    Day,
}

/// A period's start and end (exclusive) as UTC instants at midnight.
#[must_use]
#[allow(clippy::arithmetic_side_effects)] // bounded: see the module comment
pub fn period(kind: PeriodKind, s: &str) -> Option<(Unix, Unix)> {
    match kind {
        PeriodKind::Month => {
            let (y, m) = s.split_once('-')?;
            let (year, month) = year_month(y, m)?;
            let start = days_from_civil(year, month, 1) * DAY;
            Some((start, start + days_in_month(year, month) * DAY))
        }
        PeriodKind::Day => {
            let (year, month, day) = date(s)?;
            let start = days_from_civil(year, month, day) * DAY;
            Some((start, start + DAY))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        assert_eq!(instant("2000-01-01T00:00:00Z"), Some(946_684_800));
        assert_eq!(instant("2026-11-07T13:45:30Z"), Some(1_794_059_130));
        assert_eq!(instant("2024-02-29T23:59:59Z"), Some(1_709_251_199));
        assert_eq!(instant("2100-12-31T23:59:59Z"), Some(4_133_980_799));
    }

    #[test]
    fn only_one_spelling_is_accepted() {
        for bad in [
            "2026-11-07T13:45:30",
            "2026-11-07T13:45:30+00:00",
            "2026-11-07 13:45:30Z",
            "2026-11-07T13:45:30.0Z",
            "2026-1-07T13:45:30Z",
            "2026-11-07T24:00:00Z",
            "2026-11-07T13:60:00Z",
            "2026-11-07T13:45:60Z",
            "2023-02-29T00:00:00Z",
            "1999-12-31T23:59:59Z",
            "2101-01-01T00:00:00Z",
            "+026-11-07T13:45:30Z",
            "2026-11-31T00:00:00Z",
            "",
        ] {
            assert_eq!(instant(bad), None, "{bad}");
        }
    }

    #[test]
    fn periods() {
        let (s, e) = period(PeriodKind::Month, "2024-02").unwrap();
        assert_eq!(
            (s, e - s),
            (instant("2024-02-01T00:00:00Z").unwrap(), 29 * DAY)
        );
        let (s, e) = period(PeriodKind::Day, "2026-12-31").unwrap();
        assert_eq!(
            (s, e),
            (
                instant("2026-12-31T00:00:00Z").unwrap(),
                instant("2027-01-01T00:00:00Z").unwrap()
            )
        );
        assert_eq!(period(PeriodKind::Month, "2026-13"), None);
        assert_eq!(period(PeriodKind::Month, "2026-09-01"), None);
        assert_eq!(period(PeriodKind::Day, "2026-09"), None);
    }
}
