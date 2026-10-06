//! The decision log's live bound, counted in turns (ADR-701 §2).
//!
//! A turn is a decision record with `"turn_start":true` together with every
//! record after it up to the next such record: the turn's task scans
//! (`turn_start` false) and its pulls. Records before the first turn start
//! belong to no counted turn and leave with the oldest turns. The epoch is not
//! a turn index (see `cmd::scan::decision::Context`), so turns are counted
//! from the records alone.

use super::{archive_removed, mark_archive_failed, same_file, Stream};

/// The marker a turn's first record carries, as `serde_json` writes it. Inside
/// a string value the quotes are escaped, so a quoted copy never matches.
const TURN_START: &[u8] = b"\"turn_start\":true";

/// Move the oldest turns of the decision log at `path` to the day's archive so
/// the newest `keep` remain, once the log holds more than `keep` plus 10%. The
/// gap gives the hysteresis the event log's KEEP/MAX gives it: a trim moves at
/// least a tenth of the bound, so it is not a daily rewrite for a few turns.
/// Returns whether anything moved.
///
/// The count is a streamed substring scan, so a day with nothing to trim
/// costs one read of the file and little memory. The cut falls at the start
/// of a turn, so a turn is never split. The head is archived, durably, before
/// the log is replaced; when that fails nothing is removed and today's failure
/// marker is set. As in [`super::rotate_log_by_age`], one handle serves the
/// scan, the copy and the carry of appends that land during the rewrite, and a
/// log replaced meanwhile makes the trim stand down. Callers hold the log lock.
pub(super) fn trim_to_turns(path: &std::path::Path, stream: Stream, now: u64, keep: u64) -> std::io::Result<bool> {
    trim_to_turns_hooked(path, stream, now, keep, &mut || {})
}

/// [`trim_to_turns`] with `before_publish` run once the survivors are read and
/// before the file is checked and replaced, for tests that need something to
/// happen in that window.
fn trim_to_turns_hooked(path: &std::path::Path, stream: Stream, now: u64, keep: u64, before_publish: &mut dyn FnMut()) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let starts = turn_starts(&f)?;
    let (count, keep) = (starts.len() as u64, keep.max(1));
    if count <= keep.saturating_add(keep / 10) {
        return Ok(false);
    }
    let cut = starts[(count - keep) as usize];
    (&f).seek(SeekFrom::Start(0))?;
    let mut head = vec![0; cut as usize];
    (&f).read_exact(&mut head)?;
    // The kept turns, up to the handle's own length.
    let (mut out, mut pos) = (Vec::new(), cut);
    loop {
        pos += (&f).read_to_end(&mut out)? as u64;
        if f.metadata()?.len() <= pos {
            break;
        }
    }
    before_publish();
    if !same_file(&f, path) {
        return Ok(false);
    }
    if let Err(e) = archive_removed(path, stream, now, &head) {
        mark_archive_failed(path.parent().unwrap_or(std::path::Path::new(".")), stream, now);
        return Err(e);
    }
    agent_settings::writer::write_atomic(path, &out)?;
    // Records that landed on the old file while the new one was written.
    (&f).seek(SeekFrom::Start(pos))?;
    let mut gained = Vec::new();
    (&f).read_to_end(&mut gained)?;
    if !gained.is_empty() {
        std::fs::OpenOptions::new().append(true).open(path)?.write_all(&gained)?;
    }
    Ok(true)
}

