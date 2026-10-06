//! The agent-ways log streams' writer: append, size compaction, daily age
//! rotation and archiving (ADR-134 task E, ADR-701 §2).
//!
//! Hooks append with `O_APPEND` and take no lock. Compaction and rotation
//! rewrite the live file under a sidecar lock, archive what they remove
//! through `ways_core::event_archive` first, and carry over appends that land
//! while they work. Extracted from `session.rs`, which had passed the
//! 800-line priority threshold, when ADR-701 §2 added a second stream.

use ways_core::event_archive::EVENTS;

/// Append one JSONL record as a single `write` on an `O_APPEND` handle.
///
/// `writeln!` on a `File` issues two writes, the record and then the newline.
/// Parallel hooks append to the same logs, and two processes interleaving as
/// `recA recB \n \n` corrupt both lines, which readers then drop. That lost the
/// `way_fired` and `way_suppressed` rows of two racing PreToolUse hooks in CI
/// (#528). One buffer, one `write_all`, keeps each record whole.
pub(super) fn append_jsonl_line(path: &std::path::Path, line: &str) {
    use std::io::Write;
    let mut buf = String::with_capacity(line.len() + 1);
    buf.push_str(line);
    buf.push('\n');
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(buf.as_bytes()));
}

// ── Event logging ───────────────────────────────────────────────

/// Size at which the event log is compacted (ADR-134 task E). The near-miss and
/// fire-score streams (tasks A/D) grow `events.jsonl` faster than fires alone,
/// so it needs a ceiling. 32 MiB is about 65 days of history at the rates seen
/// on a working install; what the cap removes goes to the dated archives.
const MAX_EVENTS_BYTES: u64 = 32 * 1024 * 1024;
/// Target size kept after compaction (the most recent bytes). The gap to
/// MAX provides hysteresis: ~8 MiB (~30k events) of new logging between
/// compactions, so the rewrite is rare, not per-append.
const KEEP_EVENTS_BYTES: u64 = 24 * 1024 * 1024;
/// Compaction carries every `judge_call` line (#750), so the file settles at
/// KEEP + judge history. Once that history exceeds MAX - KEEP the file is over
/// MAX right after compacting; a rewrite that frees less than this is skipped so
/// appends do not re-read and rewrite the whole file each time. Non-judge lines
/// then accumulate until a compaction frees at least half the MAX-KEEP gap. The
/// file is bounded by KEEP + judge history + MAX-KEEP/2 + that accumulation.
const MIN_FREED_BYTES: u64 = (MAX_EVENTS_BYTES - KEEP_EVENTS_BYTES) / 2;
/// Days of events the live file holds. Older lines move to the dated archives
/// (ADR-701 §2); `ways.event_retention_days` governs those archives, not this.
const LIVE_EVENT_DAYS: u32 = 90;
/// Archiving wins over the size bound up to this size. Past it with the
/// archive still failing, the cap drops the head unarchived and logs one
/// `event_log_dropped` event, so a broken archive cannot grow the log forever.
const CEILING_EVENTS_BYTES: u64 = 4 * MAX_EVENTS_BYTES;
/// Sidecar that one rotation or compaction at a time holds, non-blocking.
const LOG_LOCK_NAME: &str = "events.compact.lock";
/// Day-dated marker beside the log: the archive failed today, so retry tomorrow.
const ARCHIVE_FAILED_PREFIX: &str = "events.archive-failed-";

/// Log an event to the telemetry log ($XDG_STATE/agent-ways/events.jsonl — see paths::events_log).
pub fn log_event(fields: &[(&str, &str)]) {
    log_event_with(fields, &[]);
}

/// [`log_event`] with structured values beside the string fields, for an event
/// that carries a list (`scan_candidates`). Readers key on `event` and read the
/// fields they know, so an extra nested value costs them nothing.
pub fn log_event_with(fields: &[(&str, &str)], extra: &[(&str, serde_json::Value)]) {
    let now = agent_fmt::when::now_secs();
    let rotate = rotation_due_today(now).then(|| crate::config::global().event_retention_days);
    log_event_to(&crate::paths::events_log(), now, rotate, fields, extra);
}

/// Append one event to `events_file`, stamped `now`.
///
/// With `rotate` set (the archive retention in days), the age rotation and the
/// archive expiry run first, before this event is written. The log's newest
/// line is then a real earlier event, which anchors both cutoffs (see
/// [`rotate_log_by_age`]). Rotating after the append would let the event,
/// stamped by a clock that ran ahead, pull the cutoff past history the
/// retention should keep.
fn log_event_to(events_file: &std::path::Path, now: u64, rotate: Option<u32>, fields: &[(&str, &str)], extra: &[(&str, serde_json::Value)]) {
    if let Some(stats_dir) = events_file.parent() {
        let _ = std::fs::create_dir_all(stats_dir);
    }
    if let Some(archive_days) = rotate {
        // Age rotation and archive expiry (ADR-701 §2): one winner a day across processes.
        rotate_if_due(events_file, now, archive_days);
    }

    let mut obj = serde_json::Map::new();
    obj.insert("ts".to_string(), serde_json::Value::String(agent_fmt::when::utc_iso(now)));
    for (k, v) in fields {
        obj.insert(k.to_string(), serde_json::Value::String(v.to_string()));
    }
    for (k, v) in extra {
        obj.insert(k.to_string(), v.clone());
    }

    if let Ok(line) = serde_json::to_string(&serde_json::Value::Object(obj)) {
        append_jsonl_line(events_file, &line);
    }

    // Amortized cap: only when the log crosses MAX do we rewrite it to the most
    // recent KEEP bytes. A single stat() per append; the O(n) rewrite happens
    // once per ~8 MiB of growth. Readers always see a complete file: a reader
    // that opened the pre-compaction inode keeps reading it intact (the rename
    // is atomic and unlinks the old name only after), and each compaction
    // publishes a whole file via its own private temp. One compaction or
    // rotation runs at a time (a non-blocking lock; a loser skips), so the
    // dropped head is archived once.
    if let Ok(meta) = std::fs::metadata(events_file) {
        if meta.len() > MAX_EVENTS_BYTES {
            let _ = compact_locked(events_file, now, KEEP_EVENTS_BYTES, MIN_FREED_BYTES, CEILING_EVENTS_BYTES);
        }
    }
}

