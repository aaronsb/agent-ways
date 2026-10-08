//! Dated gzip archives of the agent-ways log streams (ADR-701 §2).
//!
//! History is archived, not deleted: before the size cap or the age rotation
//! removes lines from a stream's live file (`events.jsonl`, `decisions.jsonl`),
//! the removed bytes land in `<stem>-YYYY-MM-DD.jsonl.gz` beside it, named for
//! the UTC day of the removal. The writer lives in `ways`; the naming, the
//! reading and the expiry live here so every reader of a stream sees the same
//! set of files.
//!
//! An archive is a series of gzip members, one per removal. Concatenated
//! members are one valid gzip stream, so a day's second removal appends a new
//! member and never rewrites the first.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const DAY_SECS: u64 = 86_400;
const SUFFIX: &str = ".jsonl.gz";

/// One append-only JSONL log in the state directory, with its own live file
/// and its own dated archives. The stem names both, and the writer derives its
/// lock and claim files from it, so two streams never share any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// Hook telemetry: fires, suppressions, scores (`events.jsonl`).
    Events,
    /// Per-turn decision records (`decisions.jsonl`, ADR-701 §2).
    Decisions,
}

/// The event stream.
pub const EVENTS: Stream = Stream::Events;
/// The decision stream.
pub const DECISIONS: Stream = Stream::Decisions;

impl Stream {
    /// The name every file of this stream starts with.
    pub const fn stem(self) -> &'static str {
        match self {
            Stream::Events => "events",
            Stream::Decisions => "decisions",
        }
    }

    /// The live file's name, `<stem>.jsonl`.
    pub fn live_name(self) -> String {
        format!("{}.jsonl", self.stem())
    }
}

/// The archive of `stream` that takes lines removed at Unix second `now`.
pub fn archive_path(dir: &Path, stream: Stream, now: u64) -> PathBuf {
    dir.join(format!("{}-{}{SUFFIX}", stream.stem(), agent_fmt::when::utc_date(now)))
}

/// The Unix second at the start of the day a `stream` archive's file name
/// carries, or `None` for any other name: another stream's archives, and the
/// hand-made `events-preserved-*.jsonl.gz` too.
fn archive_day(stream: Stream, name: &str) -> Option<u64> {
    let date = name.strip_prefix(stream.stem())?.strip_prefix('-')?.strip_suffix(SUFFIX)?;
    let b = date.as_bytes();
    let shaped = b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() });
    if !shaped {
        return None;
    }
    agent_fmt::when::parse_utc_iso(&format!("{date}T00:00:00Z"))
}

/// Every archive of `stream` in `dir`, oldest day first.
pub fn archives(dir: &Path, stream: Stream) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let day = archive_day(stream, &e.file_name().to_string_lossy())?;
            Some((day, e.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

/// Append `removed` to the `stream` archive for the day of `now`, as one new
/// gzip member, and make it durable. Called before the live file is rewritten:
/// when this fails the caller removes nothing.
///
/// The archive is opened and locked exclusively before anything is compressed,
/// so a failing archive costs little. The lock is held across the length read,
/// the write, the fsync and any truncation, so a failed write can only cut back
/// its own bytes and never another process's durable member. The file is
/// never removed: an empty archive is harmless, and unlinking could strand a
/// process waiting on its lock.
/// The directory is synced after every write. An empty `removed` writes nothing.
pub fn append(dir: &Path, stream: Stream, now: u64, removed: &[u8]) -> std::io::Result<()> {
    if removed.is_empty() {
        return Ok(());
    }
    append_from(dir, stream, now, &mut &removed[..])
}

/// [`append`] reading the removed bytes from `removed` as they are
/// compressed, so a large removal costs buffers and not its size in memory.
/// The member is written as it is encoded; on any failure, including one
/// reading `removed`, the archive is cut back to its length before the call.
pub fn append_from(dir: &Path, stream: Stream, now: u64, removed: &mut dyn Read) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = archive_path(dir, stream, now);
    // Read and write, not append: on Windows an append-only handle lacks
    // write-data access, so `LockFileEx` and the cut-back's `set_len` both fail
    // with ERROR_ACCESS_DENIED. Every writer holds the exclusive lock, so
    // writing at the end under it appends as `O_APPEND` would.
    let f = open_live(std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false), &path)?;
    f.lock()?;

    let written = (|| {
        let before = f.metadata()?.len();
        (&f).seek(SeekFrom::Start(before))?;
        let result = (|| {
            let mut enc = flate2::write::GzEncoder::new(std::io::BufWriter::with_capacity(64 * 1024, &f), flate2::Compression::default());
            std::io::copy(removed, &mut enc)?;
            enc.finish()?.flush()?;
            f.sync_all()
        })();
        if result.is_err() {
            // Cut back our own bytes, under the lock.
            let _ = f.set_len(before);
        }
        result
    })();
    drop(f); // releases the lock
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    written
}

