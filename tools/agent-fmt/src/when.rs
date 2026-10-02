//! Date and duration maths for every agent-ways tool: the civil-date
//! conversion, the UTC ISO-8601 stamp the event log and findings carry and
//! its parser, and the compact message timestamps every attend surface
//! shares (issue #389): attend-chat cells, `attend inbox` listings, and the
//! ADR-172 drain injection. One implementation so the surfaces cannot drift.
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

/// Seconds since the Unix epoch, now. Zero if the clock reads before it.
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `YYYY-MM-DDThh:mm:ssZ` for Unix seconds `secs` (UTC): the stamp the event
/// log, the findings ledger and the session files carry.
pub fn utc_iso(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let tod = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", tod / 3600, (tod % 3600) / 60, tod % 60)
}

/// [`utc_iso`] for now.
pub fn now_utc_iso() -> String {
    utc_iso(now_secs())
}

/// `YYYY-MM-DD` for Unix seconds `secs` (UTC).
pub fn utc_date(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM-DDThh:mm:ss[.fff]Z` to Unix seconds. Only UTC (`Z`) is accepted:
/// an offset or a missing zone returns `None` rather than a wrong instant.
/// Fractional seconds are dropped; so is anything before 1970.
pub fn parse_utc_iso(s: &str) -> Option<u64> {
    let s = s.trim().strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let m: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    if d.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    let time = time.split('.').next()?;
    let mut t = time.split(':');
    let hh: u64 = t.next()?.parse().ok()?;
    let mm: u64 = t.next()?.parse().ok()?;
    let ss: u64 = t.next()?.parse().ok()?;
    if t.next().is_some() || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let days = u64::try_from(days_from_civil(y, m, day)).ok()?;
    Some(days * 86_400 + hh * 3_600 + mm * 60 + ss)
}

/// A compact "N ago" label: seconds, minutes, hours, then days.
pub fn ago(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

/// An elapsed time: `45s`, `3m 12s`, `2h 5m`.
pub fn duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn unix_secs(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// (year, month, day) → days since 1970-01-01. Howard Hinnant's
/// `days_from_civil`, the inverse of [`civil_from_days`].
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Days-since-epoch → (year, month, day). Howard Hinnant's
/// `civil_from_days`, the standard branch-free civil calendar
/// conversion.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
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
    fn utc_stamps_render_and_parse_back() {
        assert_eq!(utc_iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_iso(NOW), "2026-07-22T15:04:05Z");
        assert_eq!(utc_date(NOW), "2026-07-22");
        assert_eq!(parse_utc_iso(&utc_iso(NOW)), Some(NOW));
        assert_eq!(parse_utc_iso("2026-09-17T20:37:03.286Z"), Some(1_789_677_423));
        assert_eq!(parse_utc_iso("2000-03-01T00:00:00Z"), Some(951_868_800));
        assert_eq!(parse_utc_iso("garbage"), None);
        assert_eq!(parse_utc_iso("2026-07"), None);
    }

    /// Differences are calendar-correct across month and leap-day boundaries
    /// (the old `parse_ts_secs` contract its callers rely on).
    #[test]
    fn parsed_differences_are_calendar_correct() {
        let s = |t: &str| parse_utc_iso(t).unwrap();
        assert_eq!(s("2026-07-03T01:02:03Z"), s("2026-07-03T01:02:03.999Z"));
        assert_eq!(s("2026-02-01T00:00:00Z") - s("2026-01-31T00:00:00Z"), 86_400);
        assert_eq!(s("2024-03-01T00:00:00Z") - s("2024-02-29T00:00:00Z"), 86_400);
    }

    /// Only UTC is read. The rethink copy took the first 19 characters and
    /// read `+02:00` as UTC, two hours off; the event-log copy did the same
    /// with no zone at all.
    #[test]
    fn parse_rejects_offsets_and_missing_zones() {
        assert_eq!(parse_utc_iso("2026-09-17T20:37:03+02:00"), None);
        assert_eq!(parse_utc_iso("2026-09-17T20:37:03"), None);
    }

    #[test]
    fn days_from_civil_inverts_civil_from_days() {
        for z in [-800_000, -1, 0, 11_016, 19_723, 20_656, 2_000_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }

    #[test]
    fn ago_and_duration_scale_units() {
        assert_eq!(ago(5), "5s ago");
        assert_eq!(ago(125), "2m ago");
        assert_eq!(ago(7_200), "2h ago");
        assert_eq!(ago(172_800), "2d ago");
        assert_eq!(duration(45), "45s");
        assert_eq!(duration(192), "3m 12s");
        assert_eq!(duration(7_500), "2h 5m");
    }

    #[test]
    fn civil_conversion_hits_known_dates() {
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_565), (2026, 4, 22));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1)); // leap year start
        assert_eq!(civil_from_days(20_656), (2026, 7, 22));
    }
}