#[cfg(test)]
thread_local! {
    /// How long a test thread waits for a busy lock. A test binary runs many
    /// threads and some spawn processes; a child holds a copy of every
    /// descriptor open at the fork, and with it the flock, until it execs. A
    /// lock another thread released then reads busy for a moment, longer under
    /// load. The product never waits: one process rotates at a time and the
    /// loser skips. A test that holds the lock on purpose sets this to zero.
    static LOCK_PATIENCE: std::cell::Cell<std::time::Duration> = const { std::cell::Cell::new(std::time::Duration::from_secs(10)) };
}

/// How long to wait for a busy lock: never in the product, where one process
/// rotates at a time and the loser skips; see `LOCK_PATIENCE` for tests.
fn lock_patience() -> std::time::Duration {
    #[cfg(test)]
    return LOCK_PATIENCE.with(|p| p.get());
    #[cfg(not(test))]
    std::time::Duration::ZERO
}

/// Hold the log's sidecar lock, waiting [`lock_patience`] at most. `None` when
/// another process holds it. Where the filesystem cannot lock, the pass runs
/// unlocked.
fn try_log_lock(dir: &std::path::Path) -> Option<std::fs::File> {
    let f = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join(LOG_LOCK_NAME)).ok()?;
    let (started, patience) = (std::time::Instant::now(), lock_patience());
    loop {
        match f.try_lock() {
            Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < patience => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(std::fs::TryLockError::WouldBlock) => return None,
            _ => return Some(f),
        }
    }
}

/// [`compact_log_tail_with`] under the log lock. `None` when another process
/// is already rotating or compacting.
fn compact_locked(path: &std::path::Path, now: u64, keep_bytes: u64, min_freed: u64, ceiling: u64) -> Option<std::io::Result<()>> {
    let _lock = try_log_lock(path.parent()?)?;
    Some(compact_log_tail_with(path, now, keep_bytes, min_freed, ceiling))
}

#[cfg(test)]
fn compact_log_tail(path: &std::path::Path, now: u64, keep_bytes: u64, min_freed: u64) -> std::io::Result<()> {
    compact_log_tail_with(path, now, keep_bytes, min_freed, CEILING_EVENTS_BYTES)
}

/// Rewrite `path` in place to retain only its most recent `keep_bytes`, cut at a
/// line boundary so the first retained line is whole. The new contents are
/// written to a per-process, per-attempt temp, synced, then atomically renamed
/// over `path`, so a published `events.jsonl` is always a complete file.
/// `judge_call` lines in the dropped head are carried ahead of the tail (#750).
///
/// The rest of the dropped head is appended to the archive for the day of `now`
/// first, and durably; when that fails nothing is removed and today's failure
/// marker stops further attempts until tomorrow. Past `ceiling` bytes with the
/// archive still failing, the head is dropped unarchived and one
/// `event_log_dropped` event records it.
///
/// One handle serves the read and the carry: after the rename, whatever the old
/// file gained since is appended to the new one. Callers hold the log lock.
fn compact_log_tail_with(path: &std::path::Path, now: u64, keep_bytes: u64, min_freed: u64, ceiling: u64) -> std::io::Result<()> {
    compact_log_tail_hooked(path, now, keep_bytes, min_freed, ceiling, &mut || {})
}

/// [`compact_log_tail_with`] with `before_publish` run once the new contents
/// are built and before the file is checked and replaced, for tests that need
/// something to happen in that window.
fn compact_log_tail_hooked(path: &std::path::Path, now: u64, keep_bytes: u64, min_freed: u64, ceiling: u64, before_publish: &mut dyn FnMut()) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    // Size and marker first: a failing archive must not cost a read of the whole
    // file on every append.
    let size = std::fs::metadata(path)?.len();
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    if size <= keep_bytes || (size <= ceiling && archive_failed_today(dir, now)) {
        return Ok(()); // nothing to cut, or retry tomorrow
    }
    let f = std::fs::File::open(path)?;
    let mut data = Vec::new();
    (&f).read_to_end(&mut data)?;
    let keep = keep_bytes as usize;
    if data.len() <= keep {
        return Ok(());
    }
    let over_ceiling = data.len() as u64 > ceiling;
    // Start `keep` bytes from the end, then advance past the next newline so we
    // never retain a partial leading line.
    let cut = data.len() - keep;
    let start = match data[cut..].iter().position(|&b| b == b'\n') {
        Some(off) => cut + off + 1,
        None => data.len(), // single huge line / no boundary: drop it all
    };

    // The judge's spend history must outlive the cut (#750): `ways agent cost`
    // reads `judge_call` lines from this file, so the ones in the dropped head
    // are carried ahead of the kept tail. A call is ~300 bytes, so a year of
    // heavy use adds a few MiB. Carried lines sit in the head region on the
    // next compaction and are carried again, never duplicated.
    let mut out: Vec<u8> = Vec::new();
    let mut removed: Vec<u8> = Vec::new();
    for line in data[..start].split_inclusive(|&b| b == b'\n') {
        let dest = if is_judge_call(line) { &mut out } else { &mut removed };
        dest.extend_from_slice(line);
        if !line.ends_with(b"\n") {
            dest.push(b'\n');
        }
    }
    out.extend_from_slice(&data[start..]);
    if ((data.len() - out.len()) as u64) < min_freed {
        return Ok(()); // would not pay for the rewrite
    }
    before_publish();
    if !same_file(&f, path) {
        return Ok(());
    }
    // The archive first: a failure stands the pass down with the log intact,
    // unless the log has passed the ceiling.
    let mut unarchived = None;
    if let Err(e) = archive_removed(path, now, &removed) {
        mark_archive_failed(dir, now);
        if !over_ceiling {
            return Err(e);
        }
        unarchived = Some((removed.len(), e.to_string()));
    }
    agent_settings::writer::write_atomic(path, &out)?;
    // Appends that landed on the old file while the new one was written.
    (&f).seek(SeekFrom::Start(data.len() as u64))?;
    let mut gained = Vec::new();
    (&f).read_to_end(&mut gained)?;
    if !gained.is_empty() {
        std::fs::OpenOptions::new().append(true).open(path)?.write_all(&gained)?;
    }
    if let Some((bytes, reason)) = unarchived {
        let event = serde_json::json!({"ts": agent_fmt::when::utc_iso(now), "event": "event_log_dropped", "bytes": bytes, "reason": reason});
        append_jsonl_line(path, &event.to_string());
    }
    Ok(())
}