/// The most archives one [`expire`] pass deletes. Normal operation writes at
/// most one day-file a day, so a backlog drains over a few passes, while a clock
/// left ahead (which drags the log's anchor forward with it) costs a couple of
/// files per real day and not the whole history.
pub const MAX_EXPIRED_PER_PASS: usize = 2;

/// Delete the oldest `stream` archives whose day is more than `retention_days`
/// before `now`, at most [`MAX_EXPIRED_PER_PASS`]. Names that are not this
/// stream's archives, and the live files, are never touched. Returns how many
/// files were removed.
pub fn expire(dir: &Path, stream: Stream, now: u64, retention_days: u32) -> usize {
    let cutoff = (now / DAY_SECS * DAY_SECS).saturating_sub(u64::from(retention_days.max(1)) * DAY_SECS);
    let mut removed = 0;
    for path in archives(dir, stream) {
        if removed == MAX_EXPIRED_PER_PASS || !path.file_name().and_then(|n| archive_day(stream, &n.to_string_lossy())).is_some_and(|day| day < cutoff) {
            break; // oldest first: nothing after a surviving file is past the cutoff
        }
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Open a stream's file with `opts` so that it can still be replaced or
/// removed while the handle is open.
///
/// The writer's compaction and daily pass rename a new live file over the old
/// one, and expiry deletes archives, while hooks append and readers such as
/// `ways tune stats` may hold the file open. Unix allows that. Windows refuses a
/// rename over, or a delete of, a file while any handle on it was opened
/// without `FILE_SHARE_DELETE`, which std's default share mode leaves out, so
/// every reader and writer of a stream's files opens them here.
pub fn open_live(opts: &mut std::fs::OpenOptions, path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        const FILE_SHARE_WRITE: u32 = 0x2;
        const FILE_SHARE_DELETE: u32 = 0x4;
        opts.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    opts.open(path)
}

/// [`open_live`] for reading.
pub fn open_live_read(path: &Path) -> std::io::Result<std::fs::File> {
    open_live(std::fs::OpenOptions::new().read(true), path)
}

/// The text of one log source: a plain file as it is, a `.gz` archive
/// decompressed. Unreadable files give `None`. The bytes become the string
/// without a copy when they are valid UTF-8. The file is opened with
/// [`open_live_read`], so a reader never blocks the writer's rename.
///
/// A member that fails to decode (a torn write) gives up the lines it held
/// before the damage, and decoding resumes at the next gzip header, so one bad
/// member does not hide the members after it.
pub fn read_source(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    open_live_read(path).and_then(|mut f| f.read_to_end(&mut bytes)).ok()?;
    let out = if path.extension().is_some_and(|e| e == "gz") { decode_members(&bytes) } else { bytes };
    Some(String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

const GZIP_HEADER: [u8; 3] = [0x1f, 0x8b, 0x08];

fn decode_members(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let mut rest = &bytes[pos..];
        let mut member = Vec::new();
        let decoded = flate2::bufread::GzDecoder::new(&mut rest).read_to_end(&mut member);
        let consumed = bytes.len() - pos - rest.len();
        if decoded.is_ok() && consumed > 0 {
            out.extend_from_slice(&member);
            pos += consumed;
            continue;
        }
        // Keep whole lines from the damaged member, then resume at the next header.
        if let Some(nl) = member.iter().rposition(|&b| b == b'\n') {
            out.extend_from_slice(&member[..=nl]);
        }
        match bytes[pos + 1..].windows(GZIP_HEADER.len()).position(|w| w == GZIP_HEADER) {
            Some(off) => pos += 1 + off,
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000; // 2027-01-15

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ways-arch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_archive_is_named_for_the_utc_day() {
        let p = archive_path(Path::new("/s"), EVENTS, NOW);
        assert_eq!(p, Path::new("/s/events-2027-01-15.jsonl.gz"));
    }

    #[test]
    fn each_stream_names_its_archive_by_its_own_stem() {
        assert_eq!(archive_path(Path::new("/s"), EVENTS, NOW), Path::new("/s/events-2027-01-15.jsonl.gz"));
        assert_eq!(archive_path(Path::new("/s"), DECISIONS, NOW), Path::new("/s/decisions-2027-01-15.jsonl.gz"));
        assert_eq!(EVENTS.live_name(), "events.jsonl");
        assert_eq!(DECISIONS.live_name(), "decisions.jsonl");
    }

    #[test]
    fn a_streams_archives_exclude_the_other_streams_files() {
        let d = dir("streams");
        append(&d, EVENTS, NOW, b"e\n").unwrap();
        append(&d, DECISIONS, NOW - DAY_SECS, b"d\n").unwrap();
        std::fs::write(d.join("decisions.jsonl"), b"live\n").unwrap();
        let names = |s| archives(&d, s).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(EVENTS), ["events-2027-01-15.jsonl.gz"]);
        assert_eq!(names(DECISIONS), ["decisions-2027-01-14.jsonl.gz"]);
        // Expiring one stream leaves the other's history alone.
        assert_eq!(expire(&d, DECISIONS, NOW + 30 * DAY_SECS, 10), 1);
        assert_eq!(names(EVENTS), ["events-2027-01-15.jsonl.gz"]);
    }

    #[test]
    fn two_removals_on_one_day_append_members_and_read_back_in_order() {
        let d = dir("append");
        append(&d, EVENTS, NOW, b"one\ntwo\n").unwrap();
        append(&d, EVENTS, NOW + 60, b"three\n").unwrap();
        assert_eq!(archives(&d, EVENTS).len(), 1);
        assert_eq!(read_source(&archive_path(&d, EVENTS, NOW)).unwrap(), "one\ntwo\nthree\n");
    }

    #[test]
    fn archives_list_oldest_day_first_and_skip_other_names() {
        let d = dir("order");
        append(&d, EVENTS, NOW, b"b\n").unwrap();
        append(&d, EVENTS, NOW - 3 * DAY_SECS, b"a\n").unwrap();
        std::fs::write(d.join("events-preserved-20261005.jsonl.gz"), b"x").unwrap();
        // Dates the parser would accept but the archive writer never makes.
        std::fs::write(d.join("events-2026-1-5.jsonl.gz"), b"x").unwrap();
        std::fs::write(d.join("events-+2026-10-05.jsonl.gz"), b"x").unwrap();
        std::fs::write(d.join("events.jsonl"), b"live\n").unwrap();
        let names: Vec<String> = archives(&d, EVENTS).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["events-2027-01-12.jsonl.gz", "events-2027-01-15.jsonl.gz"]);
    }

    #[test]
    fn expiry_deletes_only_archives_past_the_cutoff() {
        let d = dir("expire");
        for age in [0u64, 9, 10, 11, 40] {
            append(&d, EVENTS, NOW - age * DAY_SECS, b"x\n").unwrap();
        }
        std::fs::write(d.join("events.jsonl"), b"live\n").unwrap();
        std::fs::write(d.join("events-preserved-20200101.jsonl.gz"), b"x").unwrap();
        assert_eq!(expire(&d, EVENTS, NOW, 10), 2, "days 11 and 40 are past a 10-day retention");
        let left: Vec<String> = archives(&d, EVENTS).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(left, ["events-2027-01-05.jsonl.gz", "events-2027-01-06.jsonl.gz", "events-2027-01-15.jsonl.gz"]);
        assert!(d.join("events.jsonl").exists(), "the live log is never expired");
        assert!(d.join("events-preserved-20200101.jsonl.gz").exists(), "a name outside the pattern is left alone");
    }

    #[test]
    fn a_failed_archive_write_is_an_error_and_leaves_nothing_else_behind() {
        let d = dir("fail");
        // A directory squats on the archive's name, so it cannot be opened.
        std::fs::create_dir(archive_path(&d, EVENTS, NOW)).unwrap();
        assert!(append(&d, EVENTS, NOW, b"x\n").is_err());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "only the squatting directory is there");
    }

    /// A source that fails partway leaves the archive as it was: the bytes
    /// already encoded from it are cut back under the lock.
    #[test]
    fn a_source_failing_midway_leaves_the_archive_as_it_was() {
        struct Failing(usize);
        impl Read for Failing {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0 == 0 {
                    return Err(std::io::Error::other("source failed"));
                }
                let n = buf.len().min(self.0).min(4096);
                buf[..n].fill(b'x');
                self.0 -= n;
                Ok(n)
            }
        }
        let d = dir("source-fail");
        append(&d, EVENTS, NOW, b"kept\n").unwrap();
        let before = std::fs::read(archive_path(&d, EVENTS, NOW)).unwrap();
        assert!(append_from(&d, EVENTS, NOW, &mut Failing(1 << 20)).is_err());
        assert_eq!(std::fs::read(archive_path(&d, EVENTS, NOW)).unwrap(), before);
        append_from(&d, EVENTS, NOW, &mut &b"streamed\n"[..]).unwrap();
        assert_eq!(read_source(&archive_path(&d, EVENTS, NOW)).unwrap(), "kept\nstreamed\n");
    }

    #[test]
    fn a_torn_member_does_not_hide_the_members_after_it() {
        let d = dir("torn");
        let p = archive_path(&d, EVENTS, NOW);
        append(&d, EVENTS, NOW, b"first\n").unwrap();
        let a = std::fs::read(&p).unwrap();
        append(&d, EVENTS, NOW, b"second line that is long enough to tear\n").unwrap();
        let ab = std::fs::read(&p).unwrap();
        append(&d, EVENTS, NOW, b"third\n").unwrap();
        let abc = std::fs::read(&p).unwrap();
        let (b, c) = (&ab[a.len()..], &abc[ab.len()..]);
        let mut torn = a.clone();
        torn.extend_from_slice(&b[..b.len() / 2]); // the member is cut off mid-stream
        torn.extend_from_slice(c);
        std::fs::write(&p, torn).unwrap();
        let text = read_source(&p).unwrap();
        assert_eq!(text, "first\nthird\n", "the cut line is dropped, the member after it is read");
    }

    #[test]
    fn a_plain_source_reads_as_it_is_and_a_damaged_archive_keeps_what_decoded() {
        let d = dir("read");
        std::fs::write(d.join("events.jsonl"), "live\n").unwrap();
        assert_eq!(read_source(&d.join("events.jsonl")).unwrap(), "live\n");
        append(&d, EVENTS, NOW, b"first\n").unwrap();
        let p = archive_path(&d, EVENTS, NOW);
        let mut bytes = std::fs::read(&p).unwrap();
        bytes.extend_from_slice(b"\x1f\x8b\x08garbage");
        std::fs::write(&p, bytes).unwrap();
        assert_eq!(read_source(&p).unwrap(), "first\n");
        assert!(read_source(&d.join("missing.jsonl.gz")).is_none());
    }
}