/// The byte offset of every line in `f` that starts a turn, in file order.
fn turn_starts(f: &std::fs::File) -> std::io::Result<Vec<u64>> {
    use std::io::{BufRead, Seek, SeekFrom};
    (&*f).seek(SeekFrom::Start(0))?;
    let mut reader = std::io::BufReader::with_capacity(256 * 1024, f);
    let (mut starts, mut pos, mut line) = (Vec::new(), 0u64, Vec::new());
    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(starts);
        }
        if line.windows(TURN_START.len()).any(|w| w == TURN_START) {
            starts.push(pos);
        }
        pos += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    use ways_core::event_archive::{archive_path, archives, read_source};

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;
    const KEEP: u64 = 10;

    fn state(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("ways-turns-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        (d.clone(), d.join(DECISIONS.live_name()))
    }

    fn ts() -> String {
        agent_fmt::when::utc_iso(NOW)
    }

    /// One turn: its prompt record, a task scan, a pull, and `extra` more
    /// task scans, each line naming its turn.
    fn turn(t: usize, extra: usize) -> String {
        let ts = ts();
        let mut s = format!("{{\"ts\":\"{ts}\",\"kind\":\"scan\",\"surface\":\"prompt\",\"turn\":{t},\"turn_start\":true}}\n");
        s.push_str(&format!("{{\"ts\":\"{ts}\",\"kind\":\"scan\",\"surface\":\"task\",\"turn\":{t},\"turn_start\":false}}\n"));
        s.push_str(&format!("{{\"ts\":\"{ts}\",\"kind\":\"pull\",\"turn\":{t},\"way\":\"a/b\"}}\n"));
        for i in 0..extra {
            s.push_str(&format!("{{\"ts\":\"{ts}\",\"kind\":\"scan\",\"surface\":\"task\",\"turn\":{t},\"i\":{i},\"turn_start\":false}}\n"));
        }
        s
    }

    fn turns(range: std::ops::Range<usize>) -> String {
        range.map(|t| turn(t, 0)).collect()
    }

    fn run(dir: &std::path::Path, keep: u64) -> bool {
        rotate_if_due(dir, DECISIONS, NOW, 365, LiveBound::Turns(keep))
    }

    fn archived(dir: &std::path::Path) -> String {
        read_source(&archive_path(dir, DECISIONS, NOW)).unwrap_or_default()
    }

    #[test]
    fn the_oldest_turns_move_whole_to_the_days_archive_and_the_newest_stay() {
        let (dir, log) = state("trim");
        std::fs::write(&log, turns(0..15)).unwrap();
        assert!(run(&dir, KEEP));
        assert_eq!(archived(&dir), turns(0..5), "the first five turns, with their task scans and pulls, byte for byte");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), turns(5..15), "the newest ten turns stay live");
    }

    #[test]
    fn within_the_hysteresis_nothing_is_trimmed() {
        let (dir, log) = state("hysteresis");
        // Eleven turns of three lines each: 33 lines, but 11 turns is KEEP + 10%.
        let body = turns(0..11);
        std::fs::write(&log, &body).unwrap();
        assert!(!run(&dir, KEEP));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body);
        assert!(archives(&dir, DECISIONS).is_empty());
    }

    #[test]
    fn a_turn_is_never_split() {
        let (dir, log) = state("whole");
        // Records before any turn start, then turns of uneven length.
        let orphan = format!("{{\"ts\":\"{}\",\"kind\":\"scan\",\"surface\":\"task\",\"turn_start\":false}}\n", ts());
        let body: String = std::iter::once(orphan.clone()).chain((0..20).map(|t| turn(t, t % 4))).collect();
        std::fs::write(&log, &body).unwrap();
        assert!(run(&dir, 7));
        let expected_head: String = std::iter::once(orphan).chain((0..13).map(|t| turn(t, t % 4))).collect();
        assert_eq!(archived(&dir), expected_head);
        let live = std::fs::read_to_string(&log).unwrap();
        assert_eq!(live, (13..20).map(|t| turn(t, t % 4)).collect::<String>());
        assert!(live.starts_with(&format!("{{\"ts\":\"{}\",\"kind\":\"scan\",\"surface\":\"prompt\",\"turn\":13,", ts())));
    }

    #[test]
    fn malformed_lines_and_quoted_markers_are_not_turns() {
        let (dir, log) = state("malformed");
        // A record whose string value spells the marker, escaped as JSON escapes it.
        let quoted = serde_json::json!({"ts": ts(), "kind": "scan", "matched_span": "\"turn_start\":true", "turn_start": false}).to_string() + "\n";
        let body = format!("{}{{\"ts\":\"{}\",\"kind\":\"sc\nnot json at all\n{quoted}{}", turns(0..5), ts(), turns(5..11));
        std::fs::write(&log, &body).unwrap();
        assert!(!run(&dir, KEEP), "eleven turns, within the hysteresis");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body);
        // Past it, the damaged lines leave with the turn they sit in.
        let damaged = format!("{{\"ts\":\"{}\",\"kind\":\"sc\nnot json at all\n{quoted}", ts());
        std::fs::write(&log, format!("{}{damaged}{}", turns(0..5), turns(5..15))).unwrap();
        assert!(trim_to_turns(&log, DECISIONS, NOW, KEEP).unwrap());
        assert_eq!(archived(&dir), turns(0..5) + &damaged);
        assert_eq!(std::fs::read_to_string(&log).unwrap(), turns(5..15));
    }

    #[test]
    fn a_busy_lock_trims_nothing_and_does_not_spend_the_days_claim() {
        let (dir, log) = state("busy");
        let body = turns(0..15);
        std::fs::write(&log, &body).unwrap();
        let held = try_log_lock(&dir, DECISIONS).unwrap();
        LOCK_PATIENCE.with(|p| p.set(std::time::Duration::ZERO));
        assert!(!run(&dir, KEEP));
        LOCK_PATIENCE.with(|p| p.set(std::time::Duration::from_secs(10)));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body);
        assert!(!dir.join(day_file(&rotate_claim_prefix(DECISIONS), NOW)).exists(), "the day's claim is still open");
        drop(held);
        assert!(run(&dir, KEEP), "the day's trim runs once the lock frees");
        assert!(!run(&dir, KEEP), "and once only");
    }

    #[test]
    fn the_events_claim_does_not_spend_the_decisions_trim() {
        let (dir, log) = state("own-claim");
        std::fs::write(dir.join(EVENTS.live_name()), format!("{{\"ts\":\"{}\",\"event\":\"way_fired\"}}\n", ts())).unwrap();
        assert!(!rotate_if_due(&dir, EVENTS, NOW, 365, EVENT_BOUND), "the events pass ran and found nothing old");
        std::fs::write(&log, turns(0..15)).unwrap();
        assert!(run(&dir, KEEP), "the decisions trim still runs today");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), turns(5..15));
    }

    #[test]
    fn an_archive_failure_removes_nothing_and_marks_the_day() {
        let (dir, log) = state("arch-fail");
        let body = turns(0..15);
        std::fs::write(&log, &body).unwrap();
        std::fs::create_dir(archive_path(&dir, DECISIONS, NOW)).unwrap(); // the archive cannot be opened
        assert!(trim_to_turns(&log, DECISIONS, NOW, KEEP).is_err());
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body, "the live file is untouched");
        assert!(archive_failed_today(&dir, DECISIONS, NOW) && !archive_failed_today(&dir, EVENTS, NOW));
    }

    #[test]
    fn records_appended_during_the_rewrite_are_carried_over() {
        let (dir, log) = state("carry");
        std::fs::write(&log, turns(0..15)).unwrap();
        let late = turn(15, 1);
        let target = log.clone();
        let mut hook = {
            let late = late.clone();
            move || {
                for line in late.lines() {
                    append_jsonl_line(&target, line);
                }
            }
        };
        assert!(trim_to_turns_hooked(&log, DECISIONS, NOW, KEEP, &mut hook).unwrap());
        assert_eq!(std::fs::read_to_string(&log).unwrap(), turns(5..15) + &late);
        assert_eq!(archived(&dir), turns(0..5));
    }

    #[test]
    fn a_missing_log_is_a_noop() {
        let (dir, log) = state("missing");
        assert!(!trim_to_turns(&log, DECISIONS, NOW, KEEP).unwrap());
        assert!(archives(&dir, DECISIONS).is_empty());
    }

    #[test]
    fn expiry_removes_only_decisions_archives_past_the_retention() {
        let (dir, log) = state("expire");
        std::fs::write(&log, turns(0..2)).unwrap();
        let append = |stream, at| ways_core::event_archive::append(&dir, stream, at, b"x\n").unwrap();
        append(DECISIONS, NOW - 400 * DAY);
        append(DECISIONS, NOW - 30 * DAY);
        append(EVENTS, NOW - 400 * DAY);
        assert!(!run(&dir, KEEP), "two turns: nothing to trim");
        assert_eq!(archives(&dir, DECISIONS), [archive_path(&dir, DECISIONS, NOW - 30 * DAY)]);
        assert_eq!(archives(&dir, EVENTS), [archive_path(&dir, EVENTS, NOW - 400 * DAY)], "the events archive is not the decisions pass's");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), turns(0..2));
    }

    /// The writer runs the day's trim before it appends, as the event writer
    /// runs its rotation; with the day already checked it only appends.
    #[test]
    fn writing_a_record_runs_the_days_trim_first() {
        let (dir, log) = state("wired");
        std::fs::write(&log, turns(0..15)).unwrap();
        let record = serde_json::json!({"ts": ts(), "kind": "scan", "turn": 15, "turn_start": true});
        log_decision_to(&dir, NOW, None, &record);
        assert_eq!(std::fs::read_to_string(&log).unwrap(), turns(0..15) + &record.to_string() + "\n", "no trim when the day was checked");
        log_decision_to(&dir, NOW, Some((365, KEEP)), &record);
        let live = std::fs::read_to_string(&log).unwrap();
        assert_eq!(live, turns(6..15) + &record.to_string() + "\n" + &record.to_string() + "\n");
        assert_eq!(archived(&dir), turns(0..6));
    }
}