fn archive_failed_today(dir: &std::path::Path, now: u64) -> bool {
    dir.join(format!("{ARCHIVE_FAILED_PREFIX}{}", now / DAY_SECS)).exists()
}

/// Record that archiving failed today and clear markers of other days.
fn mark_archive_failed(dir: &std::path::Path, now: u64) {
    let mine = format!("{ARCHIVE_FAILED_PREFIX}{}", now / DAY_SECS);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(ARCHIVE_FAILED_PREFIX) && name != mine {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(dir.join(mine));
}

const DAY_SECS: u64 = 86_400;

/// Prefix of the daily claim files that sit beside the event log.
const ROTATE_LOCK_PREFIX: &str = "events.rotated-";

/// Whether this process has yet to check age rotation today. The first call
/// each day answers true, so the claim directory is scanned once per process
/// per day and not on every event.
fn rotation_due_today(now: u64) -> bool {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CHECKED_DAY: AtomicU64 = AtomicU64::new(0);
    let day = now / DAY_SECS + 1; // +1 so the initial 0 never matches
    CHECKED_DAY.swap(day, Ordering::Relaxed) != day
}

/// Append lines about to leave `path` to the dated archive beside it. Callers
/// remove nothing from the log unless this returns `Ok`.
fn archive_removed(path: &std::path::Path, now: u64, removed: &[u8]) -> std::io::Result<()> {
    match path.parent() {
        Some(dir) => ways_core::event_archive::append(dir, EVENTS, now, removed),
        None => Ok(()),
    }
}

/// Rotate the event log by age if no process has claimed today's slot.
///
/// The slot is a file named for the day, made with `create_new`, so exactly one
/// of any number of parallel hooks wins it. Claim files for earlier days are
/// removed, and so are claims for days still to come: those only exist after
/// the clock ran ahead and was set back, and would otherwise block rotation
/// until the calendar caught up. The slot's winner also deletes archives older
/// than `archive_days`. Returns whether lines left the live log.
fn rotate_if_due(path: &std::path::Path, now: u64, archive_days: u32) -> bool {
    let Some(dir) = path.parent() else { return false };
    let mine = format!("{ROTATE_LOCK_PREFIX}{}", now / DAY_SECS);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(ROTATE_LOCK_PREFIX) && name != mine {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    // The lock first, so a busy log does not spend today's slot.
    let Some(_lock) = try_log_lock(dir) else { return false };
    if std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&mine)).is_err() {
        return false; // claimed today already, here or by a parallel hook
    }
    // Expiry anchors to the log like the rotation cutoff: a clock that ran
    // ahead must not delete archives. With no readable line there is no anchor
    // and nothing expires.
    if let Some(anchor) = std::fs::File::open(path).ok().and_then(|f| newest_ts(&f).ok().flatten()) {
        ways_core::event_archive::expire(dir, EVENTS, anchor.min(now), archive_days);
    }
    rotate_log_by_age(path, now, LIVE_EVENT_DAYS).unwrap_or(false)
}

/// Move event lines older than `retention_days` out of `path` and into the
/// dated archive beside it (ADR-701 §2).
///
/// The cutoff is `min(now, newest line's ts) - retention`. Anchoring to the log
/// keeps a clock that ran ahead from aging out real history: the newest line is
/// an earlier real event, so the cutoff cannot pass it.
///
/// The log is append-ordered, so the old lines are a prefix. Only that prefix is
/// examined, and only the leading `{"ts":"..."` of each line is read; the scan
/// stops at the first line at or after the cutoff and the rest is copied
/// unchanged. A log with nothing old costs one line read. Every `judge_call` and
/// every line with an unreadable ts is kept (#750) and does not end the scan.
///
/// One handle serves the scan, the copy and the carry. Hooks append with
/// `O_APPEND` and take no lock, so after the survivors are published, whatever
/// the old file gained since is appended to the new one. Before publishing, the
/// handle's file is compared with the path's: a size compaction that replaced
/// the log meanwhile makes the rotation stand down. The expired lines are
/// archived, durably, before the log is replaced; an archive failure is an
/// error and the log is left as it was. Returns whether any line was moved.
fn rotate_log_by_age(path: &std::path::Path, now: u64, retention_days: u32) -> std::io::Result<bool> {
    rotate_log_by_age_hooked(path, now, retention_days, &mut || {})
}

/// [`rotate_log_by_age`] with `before_publish` run once the survivors are
/// collected and before the file is checked and replaced, for tests that need
/// something to happen in that window.
fn rotate_log_by_age_hooked(path: &std::path::Path, now: u64, retention_days: u32, before_publish: &mut dyn FnMut()) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let anchor = newest_ts(&f)?.map_or(now, |n| n.min(now));
    let cutoff = anchor.saturating_sub(u64::from(retention_days.max(1)) * DAY_SECS);
    let Some(split) = split_expired(&f, cutoff)? else { return Ok(false) };
    let (mut out, expired) = (split.kept, split.expired);
    // Everything from `current_from` on is current; copy it up to the handle's own length.
    let mut pos = split.current_from;
    loop {
        (&f).seek(SeekFrom::Start(pos))?;
        pos += (&f).read_to_end(&mut out)? as u64;
        if f.metadata()?.len() <= pos {
            break;
        }
    }
    before_publish();
    if !same_file(&f, path) {
        return Ok(false);
    }
    archive_removed(path, now, &expired)?;
    agent_settings::writer::write_atomic(path, &out)?;
    // Appends that landed on the old file while the new one was written.
    (&f).seek(SeekFrom::Start(pos))?;
    let mut gained = Vec::new();
    (&f).read_to_end(&mut gained)?;
    if !gained.is_empty() {
        std::fs::OpenOptions::new().append(true).open(path)?.write_all(&gained)?;
    }
    Ok(true)
}

