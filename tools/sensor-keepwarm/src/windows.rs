//! The keepalive policy's time windows (ADR-414 item 5): a list of entries,
//! each `<days> <HH:MM>-<HH:MM>`, read in an IANA zone or the host's own.
//!
//! - days are `daily`, one day (`Sat`), or a range (`Mon-Fri`); a range may
//!   wrap the week (`Fri-Mon`). Day names are the three-letter English ones,
//!   in any case.
//! - `24:00` closes a day and is allowed only as an end. An end before its
//!   start crosses midnight and belongs to the start's day. An end equal to
//!   its start is an empty window and is dropped.
//! - an entry that does not parse is dropped and named; a list whose every
//!   entry is dropped is empty, and an empty list is never inside.

use jiff::tz::TimeZone;
use jiff::Timestamp;

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// The default list: no time limit, the count the only bound.
pub const DEFAULT_WINDOW: &str = "daily 00:00-24:00";

/// One entry, as parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// Monday is bit 0.
    days: u8,
    start: u16,
    end: u16,
    /// The entry as written, for the card and the wrap's reason.
    pub text: String,
}

impl Window {
    /// Whether the minute `m` of weekday `day` (Monday 0) is inside.
    fn contains(&self, day: u8, m: u16) -> bool {
        let on = |d: u8| self.days & (1 << d) != 0;
        if self.start < self.end {
            on(day) && self.start <= m && m < self.end
        } else {
            // Crosses midnight: the start's day from the start, and the
            // morning of the day after it up to the end.
            (on(day) && m >= self.start) || (on((day + 6) % 7) && m < self.end)
        }
    }
}

fn day_index(s: &str) -> Option<u8> {
    let s = s.to_ascii_lowercase();
    DAYS.iter().position(|d| *d == s).map(|i| i as u8)
}

fn parse_days(s: &str) -> Result<u8, String> {
    if s.eq_ignore_ascii_case("daily") {
        return Ok(0x7f);
    }
    let (a, b) = match s.split_once('-') {
        Some((a, b)) => (a, b),
        None => (s, s),
    };
    let a = day_index(a).ok_or_else(|| format!("`{a}` is not a day (Mon to Sun, or daily)"))?;
    let b = day_index(b).ok_or_else(|| format!("`{b}` is not a day (Mon to Sun, or daily)"))?;
    let mut mask = 0u8;
    let mut d = a;
    loop {
        mask |= 1 << d;
        if d == b {
            break;
        }
        d = (d + 1) % 7;
    }
    Ok(mask)
}

fn parse_time(s: &str, is_end: bool) -> Result<u16, String> {
    let bad = || format!("`{s}` is not a time (HH:MM)");
    let (h, m) = s.split_once(':').ok_or_else(bad)?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return Err(bad());
    }
    let h: u16 = h.parse().map_err(|_| bad())?;
    let m: u16 = m.parse().map_err(|_| bad())?;
    if m > 59 {
        return Err(bad());
    }
    let t = h * 60 + m;
    match t {
        1440 if is_end => Ok(t),
        1440 => Err("24:00 only closes a window".into()),
        t if t > 1440 => Err(bad()),
        t => Ok(t),
    }
}

/// One entry, `<days> <HH:MM>-<HH:MM>`.
pub fn parse_window(text: &str) -> Result<Window, String> {
    let t = text.split_whitespace().collect::<Vec<_>>();
    let [days, span] = t.as_slice() else {
        return Err("an entry is `<days> <HH:MM>-<HH:MM>`".into());
    };
    let days = parse_days(days)?;
    let (a, b) = span.split_once('-').ok_or("the times are `<HH:MM>-<HH:MM>`")?;
    let start = parse_time(a, false)?;
    let end = parse_time(b, true)?;
    if start == end {
        return Err("the window is empty: its end is its start".into());
    }
    Ok(Window { days, start, end, text: t.join(" ") })
}

/// The windows in force, with the entries dropped and the zone they are
/// read in.
#[derive(Clone, Debug)]
pub struct Windows {
    pub list: Vec<Window>,
    /// Each dropped entry, with why.
    pub dropped: Vec<(String, String)>,
    tz: TimeZone,
    /// The zone named in `timezone`, when it is not one the host knows; the
    /// host's zone is used in its place.
    pub bad_zone: Option<String>,
}

