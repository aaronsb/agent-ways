//! Compact message timestamps — the one datetime format every attend
//! surface shares (issue #389): attend-chat cells, `attend inbox`
//! listings, and the ADR-172 drain injection. One implementation so
//! the surfaces cannot drift.
//!
//! Dependency-free by conviction (this workspace carries no date
//! crate): civil-date conversion is the standard days-from-epoch
//! algorithm, and the local UTC offset is probed once per process via
//! `date +%z` (POSIX), falling back to UTC when unavailable.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Render `t` compactly relative to `now`:
/// same local day → `HH:MM`; same year → `MM-DD HH:MM`;
/// older → `YYYY-MM-DD`.
pub fn compact_time(t: SystemTime, now: SystemTime) -> String {
    compact_time_with_offset(t, now, local_offset_secs())
}

/// Offset-injected core, exposed for tests and for callers that carry
/// their own zone.
pub fn compact_time_with_offset(t: SystemTime, now: SystemTime, offset_secs: i64) -> String {
    let ts = unix_secs(t) + offset_secs;
    let ns = unix_secs(now) + offset_secs;
    let (ty, tm, td) = civil_from_days(ts.div_euclid(86_400));
    let (ny, nm, nd) = civil_from_days(ns.div_euclid(86_400));
    let secs_of_day = ts.rem_euclid(86_400);
    let (hh, mm) = (secs_of_day / 3600, (secs_of_day % 3600) / 60);

    if (ty, tm, td) == (ny, nm, nd) {
        format!("{hh:02}:{mm:02}")
    } else if ty == ny {
        format!("{tm:02}-{td:02} {hh:02}:{mm:02}")
    } else {
        format!("{ty:04}-{tm:02}-{td:02}")
    }
}

fn unix_secs(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// Days-since-epoch → (year, month, day). Howard Hinnant's
/// `civil_from_days`, the standard branch-free civil calendar
/// conversion.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Local UTC offset in seconds, probed once via `date +%z` (`±HHMM`).
/// Zero (UTC) when the probe fails — a wrong-zone timestamp is worse
/// than an honest UTC one.
fn local_offset_secs() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| parse_utc_offset(s.trim()))
            .unwrap_or(0)
    })
}

/// Parse `±HHMM` (also tolerates `±HH:MM`).
fn parse_utc_offset(s: &str) -> Option<i64> {
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => (1, s),
    };
    let rest = rest.replace(':', "");
    if rest.len() != 4 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hh: i64 = rest[..2].parse().ok()?;
    let mm: i64 = rest[2..].parse().ok()?;
    Some(sign * (hh * 3600 + mm * 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    // 2026-07-22 15:04:05 UTC
    const NOW: u64 = 1_784_732_645;

    #[test]
    fn same_day_renders_clock_only() {
        let t = at(NOW - 3 * 3600);
        assert_eq!(compact_time_with_offset(t, at(NOW), 0), "12:04");
    }

    #[test]
    fn same_year_renders_month_day_clock() {
        let t = at(NOW - 40 * 86_400);
        assert_eq!(compact_time_with_offset(t, at(NOW), 0), "06-12 15:04");
    }

    #[test]
    fn older_years_render_date_only() {
        let t = at(NOW - 400 * 86_400);
        assert_eq!(compact_time_with_offset(t, at(NOW), 0), "2025-06-17");
    }

    #[test]
    fn offset_shifts_the_civil_day_boundary() {
        // 00:30 UTC with a -2h offset is 22:30 the PREVIOUS local day.
        let midnightish = (NOW / 86_400) * 86_400 + 1_800;
        let rendered = compact_time_with_offset(at(midnightish), at(NOW), -7_200);
        assert_eq!(rendered, "07-21 22:30");
    }

    #[test]
    fn parses_offset_formats() {
        assert_eq!(parse_utc_offset("+0000"), Some(0));
        assert_eq!(parse_utc_offset("-0500"), Some(-18_000));
        assert_eq!(parse_utc_offset("+05:30"), Some(19_800));
        assert_eq!(parse_utc_offset("garbage"), None);
    }

    #[test]
    fn civil_conversion_hits_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // leap year start
        assert_eq!(civil_from_days(20_656), (2026, 7, 22));
    }
}