/// Whether the open handle and `path` are the same file. Where the platform
/// gives no file identity this is assumed.
fn same_file(f: &std::fs::File, path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (f.metadata(), std::fs::metadata(path)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (f, path);
        true
    }
}

/// The ts of the log's newest line, read from its last 64 KiB.
fn newest_ts(f: &std::fs::File) -> std::io::Result<Option<u64>> {
    use std::io::{Read, Seek, SeekFrom};
    let len = f.metadata()?.len();
    let start = len.saturating_sub(64 * 1024);
    (&*f).seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    (&*f).read_to_end(&mut tail)?;
    Ok(tail.rsplit(|&b| b == b'\n').find_map(line_ts))
}

/// The old prefix of the log, sorted into what stays and what expires.
struct Split {
    /// Head lines that survive (judge calls, lines with no readable ts).
    kept: Vec<u8>,
    /// The lines that expire.
    expired: Vec<u8>,
    /// Byte offset of the first current line.
    current_from: u64,
}

/// Scan the old prefix of the log from the start. `None` when there is nothing to drop or the
/// scan must not drop (no line reaches `cutoff`).
fn split_expired(file: &std::fs::File, cutoff: u64) -> std::io::Result<Option<Split>> {
    use std::io::{BufRead, Seek, SeekFrom};
    (&*file).seek(SeekFrom::Start(0))?;
    let mut reader = std::io::BufReader::new(file);
    let (mut kept, mut expired) = (Vec::new(), Vec::new());
    let (mut pos, mut line) = (0u64, Vec::new());
    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(None); // no line reached the cutoff
        }
        // A judge_call is kept wherever it sits and never ends the scan: size
        // compaction carries recent ones to the head ahead of older lines.
        if is_judge_call(&line) {
            kept.extend_from_slice(&line);
            pos += n as u64;
            continue;
        }
        match line_ts(&line) {
            Some(ts) if ts >= cutoff => {
                return Ok((!expired.is_empty()).then_some(Split { kept, expired, current_from: pos }));
            }
            Some(_) => expired.extend_from_slice(&line),
            None => kept.extend_from_slice(&line),
        }
        pos += n as u64;
    }
}

/// The Unix second of an event line's leading `"ts":"..."`, read from its first
/// bytes without parsing the line.
fn line_ts(line: &[u8]) -> Option<u64> {
    let rest = line.strip_prefix(b"{\"ts\":\"")?;
    let end = rest.iter().position(|&b| b == b'"')?;
    agent_fmt::when::parse_utc_iso(std::str::from_utf8(&rest[..end]).ok()?)
}

/// A whole `judge_call` event line (substring prefilter, then a real parse).
fn is_judge_call(line: &[u8]) -> bool {
    const NEEDLE: &[u8] = b"judge_call";
    line.windows(NEEDLE.len()).any(|w| w == NEEDLE)
        && serde_json::from_slice::<serde_json::Value>(line).is_ok_and(|v| v.get("event").and_then(|e| e.as_str()) == Some("judge_call"))
}

#[cfg(test)]
mod compaction_tests {
    use super::*;

    // ── ADR-701 §2: rotation by age ────────────────────────────────

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    fn line(event: &str, age_days: u64, tag: &str) -> String {
        format!("{{\"ts\":\"{}\",\"event\":\"{event}\",\"tag\":\"{tag}\"}}\n", agent_fmt::when::utc_iso(NOW - age_days * DAY))
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ways-rot-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("events.jsonl")
    }

    #[test]
    fn rotation_drops_lines_older_than_the_retention_and_keeps_the_rest() {
        let p = tmp("age");
        let mut body = String::new();
        body.push_str(&line("way_fired", 120, "old"));
        body.push_str(&line("judge_call", 120, "old-judge"));
        body.push_str("not json at all\n");
        body.push_str(&line("way_fired", 89, "recent"));
        body.push_str(&line("way_fired", 0, "today"));
        std::fs::write(&p, &body).unwrap();

        assert!(rotate_log_by_age(&p, NOW, 90).unwrap());
        let got = std::fs::read_to_string(&p).unwrap();
        assert!(!got.contains("\"old\""), "a line past retention is dropped");
        assert!(got.contains("old-judge"), "judge_call history outlives rotation (#750)");
        assert!(got.contains("not json at all"), "a line with no readable ts is kept");
        assert!(got.contains("recent") && got.contains("today"));
    }