impl Default for Windows {
    fn default() -> Self {
        Windows::parse(&[DEFAULT_WINDOW.to_string()], "")
    }
}

impl Windows {
    /// `entries` read in `zone`, an IANA name, or the host's zone when empty.
    pub fn parse(entries: &[String], zone: &str) -> Self {
        let mut list = Vec::new();
        let mut dropped = Vec::new();
        for e in entries {
            match parse_window(e) {
                Ok(w) => list.push(w),
                Err(why) => dropped.push((e.clone(), why)),
            }
        }
        let (tz, bad_zone) = zone_of(zone);
        Windows { list, dropped, tz, bad_zone }
    }

    /// The same windows read in `tz`, for tests.
    pub fn with_zone(mut self, tz: TimeZone) -> Self {
        self.tz = tz;
        self
    }

    /// Whether `epoch` falls inside any window.
    pub fn contains(&self, epoch: u64) -> bool {
        let Some((day, m)) = self.civil(epoch) else { return false };
        self.list.iter().any(|w| w.contains(day, m))
    }

    /// Weekday (Monday 0) and minute of the day at `epoch` in the zone.
    fn civil(&self, epoch: u64) -> Option<(u8, u16)> {
        let ts = Timestamp::from_second(epoch as i64).ok()?;
        let z = ts.to_zoned(self.tz.clone());
        let day = z.weekday().to_monday_zero_offset() as u8;
        Some((day, z.hour() as u16 * 60 + z.minute() as u16))
    }

    /// `HH:MM` at `epoch` in the zone.
    pub fn hm(&self, epoch: u64) -> String {
        match Timestamp::from_second(epoch as i64) {
            Ok(ts) => {
                let z = ts.to_zoned(self.tz.clone());
                format!("{:02}:{:02}", z.hour(), z.minute())
            }
            Err(_) => "--:--".into(),
        }
    }

    /// `YYYY-MM-DD` at `epoch` in the zone.
    pub fn date(&self, epoch: u64) -> String {
        match Timestamp::from_second(epoch as i64) {
            Ok(ts) => {
                let z = ts.to_zoned(self.tz.clone());
                format!("{:04}-{:02}-{:02}", z.year(), z.month(), z.day())
            }
            Err(_) => "0000-00-00".into(),
        }
    }

    /// The entries in force, as the card and the wrap's reason name them.
    pub fn describe(&self) -> String {
        if self.list.is_empty() {
            "none".into()
        } else {
            self.list.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(", ")
        }
    }
}

/// The zone `name` names, or the host's when it is empty or unknown; the
/// second value is an unknown name.
fn zone_of(name: &str) -> (TimeZone, Option<String>) {
    let name = name.trim();
    if name.is_empty() {
        return (TimeZone::try_system().unwrap_or(TimeZone::UTC), None);
    }
    match TimeZone::get(name) {
        Ok(tz) => (tz, None),
        Err(_) => (TimeZone::try_system().unwrap_or(TimeZone::UTC), Some(name.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(hours: i8) -> TimeZone {
        TimeZone::fixed(jiff::tz::offset(hours))
    }

    /// 2026-10-12 is a Monday; `at(day, "HH:MM")` is that day plus `day`
    /// days, at that time in UTC.
    fn at(day: u64, hm: &str) -> u64 {
        let (h, m) = hm.split_once(':').unwrap();
        let monday = 1_791_763_200; // 2026-10-12T00:00:00Z
        monday + day * 86_400 + h.parse::<u64>().unwrap() * 3600 + m.parse::<u64>().unwrap() * 60
    }

    fn utc(entries: &[&str]) -> Windows {
        Windows::parse(&entries.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "").with_zone(TimeZone::UTC)
    }

    #[test]
    fn the_anchor_is_a_monday() {
        assert_eq!(utc(&[]).civil(at(0, "00:00")), Some((0, 0)));
        assert_eq!(utc(&[]).civil(at(6, "23:59")), Some((6, 1439)));
    }

    #[test]
    fn the_default_is_always_inside() {
        let w = utc(&[DEFAULT_WINDOW]);
        for day in 0..7 {
            for hm in ["00:00", "08:30", "23:59"] {
                assert!(w.contains(at(day, hm)), "{day} {hm}");
            }
        }
    }

    #[test]
    fn an_empty_list_is_never_inside() {
        assert!(!utc(&[]).contains(at(0, "12:00")));
        let all_bad = utc(&["sometimes", "Mon 9-5"]);
        assert!(all_bad.list.is_empty());
        assert_eq!(all_bad.dropped.len(), 2);
        assert!(!all_bad.contains(at(0, "12:00")));
    }

    #[test]
    fn working_hours_on_weekdays() {
        let w = utc(&["Mon-Fri 08:00-19:00"]);
        assert!(!w.contains(at(0, "07:59")));
        assert!(w.contains(at(0, "08:00")));
        assert!(w.contains(at(4, "18:59")));
        assert!(!w.contains(at(4, "19:00")));
        assert!(!w.contains(at(5, "12:00")), "Saturday");
        assert_eq!(w.describe(), "Mon-Fri 08:00-19:00");
    }

    #[test]
    fn a_range_wraps_the_week() {
        let w = utc(&["Fri-Mon 10:00-11:00"]);
        for (day, inside) in [(0, true), (1, false), (3, false), (4, true), (5, true), (6, true)] {
            assert_eq!(w.contains(at(day, "10:30")), inside, "day {day}");
        }
    }

    #[test]
    fn an_end_before_its_start_crosses_midnight_and_belongs_to_the_start_day() {
        let w = utc(&["Fri 22:00-02:00"]);
        assert!(w.contains(at(4, "23:00")));
        assert!(w.contains(at(5, "01:59")), "Saturday morning belongs to Friday's window");
        assert!(!w.contains(at(5, "02:00")));
        assert!(!w.contains(at(3, "23:00")), "Thursday");
        assert!(!w.contains(at(4, "01:00")), "Friday morning belongs to Thursday");
    }

    #[test]
    fn twenty_four_hundred_closes_a_day_and_only_closes() {
        assert!(utc(&["Sat 20:00-24:00"]).contains(at(5, "23:59")));
        assert!(!utc(&["Sat 20:00-24:00"]).contains(at(6, "00:00")));
        assert!(parse_window("Sat 24:00-02:00").is_err());
    }

    #[test]
    fn bad_entries_are_dropped_and_named() {
        for bad in ["Mon", "Mon 8:00", "Someday 08:00-09:00", "Mon 25:00-26:00", "Mon 08:60-09:00", "Mon 08:00-08:00", "Mon 08:00-09:00 extra"] {
            assert!(parse_window(bad).is_err(), "{bad}");
        }
        let w = utc(&["Mon-Fri 08:00-19:00", "Tue 9am-5pm"]);
        assert_eq!(w.list.len(), 1);
        assert_eq!(w.dropped[0].0, "Tue 9am-5pm");
        assert!(w.dropped[0].1.contains("not a time"), "{:?}", w.dropped);
    }

    #[test]
    fn day_names_take_any_case_and_one_day_stands_alone() {
        let w = utc(&["SAT 00:00-24:00", "daily 03:00-04:00"]);
        assert!(w.contains(at(5, "12:00")));
        assert!(w.contains(at(2, "03:30")));
        assert!(!w.contains(at(2, "04:30")));
    }

    #[test]
    fn the_zone_moves_the_window() {
        // 07:30 UTC is 09:30 at UTC+2: inside 08:00-19:00 there, not in UTC.
        let entries = vec!["Mon-Fri 08:00-19:00".to_string()];
        assert!(!utc(&["Mon-Fri 08:00-19:00"]).contains(at(0, "07:30")));
        assert!(Windows::parse(&entries, "").with_zone(fixed(2)).contains(at(0, "07:30")));
        assert_eq!(Windows::parse(&entries, "").with_zone(fixed(2)).hm(at(0, "07:30")), "09:30");
    }

    #[test]
    fn an_iana_name_is_read_and_an_unknown_one_named() {
        let entries = vec![DEFAULT_WINDOW.to_string()];
        let w = Windows::parse(&entries, "Not/AZone");
        assert_eq!(w.bad_zone.as_deref(), Some("Not/AZone"));
        // The system tzdb is present on the hosts attend builds for; where
        // it is not, the name is reported as unknown, which this allows.
        let utc = Windows::parse(&entries, "UTC");
        if utc.bad_zone.is_none() {
            assert_eq!(utc.hm(at(0, "07:30")), "07:30");
            assert_eq!(utc.date(at(2, "07:30")), "2026-10-14");
        }
    }
}