    #[test]
    fn rotation_never_touches_a_log_with_nothing_old() {
        let p = tmp("noop");
        let body = format!("{}{}", line("way_fired", 1, "a"), line("way_fired", 0, "b"));
        std::fs::write(&p, &body).unwrap();
        assert!(!rotate_log_by_age(&p, NOW, 90).unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), body, "the current session's events are never lost");
    }

    #[test]
    fn rotation_on_a_missing_log_is_a_noop() {
        let p = tmp("missing");
        assert!(!rotate_log_by_age(&p, NOW, 90).unwrap());
    }

    #[test]
    fn rotation_runs_once_a_day() {
        let p = tmp("throttle");
        let current = line("way_fired", 0, "now");
        std::fs::write(&p, format!("{}{current}", line("way_fired", 200, "old"))).unwrap();
        assert!(rotate_if_due(&p, NOW, 90));
        // An old line appears again: the day's slot is spent.
        std::fs::write(&p, format!("{}{current}", line("way_fired", 200, "old2"))).unwrap();
        assert!(!rotate_if_due(&p, NOW + 60, 90));
        assert!(std::fs::read_to_string(&p).unwrap().contains("old2"));
        // A day later it is due again, and yesterday's claim is gone.
        let later = NOW + DAY + 60;
        std::fs::write(&p, format!("{}{}", line("way_fired", 200, "old2"), line("way_fired", 0, "x"))).unwrap();
        assert!(rotate_if_due(&p, later, 90));
        assert!(!std::fs::read_to_string(&p).unwrap().contains("old2"));
        let claims = |d: &std::path::Path| std::fs::read_dir(d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(ROTATE_LOCK_PREFIX)).count();
        assert_eq!(claims(p.parent().unwrap()), 1);
    }

    #[test]
    fn only_one_of_many_parallel_claims_rotates() {
        let p = tmp("claims");
        std::fs::write(&p, format!("{}{}", line("way_fired", 200, "old"), line("way_fired", 0, "now"))).unwrap();
        let wins: usize = std::thread::scope(|s| {
            let hs: Vec<_> = (0..8).map(|_| s.spawn(|| rotate_if_due(&p, NOW, 90))).collect();
            hs.into_iter().map(|h| usize::from(h.join().unwrap())).sum()
        });
        assert_eq!(wins, 1);
    }

    /// A claim for a day that has not come (the clock ran ahead, then back)
    /// must not block rotation.
    #[test]
    fn a_claim_from_the_future_is_discarded() {
        let p = tmp("future");
        std::fs::write(&p, format!("{}{}", line("way_fired", 200, "old"), line("way_fired", 0, "now"))).unwrap();
        std::fs::write(p.parent().unwrap().join(format!("{ROTATE_LOCK_PREFIX}{}", NOW / DAY + 30)), "").unwrap();
        assert!(rotate_if_due(&p, NOW, 90));
        assert!(!std::fs::read_to_string(&p).unwrap().contains("old"));
        assert!(!p.parent().unwrap().join(format!("{ROTATE_LOCK_PREFIX}{}", NOW / DAY + 30)).exists());
    }

    /// When the clock jumped ahead, every line is "old". Refuse rather than
    /// empty the log.
    #[test]
    fn rotation_refuses_when_no_line_reaches_the_cutoff() {
        let p = tmp("ahead");
        let body = format!("{}{}", line("way_fired", 10, "a"), line("way_fired", 5, "b"));
        std::fs::write(&p, &body).unwrap();
        assert!(!rotate_log_by_age(&p, NOW + 400 * DAY, 90).unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), body);
    }

    /// The hook appends the current event, stamped by the same clock. A clock
    /// that ran ahead must not let that event pull the cutoff past real history.
    #[test]
    fn a_skewed_clock_cannot_age_out_real_history() {
        let p = tmp("skew");
        std::fs::write(&p, format!("{}{}", line("way_fired", 30, "real-30d"), line("way_fired", 0, "real-now"))).unwrap();
        // 80 days ahead, retention 90: the naive cutoff is NOW - 10 days.
        log_event_to(&p, NOW + 80 * DAY, Some(90), &[("event", "skewed")], &[]);
        let got = std::fs::read_to_string(&p).unwrap();
        assert!(got.contains("real-30d"), "history inside the retention survives a fast clock:\n{got}");
        assert!(got.contains("\"event\":\"skewed\""), "the event itself is still logged");
    }

    /// A skewed line already in the log, then the clock set right: the cutoff
    /// follows the clock, not the future line.
    #[test]
    fn a_future_line_does_not_pull_the_cutoff_forward() {
        let p = tmp("future-line");
        let future = format!("{{\"ts\":\"{}\",\"event\":\"way_fired\",\"tag\":\"future\"}}\n", agent_fmt::when::utc_iso(NOW + 80 * DAY));
        std::fs::write(&p, format!("{}{}{future}", line("way_fired", 120, "old"), line("way_fired", 30, "real-30d"))).unwrap();
        assert!(rotate_log_by_age(&p, NOW, 90).unwrap());
        let got = std::fs::read_to_string(&p).unwrap();
        assert!(!got.contains("\"old\"") && got.contains("real-30d") && got.contains("future"));
    }

    /// A recent judge_call carried to the head by size compaction is kept and
    /// does not end the scan.
    #[test]
    fn a_recent_judge_call_in_the_head_does_not_disable_rotation() {
        let p = tmp("judge-head");
        let body = format!("{}{}{}", line("judge_call", 10, "judge"), line("way_fired", 200, "old"), line("way_fired", 0, "now"));
        std::fs::write(&p, body).unwrap();
        assert!(rotate_log_by_age(&p, NOW, 90).unwrap());
        let got = std::fs::read_to_string(&p).unwrap();
        assert!(got.contains("judge") && got.contains("now") && !got.contains("\"old\""));
    }

    /// Events appended while the survivors are written land in the new log.
    #[test]
    fn events_appended_during_the_write_are_carried_over() {
        let p = tmp("carry");
        std::fs::write(&p, format!("{}{}", line("way_fired", 200, "old"), line("way_fired", 0, "now"))).unwrap();
        let during = line("way_fired", 0, "during");
        let appender = p.clone();
        let mut hook = move || append_jsonl_line(&appender, during.trim_end());
        assert!(rotate_log_by_age_hooked(&p, NOW, 90, &mut hook).unwrap());
        let got = std::fs::read_to_string(&p).unwrap();
        assert!(got.contains("during") && got.contains("\"now\"") && !got.contains("\"old\""), "{got}");
    }

    /// A size compaction that replaced the file meanwhile makes the rotation
    /// stand down rather than overwrite it.
    #[test]
    fn rotation_stands_down_when_the_file_was_replaced_meanwhile() {
        let p = tmp("replaced");
        std::fs::write(&p, format!("{}{}", line("way_fired", 200, "old"), line("way_fired", 0, "now"))).unwrap();
        let replacement = line("way_fired", 0, "compacted");
        let target = p.clone();
        let mut hook = move || {
            let tmp = target.with_extension("swap");
            std::fs::write(&tmp, &replacement).unwrap();
            std::fs::rename(&tmp, &target).unwrap();
        };
        assert!(!rotate_log_by_age_hooked(&p, NOW, 90, &mut hook).unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), line("way_fired", 0, "compacted"));
    }

    #[test]
    fn rotation_stops_at_the_first_current_line_and_copies_the_rest_unchanged() {
        let p = tmp("prefix");
        // A later line with an old ts (clock skew) sits in the copied region and stays.
        let body = format!("{}{}{}{}", line("way_fired", 200, "old"), line("way_fired", 1, "cur"), line("way_fired", 300, "skewed"), line("way_fired", 0, "z"));
        std::fs::write(&p, body).unwrap();
        assert!(rotate_log_by_age(&p, NOW, 90).unwrap());
        let got = std::fs::read_to_string(&p).unwrap();
        assert!(!got.contains("\"old\"") && got.contains("cur") && got.contains("skewed") && got.contains("\"z\""));
    }

    #[test]
    fn compact_log_tail_keeps_recent_whole_lines() {
        // A unique temp path (process id; no clock/random in this crate's tests).
        let path = std::env::temp_dir().join(format!("ways-evt-{}.jsonl", std::process::id()));
        // 1000 numbered JSON lines (~20 bytes each → ~20 KB).
        let mut content = String::new();
        for i in 0..1000 {
            content.push_str(&format!("{{\"n\":{i}}}\n"));
        }
        std::fs::write(&path, &content).unwrap();
        let before = std::fs::metadata(&path).unwrap().len();

        // Keep ~2 KB → far below the file size, so it must compact.
        compact_log_tail(&path, NOW, 2000, 0).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        let after_len = after.len() as u64;
        assert!(after_len < before, "file should shrink");
        assert!(after_len <= 2000 + 32, "retained ~keep_bytes (+ one boundary line)");

        let lines: Vec<&str> = after.lines().collect();
        // Every retained line is whole, parseable JSON (no partial leading line).
        for l in &lines {
            serde_json::from_str::<serde_json::Value>(l).expect("retained line is valid JSON");
        }
        // The most recent line is preserved; the oldest are dropped.
        assert_eq!(*lines.last().unwrap(), "{\"n\":999}");
        assert!(!after.contains("{\"n\":0}"), "oldest events dropped");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compaction_keeps_judge_calls_so_spend_still_counts_them() {
        let path = std::env::temp_dir().join(format!("ways-evt-judge-{}.jsonl", std::process::id()));
        let judge = |ts: &str| format!("{{\"event\":\"judge_call\",\"ts\":\"{ts}\",\"session\":\"s\",\"project\":\"/p\",\"input_tokens\":\"10\",\"output_tokens\":\"1\",\"cost_usd\":\"0.0100\",\"cost_source\":\"provider\"}}\n");
        let filler = |s: &mut String| {
            for i in 0..500 {
                s.push_str(&format!("{{\"event\":\"way_fired\",\"n\":\"{i}\"}}\n"));
            }
        };
        // Two old judge calls buried in filler that the cut drops.
        let mut content = judge("2026-01-05T10:00:00Z");
        filler(&mut content);
        content.push_str(&judge("2026-01-06T10:00:00Z"));
        filler(&mut content);
        content.push_str(&judge("2026-10-01T10:00:00Z"));
        std::fs::write(&path, &content).unwrap();

        compact_log_tail(&path, NOW, 2000, 0).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.len() < content.len(), "filler is cut");

        let calls = ways_agent_core::spend::parse_log(&after);
        assert_eq!(calls.len(), 3, "all three judge calls survive");
        assert_eq!(ways_agent_core::spend::covers_since(&calls).as_deref(), Some("2026-01-05T10:00:00Z"));

        // A second compaction carries them again without duplicating.
        compact_log_tail(&path, NOW, 2000, 0).unwrap();
        assert_eq!(ways_agent_core::spend::parse_log(&std::fs::read_to_string(&path).unwrap()).len(), 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compaction_skips_a_rewrite_that_frees_too_little_and_keeps_judge_calls() {
        let path = std::env::temp_dir().join(format!("ways-evt-gap-{}.jsonl", std::process::id()));
        let judge = |i: usize| format!("{{\"event\":\"judge_call\",\"ts\":\"2026-01-05T10:00:{:02}Z\",\"cost_source\":\"unknown\"}}\n", i % 60);
        // Judge history (~4 KB) larger than the gap (keep 500, min_freed 1000).
        let mut content: String = (0..50).map(judge).collect();
        std::fs::write(&path, &content).unwrap();
        let (keep, min_freed) = (500, 1000);

        // Appending filler: the first compactions free < min_freed, so the file is left as is.
        for i in 0..10 {
            content.push_str(&format!("{{\"event\":\"way_fired\",\"n\":\"{i}\"}}\n"));
            std::fs::write(&path, &content).unwrap();
            let before = std::fs::read(&path).unwrap();
            compact_log_tail(&path, NOW, keep, min_freed).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before, "no rewrite while it frees too little");
        }

        // Enough filler to free more than min_freed: it compacts, and judge calls survive.
        for i in 0..100 {
            content.push_str(&format!("{{\"event\":\"way_fired\",\"n\":\"{i}\"}}\n"));
        }
        std::fs::write(&path, &content).unwrap();
        compact_log_tail(&path, NOW, keep, min_freed).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.len() < content.len(), "compacted once it pays");
        assert_eq!(ways_agent_core::spend::parse_log(&after).len(), 50, "every judge call kept");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compact_log_tail_noop_when_under_keep() {
        let path = std::env::temp_dir().join(format!("ways-evt-small-{}.jsonl", std::process::id()));
        let content = "{\"n\":1}\n{\"n\":2}\n";
        std::fs::write(&path, content).unwrap();
        compact_log_tail(&path, NOW, 1024 * 1024, 0).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compact_log_tail_drops_oversized_unbroken_blob() {
        // A single line larger than keep_bytes with no newline has no boundary
        // to cut at — the intended behavior is to drop it (it is already corrupt
        // for a line-oriented log), leaving an empty file rather than a partial.
        let path = std::env::temp_dir().join(format!("ways-evt-blob-{}.jsonl", std::process::id()));
        std::fs::write(&path, "x".repeat(5000)).unwrap();
        compact_log_tail(&path, NOW, 1000, 0).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compact_log_tail_is_idempotent() {
        // After one compaction the file is under keep, so a second is a no-op.
        let path = std::env::temp_dir().join(format!("ways-evt-idem-{}.jsonl", std::process::id()));
        let mut content = String::new();
        for i in 0..500 {
            content.push_str(&format!("{{\"n\":{i}}}\n"));
        }
        std::fs::write(&path, &content).unwrap();
        compact_log_tail(&path, NOW, 1500, 0).unwrap();
        let once = std::fs::read_to_string(&path).unwrap();
        compact_log_tail(&path, NOW, 1500, 0).unwrap();
        let twice = std::fs::read_to_string(&path).unwrap();
        assert_eq!(once, twice, "second compaction is a no-op");
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use ways_core::event_archive::{archive_path, archives, read_source};

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    fn line(event: &str, age_days: u64, tag: &str) -> String {
        format!("{{\"ts\":\"{}\",\"event\":\"{event}\",\"tag\":\"{tag}\"}}\n", agent_fmt::when::utc_iso(NOW - age_days * DAY))
    }

    fn state(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("ways-arch-sess-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        (d.clone(), d.join("events.jsonl"))
    }

    #[test]
    fn rotation_archives_the_expired_lines_and_the_live_file_loses_them() {
        let (dir, log) = state("rot");
        std::fs::write(&log, format!("{}{}{}", line("way_fired", 200, "old1"), line("way_fired", 150, "old2"), line("way_fired", 0, "now"))).unwrap();
        assert!(rotate_log_by_age(&log, NOW, 90).unwrap());
        let archived = read_source(&archive_path(&dir, EVENTS, NOW)).unwrap();
        assert_eq!(archived, format!("{}{}", line("way_fired", 200, "old1"), line("way_fired", 150, "old2")), "oldest first, byte for byte");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), line("way_fired", 0, "now"));
    }

    #[test]
    fn a_failed_archive_write_makes_rotation_stand_down_and_remove_nothing() {
        let (dir, log) = state("rot-fail");
        let body = format!("{}{}", line("way_fired", 200, "old"), line("way_fired", 0, "now"));
        std::fs::write(&log, &body).unwrap();
        std::fs::create_dir(archive_path(&dir, EVENTS, NOW)).unwrap(); // the archive cannot be opened
        assert!(rotate_log_by_age(&log, NOW, 90).is_err());
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body, "the live file is untouched");
    }

    #[test]
    fn the_size_cap_archives_its_dropped_head_and_keeps_judge_calls_live() {
        let (dir, log) = state("cap");
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&line("way_fired", 5, &format!("n{i:02}")));
            if i == 3 {
                body.push_str(&line("judge_call", 5, "judge"));
            }
        }
        std::fs::write(&log, &body).unwrap();
        compact_log_tail(&log, NOW, 1500, 0).unwrap();
        let live = std::fs::read_to_string(&log).unwrap();
        let archived = read_source(&archive_path(&dir, EVENTS, NOW)).unwrap();
        assert!(live.contains("judge") && !archived.contains("judge"), "judge_call stays in the live file");
        assert!(live.contains("n49") && !live.contains("n00"), "the live file keeps the tail");
        assert!(archived.starts_with(&line("way_fired", 5, "n00")), "the head went to the archive:\n{archived}");
        // Archived head then live tail is the original order, with nothing lost or doubled.
        let rejoined: Vec<&str> = archived.lines().chain(live.lines().filter(|l| !l.contains("judge"))).collect();
        let original: Vec<&str> = body.lines().filter(|l| !l.contains("judge")).collect();
        assert_eq!(rejoined, original);
    }

    #[test]
    fn a_failed_archive_write_makes_the_size_cap_stand_down() {
        let (dir, log) = state("cap-fail");
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&line("way_fired", 5, &format!("n{i:02}")));
        }
        std::fs::write(&log, &body).unwrap();
        std::fs::create_dir(archive_path(&dir, EVENTS, NOW)).unwrap();
        assert!(compact_log_tail(&log, NOW, 1500, 0).is_err());
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body, "nothing was removed");
    }

    #[test]
    fn the_daily_pass_expires_archives_past_the_retention_and_never_the_live_log() {
        let (dir, log) = state("expire");
        std::fs::write(&log, line("way_fired", 0, "now")).unwrap();
        ways_core::event_archive::append(&dir, EVENTS, NOW - 400 * DAY, b"ancient\n").unwrap();
        ways_core::event_archive::append(&dir, EVENTS, NOW - 30 * DAY, b"recent\n").unwrap();
        rotate_if_due(&log, NOW, 365);
        assert_eq!(archives(&dir, EVENTS), [archive_path(&dir, EVENTS, NOW - 30 * DAY)]);
        assert!(log.exists());
        // A one-day retention still leaves the live log alone.
        rotate_if_due(&log, NOW + DAY, 1);
        assert!(archives(&dir, EVENTS).is_empty() && log.exists());
    }

    #[test]
    fn a_clock_jump_does_not_expire_archives() {
        let (dir, log) = state("clock-jump");
        std::fs::write(&log, line("way_fired", 0, "now")).unwrap();
        ways_core::event_archive::append(&dir, EVENTS, NOW - 30 * DAY, b"recent\n").unwrap();
        rotate_if_due(&log, NOW + 400 * DAY, 365);
        assert_eq!(archives(&dir, EVENTS), [archive_path(&dir, EVENTS, NOW - 30 * DAY)], "the cutoff follows the log, not a clock that ran ahead");
    }

    fn numbered(n: usize) -> String {
        (0..n).map(|i| line("way_fired", 5, &format!("n{i:04}"))).collect()
    }

    #[test]
    fn two_concurrent_compactions_archive_the_head_once() {
        let (dir, log) = state("concurrent");
        let body = numbered(3000);
        std::fs::write(&log, &body).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let runs: Vec<_> = (0..2)
            .map(|_| {
                let (log, barrier) = (log.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    compact_locked(&log, NOW, 20_000, 0, CEILING_EVENTS_BYTES)
                })
            })
            .collect();
        for r in runs {
            let _ = r.join().unwrap();
        }
        let archived = read_source(&archive_path(&dir, EVENTS, NOW)).unwrap();
        let live = std::fs::read_to_string(&log).unwrap();
        let rejoined: Vec<&str> = archived.lines().chain(live.lines()).collect();
        assert_eq!(rejoined, body.lines().collect::<Vec<_>>(), "each line is in exactly one place, once");
    }

    #[test]
    fn a_pass_skips_while_another_holds_the_log_lock() {
        let (dir, log) = state("locked");
        std::fs::write(&log, numbered(100)).unwrap();
        let held = try_log_lock(&dir).expect("first holder");
        LOCK_PATIENCE.with(|p| p.set(std::time::Duration::ZERO));
        assert!(compact_locked(&log, NOW, 500, 0, CEILING_EVENTS_BYTES).is_none());
        assert!(!rotate_if_due(&log, NOW, 365));
        LOCK_PATIENCE.with(|p| p.set(std::time::Duration::from_secs(10)));
        drop(held);
        assert!(compact_locked(&log, NOW, 500, 0, CEILING_EVENTS_BYTES).is_some());
    }

    #[test]
    fn a_failing_archive_is_retried_once_a_day_not_per_append() {
        let (dir, log) = state("retry");
        let body = numbered(50);
        std::fs::write(&log, &body).unwrap();
        std::fs::create_dir(archive_path(&dir, EVENTS, NOW)).unwrap();
        assert!(compact_log_tail(&log, NOW, 1500, 0).is_err());
        // Today's marker is down: the next attempt does not even try.
        std::fs::remove_dir(archive_path(&dir, EVENTS, NOW)).unwrap();
        assert!(compact_log_tail(&log, NOW, 1500, 0).is_ok());
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body, "no attempt was made");
        // Tomorrow it is tried again, and succeeds.
        assert!(compact_log_tail(&log, NOW + DAY, 1500, 0).is_ok());
        assert!(std::fs::read_to_string(&log).unwrap().len() < body.len());
        assert!(archive_failed_today(&dir, NOW) && !archive_failed_today(&dir, NOW + DAY));
    }

    #[test]
    fn past_the_ceiling_with_archiving_failing_the_cap_drops_the_head_and_says_so() {
        let (dir, log) = state("ceiling");
        std::fs::write(&log, numbered(50)).unwrap();
        std::fs::create_dir(archive_path(&dir, EVENTS, NOW)).unwrap();
        compact_log_tail_with(&log, NOW, 1500, 0, 2000).unwrap();
        let live = std::fs::read_to_string(&log).unwrap();
        assert!(!live.contains("n0000") && live.contains("n0049"), "the head went");
        let dropped = live.lines().find(|l| l.contains("event_log_dropped")).expect("one event_log_dropped line");
        let v: serde_json::Value = serde_json::from_str(dropped).unwrap();
        assert!(v["bytes"].as_u64().unwrap() > 0 && v["reason"].as_str().is_some_and(|r| !r.is_empty()), "{dropped}");
        assert_eq!(live.matches("event_log_dropped").count(), 1);
    }

    #[test]
    fn under_the_ceiling_a_failing_archive_still_preserves_everything() {
        let (dir, log) = state("under-ceiling");
        let body = numbered(50);
        std::fs::write(&log, &body).unwrap();
        std::fs::create_dir(archive_path(&dir, EVENTS, NOW)).unwrap();
        assert!(compact_log_tail_with(&log, NOW, 1500, 0, 10_000_000).is_err());
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body);
    }

    #[test]
    fn events_appended_during_a_compaction_are_carried_over() {
        let (_dir, log) = state("cap-carry");
        std::fs::write(&log, numbered(100)).unwrap();
        let late = line("way_fired", 0, "late");
        let target = log.clone();
        let mut hook = move || append_jsonl_line(&target, late.trim_end());
        compact_log_tail_hooked(&log, NOW, 500, 0, CEILING_EVENTS_BYTES, &mut hook).unwrap();
        let live = std::fs::read_to_string(&log).unwrap();
        assert!(live.contains("late") && live.contains("n0099") && !live.contains("n0000"), "{live}");
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_archive_costs_no_read_of_the_log_once_marked() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, log) = state("no-read");
        std::fs::write(&log, numbered(50)).unwrap();
        mark_archive_failed(&dir, NOW);
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&log).is_ok() {
            return; // running as a user the mode does not bind
        }
        let r = compact_log_tail(&log, NOW, 1500, 0);
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(r.is_ok(), "the marker and the size decide before the file is opened: {r:?}");
    }

    #[test]
    fn a_busy_lock_does_not_spend_the_days_rotation_slot() {
        let (dir, log) = state("slot");
        std::fs::write(&log, format!("{}{}", line("way_fired", 200, "old"), line("way_fired", 0, "now"))).unwrap();
        let held = try_log_lock(&dir).unwrap();
        LOCK_PATIENCE.with(|p| p.set(std::time::Duration::ZERO));
        assert!(!rotate_if_due(&log, NOW, 365));
        LOCK_PATIENCE.with(|p| p.set(std::time::Duration::from_secs(10)));
        let claimed = |d: &std::path::Path| std::fs::read_dir(d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(ROTATE_LOCK_PREFIX));
        assert!(!claimed(&dir), "the slot is still open");
        drop(held);
        assert!(rotate_if_due(&log, NOW, 365), "and the day's rotation runs when the lock frees");
    }

    /// A clock left ahead is logged, so the next day's anchor is ahead too.
    /// Expiry then bites, but only a file or two per daily pass.
    #[test]
    fn a_clock_left_ahead_costs_at_most_two_archives_a_day() {
        let (dir, log) = state("ahead");
        std::fs::write(&log, line("way_fired", 0, "now")).unwrap();
        for age in [30u64, 20, 10, 5, 2] {
            ways_core::event_archive::append(&dir, EVENTS, NOW - age * DAY, b"x\n").unwrap();
        }
        log_event_to(&log, NOW + 400 * DAY, Some(365), &[("event", "ahead")], &[]);
        // The rotation itself archives the real line the jump aged out; count the planted ones.
        let planted = || archives(&dir, EVENTS).into_iter().filter(|a| *a < archive_path(&dir, EVENTS, NOW + DAY)).count();
        assert_eq!(planted(), 5, "day one: the anchor is a real line");
        rotate_if_due(&log, NOW + 401 * DAY, 365);
        assert_eq!(planted(), 3, "day two: the oldest two at most");
        rotate_if_due(&log, NOW + 402 * DAY, 365);
        assert_eq!(planted(), 1);
    }

    /// The flake's mechanism: another test thread spawns a process while this
    /// one holds the lock. The child's copy of the descriptor keeps the flock
    /// held from the fork until it execs, so a lock this thread already
    /// released reads busy for a moment. Held here by a thread for 100 ms.
    #[test]
    fn the_lock_waits_out_a_descriptor_a_forked_child_still_holds() {
        let (dir, log) = state("inherited");
        std::fs::write(&log, format!("{}{}", line("way_fired", 200, "old"), line("way_fired", 0, "now"))).unwrap();
        let inherited = try_log_lock(&dir).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(inherited);
        });
        assert!(rotate_if_due(&log, NOW, 365), "the pass ran once the descriptor was released");
        release.join().unwrap();
    }
}
