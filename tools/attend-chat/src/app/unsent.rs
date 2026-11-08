// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The unsent directory: text a chat kept, one file a text, for the next
//! chat to restore ([`super::ChatPane::restore_unsent`]).
//!
//! A chat writes each file whole, by a temporary file and a rename, named
//! for the process and the moment (`<pid>-<nanos>-<n>`). The file names the
//! view the text was written in, its route, its label, and whether Enter
//! held it, it was a draft, or it was being sent as the chat ended. It names
//! its writer, the chat that wrote it, and the kept files whose text it
//! carries on, so the next chat can tell a text kept twice from two chats
//! that each kept the same words ([`claim`]). A chat
//! that restores a file first claims it: it takes an `flock` on the file,
//! renames it to `.claim.<pid>.<name>`, and holds the lock while it runs.
//! A claim is live only while its lock is held, whatever its pid names, so
//! a pid reused by another process never hides a file. A claim whose lock
//! cannot be tried is named on the status line. The claimed file stays
//! until its text is sent, held again, or kept again; a chat that ends
//! without doing any of these leaves it claimed with no lock, and the next
//! chat claims it again. Nothing here removes a file whose text has not
//! gone elsewhere: a temporary file cut short by a crash is restored as a
//! draft marked cut short, and one with no text in it is the only file
//! removed.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::tabs::Tab;

/// The first line of a file this version writes. A file without it was
/// written by an earlier build of the delayed send and holds bare text.
const MAGIC: &str = "attend-chat unsent 2";

/// Whether Enter held the text, it was a draft, or it was being sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Enter held it, and it was not sent.
    Held,
    /// The operator never pressed Enter on it.
    Draft,
    /// It was being sent as the chat ended, and may have gone: kept so it
    /// is not lost, marked so it is not sent twice unasked.
    Sending,
}

/// One text kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub(super) kind: Kind,
    /// The view it was written in; `None` from a file of an earlier build,
    /// which did not note it.
    pub(super) tab: Option<Tab>,
    /// Its route as the send resolves it ([`super::ChatPane::destination`]).
    pub(super) dest: String,
    /// Where it goes, as the destination flag named it.
    pub(super) label: String,
    pub(super) text: String,
}

fn tab_key(t: &Tab) -> String {
    match t {
        Tab::Merged => "merged".into(),
        Tab::Channel(c) => format!("channel {c}"),
        Tab::Dm(m) => format!("dm {m}"),
        Tab::Review(m) => format!("review {m}"),
    }
}

fn tab_of(s: &str) -> Option<Tab> {
    let (kind, name) = s.split_once(' ').unwrap_or((s, ""));
    match (kind, name) {
        ("merged", "") => Some(Tab::Merged),
        ("channel", n) if !n.is_empty() => Some(Tab::Channel(n.into())),
        ("dm", n) if !n.is_empty() => Some(Tab::Dm(n.into())),
        ("review", n) if !n.is_empty() => Some(Tab::Review(n.into())),
        _ => None,
    }
}

/// Who wrote a kept file, and which kept files its text carries on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Stamp {
    /// The chat that wrote it ([`writer_id`]); `None` in a file of an
    /// earlier build, which did not note it.
    pub(super) writer: Option<String>,
    /// The names of the kept files whose text it carries on: a text
    /// restored and kept again names the file it came from.
    pub(super) from: Vec<String>,
}

/// A name for one chat as a writer of kept files, made once when the chat
/// starts: its pid, the moment and a count, so two chats never share one,
/// a pid reused by another process included.
pub(super) fn writer_id() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{}.{nanos}.{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed))
}

/// The kept file's name a kept or claimed path stands for.
fn kept_name(p: &Path) -> Option<String> {
    let n = p.file_name()?.to_string_lossy().to_string();
    match name_of(&n) {
        Name::Kept => Some(n),
        Name::Claimed { name } => Some(name),
        _ => None,
    }
}

/// `r` as a file holds it: the header lines, a blank line, the text. The
/// header's `bytes` is the text's length, so a file cut short is told
/// from a whole one. The writer and the files it carries on come before
/// it, so a file cut anywhere in its text still names them.
pub(super) fn encode(r: &Record, stamp: &Stamp) -> String {
    let kind = match r.kind {
        Kind::Held => "held",
        Kind::Draft => "draft",
        Kind::Sending => "sending",
    };
    let view = r.tab.as_ref().map(tab_key).unwrap_or_default();
    let line = |s: &str| s.replace('\n', " ");
    let writer = stamp.writer.as_deref().unwrap_or_default();
    format!(
        "{MAGIC}\nkind: {kind}\nview: {}\ndest: {}\nlabel: {}\nwriter: {}\nfrom: {}\nbytes: {}\n\n{}",
        line(&view),
        line(&r.dest),
        line(&r.label),
        line(writer),
        line(&stamp.from.join(" ")),
        r.text.len(),
        r.text
    )
}

/// The header lines of a file this version writes, before the text.
fn header(body: &str) -> Option<&str> {
    let rest = body.strip_prefix(MAGIC)?.strip_prefix('\n')?;
    Some(rest.split_once("\n\n").map_or(rest, |(h, _)| h))
}

/// The value of header field `k`, empty when it is absent.
fn field<'a>(head: &'a str, k: &str) -> &'a str {
    head.lines().find_map(|l| l.strip_prefix(k).and_then(|v| v.strip_prefix(": "))).unwrap_or("")
}

/// The writer and the files carried on that a file names.
pub(super) fn stamp_of(body: &str) -> Stamp {
    let Some(head) = header(body) else { return Stamp::default() };
    let writer = Some(field(head, "writer").to_string()).filter(|w| !w.is_empty());
    Stamp { writer, from: field(head, "from").split_whitespace().map(String::from).collect() }
}

/// The record a file holds. A file without the header is an earlier
/// build's bare text: a draft whose view is not known. `whole` is false
/// when the header is cut short or the text is shorter than its `bytes`.
pub(super) fn decode(body: &str) -> (Record, bool) {
    let Some(rest) = body.strip_prefix(MAGIC).and_then(|r| r.strip_prefix('\n')) else {
        return (Record { kind: Kind::Draft, tab: None, dest: String::new(), label: String::new(), text: body.trim_end().to_string() }, true);
    };
    let (head, text, ended) = match rest.split_once("\n\n") {
        Some((h, t)) => (h, t, true),
        None => (rest, "", false),
    };
    let field = |k: &str| field(head, k);
    let bytes: Option<usize> = field("bytes").parse().ok();
    let kind = match field("kind") {
        "held" => Kind::Held,
        "sending" => Kind::Sending,
        _ => Kind::Draft,
    };
    let whole = ended && bytes == Some(text.len());
    (Record { kind, tab: tab_of(field("view")), dest: field("dest").into(), label: field("label").into(), text: text.to_string() }, whole)
}

/// Write each text to a file of its own in `dir`, stamped with `writer`
/// and the kept files the text carries on. Every text is tried. The
/// outcome of each, in order, is the path written, or the path tried with
/// the error.
pub(super) fn write(dir: &Path, writer: &str, texts: &[(Record, Vec<PathBuf>)]) -> Vec<Result<PathBuf, (PathBuf, io::Error)>> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    texts
        .iter()
        .map(|(r, origins)| {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("{}-{nanos:020}-{seq}", std::process::id()));
            let stamp = Stamp { writer: Some(writer.to_string()), from: origins.iter().filter_map(|p| kept_name(p)).collect() };
            match agent_settings::writer::write_atomic(&path, encode(r, &stamp)) {
                Ok(()) => Ok(path),
                Err(e) => Err((path, e)),
            }
        })
        .collect()
}

/// One file this chat claimed, and what it holds.
#[derive(Debug)]
pub(super) struct Claimed {
    /// Its claimed path, which the chat removes once the text has gone
    /// elsewhere.
    pub(super) path: PathBuf,
    pub(super) record: Record,
    /// The file was cut short: what it holds is the text up to the cut,
    /// restored as a draft.
    pub(super) partial: bool,
}

/// What [`claim`] found.
#[derive(Debug, Default)]
pub(super) struct Found {
    /// The files claimed, in the order they were kept.
    pub(super) claimed: Vec<Claimed>,
    /// The names of files left in place because they could not be read.
    pub(super) unreadable: Vec<String>,
    /// Claimed files whose lock could not be tried, each with the reason:
    /// left in place, and named on the status line rather than hidden.
    pub(super) unjudged: Vec<(String, String)>,
}

/// Whether process `pid` is running, or another process now has its pid.
fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 sends nothing; it only checks the pid.
        let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
        r == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Take an exclusive lock on `f` without waiting: true when taken, false
/// when another open of the file holds it, an error when the lock cannot
/// be tried here.
fn try_lock(f: &File) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: flock on a descriptor this File owns.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(false)
        } else {
            Err(e)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = f;
        Err(io::Error::new(io::ErrorKind::Unsupported, "no file locks here"))
    }
}

/// The locks this chat holds on the files it claimed, each by its claimed
/// path, until [`settle`] removes the file.
static LOCKS: Mutex<Vec<(PathBuf, File)>> = Mutex::new(Vec::new());

fn locks() -> MutexGuard<'static, Vec<(PathBuf, File)>> {
    LOCKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The kept file's name a claimed or temporary name stands for, and the
/// pid that claimed or was writing it.
enum Name {
    Kept,
    Claimed { name: String },
    Temp { pid: u32, name: String },
    Other,
}

fn name_of(n: &str) -> Name {
    if let Some(rest) = n.strip_prefix(".claim.") {
        return match rest.split_once('.') {
            Some((pid, name)) if pid.parse::<u32>().is_ok() => Name::Claimed { name: name.into() },
            _ => Name::Other,
        };
    }
    if let Some(rest) = n.strip_prefix('.').and_then(|r| r.strip_suffix(".tmp")) {
        // `.<name>.<pid>.<seq>.tmp`, as agent_settings::writer names it.
        let mut parts = rest.rsplitn(3, '.');
        let (_seq, pid, name) = (parts.next(), parts.next(), parts.next());
        return match (pid.and_then(|p| p.parse().ok()), name) {
            (Some(pid), Some(name)) => Name::Temp { pid, name: name.into() },
            _ => Name::Other,
        };
    }
    if n.starts_with('.') {
        Name::Other
    } else {
        Name::Kept
    }
}

/// When a kept file was written, from its name, for the order.
fn written(name: &str) -> u128 {
    name.split('-').nth(1).and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// How long a temporary file is taken to be still being written. A write
/// takes milliseconds; one older than this was left by a writer that
/// ended, whatever process now has its pid.
const TEMP_LIVE: Duration = Duration::from_secs(60);

/// Whether the temporary file at `path`, written by `pid`, was left by a
/// writer that ended.
fn temp_left(path: &Path, pid: u32) -> bool {
    let old = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age >= TEMP_LIVE);
    pid != std::process::id() && (old || !alive(pid))
}

/// `bytes` as text: whole, or up to a character a cut left unfinished at
/// the end, with whether it was cut there. `None` for bytes that are not
/// UTF-8 before the end.
fn text_of(bytes: Vec<u8>) -> Option<(String, bool)> {
    match String::from_utf8(bytes) {
        Ok(s) => Some((s, false)),
        Err(e) if e.utf8_error().error_len().is_none() => {
            let at = e.utf8_error().valid_up_to();
            let mut b = e.into_bytes();
            b.truncate(at);
            String::from_utf8(b).ok().map(|s| (s, true))
        }
        Err(_) => None,
    }
}

/// An earlier build kept one file at `dir`'s own path: move it into the
/// directory as a kept file. Put aside first as `<dir>.moving.<pid>`, it
/// goes back where it was when the directory cannot be made, and one a
/// crash left aside is moved in here too.
fn move_in_single_file(dir: &Path) {
    let me = std::process::id();
    if dir.is_file() {
        let aside = dir.with_extension(format!("moving.{me}"));
        if std::fs::rename(dir, &aside).is_ok() && std::fs::create_dir_all(dir).is_err() {
            let _ = std::fs::rename(&aside, dir);
            return;
        }
    }
    let (Some(parent), Some(stem)) = (dir.parent(), dir.file_name().map(|n| format!("{}.moving.", n.to_string_lossy()))) else { return };
    if !dir.is_dir() {
        return;
    }
    for e in std::fs::read_dir(parent).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        let Some(pid) = n.strip_prefix(&stem).and_then(|p| p.parse::<u32>().ok()) else { continue };
        // Another chat's, still moving its own in.
        if pid != me && !temp_left(&e.path(), pid) {
            continue;
        }
        let _ = std::fs::rename(e.path(), dir.join(format!("0-{:020}-{pid}", 0)));
    }
}

/// Claim every file in `dir` whose lock no running chat holds, and read it.
///
/// - A kept file is locked, then renamed to this chat's claimed name before
///   it is read, so a chat starting at the same moment cannot take it too.
/// - A claimed file whose lock is free is claimed again, whatever pid its
///   name holds. One whose lock cannot be tried stays, and is named in
///   [`Found::unjudged`].
/// - A temporary file left by a writer that ended is moved to its kept
///   name: whole, or cut short, which restores as a draft marked so. One
///   with no text in it at all is removed.
/// - A file that cannot be read, or is not UTF-8, goes back to its kept
///   name and is named in [`Found::unreadable`].
/// - A file whose text another file holds whole, for the same view and
///   route, is a copy, removed only when the two have one writer, or one
///   carries the other's text on ([`copy_of`]). Two chats that each kept
///   the same words keep a file each.
/// - An earlier build kept one file at `dir`'s own path: it is moved into
///   the directory as a kept file first.
pub(super) fn claim(dir: &Path, writer: &str) -> Found {
    let me = std::process::id();
    move_in_single_file(dir);
    let mut found = Found::default();
    let mut mine: Vec<(u128, String, PathBuf, File)> = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        let mut path = e.path();
        if !path.is_file() {
            continue;
        }
        let kept = match name_of(&n) {
            Name::Kept => n.clone(),
            Name::Claimed { name } => name,
            Name::Temp { pid, name } if temp_left(&path, pid) => {
                let has_text = std::fs::read(&path).ok().and_then(text_of).is_some_and(|(b, _)| if b.starts_with(MAGIC) { !decode(&b).0.text.is_empty() } else { !b.trim().is_empty() });
                if !has_text {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                let target = dir.join(&name);
                if target.exists() || std::fs::rename(&path, &target).is_err() {
                    continue;
                }
                path = target;
                name
            }
            _ => continue,
        };
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) if matches!(name_of(&n), Name::Claimed { .. }) => {
                found.unjudged.push((n, e.to_string()));
                continue;
            }
            Err(_) => {
                found.unreadable.push(n);
                continue;
            }
        };
        match try_lock(&file) {
            Ok(true) => {}
            // Another chat holds it: its claim is live, or it is claiming
            // the file now.
            Ok(false) => continue,
            Err(e) => {
                found.unjudged.push((n, e.to_string()));
                continue;
            }
        }
        let to = dir.join(format!(".claim.{me}.{kept}"));
        // A rename that fails lost the race to another chat.
        if path == to || std::fs::rename(&path, &to).is_ok() {
            mine.push((written(&kept), kept, to, file));
        }
    }
    mine.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut read = Vec::new();
    for (_, name, path, mut file) in mine {
        let mut bytes = Vec::new();
        match file.read_to_end(&mut bytes).ok().and_then(|_| text_of(bytes)) {
            Some((body, cut)) => {
                let (mut record, whole) = decode(&body);
                let sending = record.kind == Kind::Sending;
                let partial = cut || !whole;
                if partial {
                    record.kind = Kind::Draft;
                }
                if record.text.trim().is_empty() {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                read.push(Taken { name, path, file, record, partial, sending, stamp: stamp_of(&body) });
            }
            None => {
                let _ = std::fs::rename(&path, dir.join(&name));
                found.unreadable.push(name);
            }
        }
    }
    for t in drop_copies(dir, writer, read) {
        locks().push((t.path.clone(), t.file));
        found.claimed.push(Claimed { path: t.path, record: t.record, partial: t.partial });
    }
    found
}

/// One file claimed and read.
struct Taken {
    /// Its kept name.
    name: String,
    path: PathBuf,
    file: File,
    record: Record,
    partial: bool,
    /// Its header said it was being sent, whether or not it is whole.
    sending: bool,
    stamp: Stamp,
}

/// Whether `b` is a copy of `a`: `a` holds `b`'s text whole, for the same
/// view and route, and the two are one text, not two chats' alike. They
/// are one text when one writer wrote both, or one carries the other on
/// ([`Stamp::from`]); a file of an earlier build names no writer and is
/// never a copy. `b` cut short is a copy when its text starts `a`'s. `b`
/// whole is a copy when its text is `a`'s, and `a` says at least what
/// `b` does: the same kind, or `a` was being sent.
fn copy_of(b: &Taken, a: &Taken) -> bool {
    let same_writer = b.stamp.writer.is_some() && b.stamp.writer == a.stamp.writer;
    let carried = b.stamp.from.contains(&a.name) || a.stamp.from.contains(&b.name);
    let route = b.record.tab == a.record.tab && b.record.dest == a.record.dest && b.record.label == a.record.label;
    if !(same_writer || carried) || !route || a.partial {
        return false;
    }
    if b.partial {
        a.record.text.starts_with(&b.record.text)
    } else {
        b.record.text == a.record.text && (b.record.kind == a.record.kind || a.sending)
    }
}

/// `texts` with each copy of another removed, its file with it ([`copy_of`]):
/// the later of two that are each other's copy. A copy cut short that was
/// being sent leaves its mark on the text it copies: that is written again
/// marked [`Kind::Sending`], and claimed in its place, before either file
/// goes. When it cannot be, both stay.
fn drop_copies(dir: &Path, writer: &str, mut texts: Vec<Taken>) -> Vec<Taken> {
    let mut i = texts.len();
    while i > 0 {
        i -= 1;
        let Some(a) = (0..texts.len()).find(|&a| a != i && copy_of(&texts[i], &texts[a])) else { continue };
        if texts[i].sending && texts[a].record.kind != Kind::Sending {
            let Some((path, file)) = mark_sending(dir, writer, &texts[a]) else { continue };
            let old = std::mem::replace(&mut texts[a].path, path);
            let _ = std::fs::remove_file(old);
            texts[a].file = file;
            texts[a].record.kind = Kind::Sending;
            texts[a].sending = true;
        }
        let b = texts.remove(i);
        let _ = std::fs::remove_file(&b.path);
    }
    texts
}

/// `t` written again by `writer` as a new kept file marked
/// [`Kind::Sending`], carrying `t` on, and claimed: its claimed path and
/// the open file holding its lock.
fn mark_sending(dir: &Path, writer: &str, t: &Taken) -> Option<(PathBuf, File)> {
    let record = Record { kind: Kind::Sending, ..t.record.clone() };
    let path = write(dir, writer, &[(record, vec![t.path.clone()])]).remove(0).ok()?;
    let file = File::open(&path).ok()?;
    if !matches!(try_lock(&file), Ok(true)) {
        return None;
    }
    let to = dir.join(format!(".claim.{}.{}", std::process::id(), kept_name(&path)?));
    std::fs::rename(&path, &to).ok()?;
    Some((to, file))
}

/// Remove kept files whose text has gone elsewhere: sent, held again, or
/// kept again in a file of its own. Their locks go with them.
pub(super) fn settle(paths: impl IntoIterator<Item = PathBuf>) {
    let paths: Vec<PathBuf> = paths.into_iter().collect();
    for p in &paths {
        let _ = std::fs::remove_file(p);
    }
    locks().retain(|(p, _)| !paths.contains(p));
}

/// One text the chat would keep if it ended now: its record, the claimed
/// files it carries, and whether it is being sent at this moment.
#[derive(Debug, Clone)]
pub(super) struct Noted {
    pub(super) record: Record,
    pub(super) origins: Vec<PathBuf>,
    /// Being sent: kept as [`Kind::Sending`] if the chat ends before the
    /// send returns.
    pub(super) flight: bool,
}

/// What the chat would keep if it ended now, for a panic and a forced
/// exit: the directory and each text. The chat notes it after each key,
/// paste, frame and tick, around each send and as each text is kept, and
/// clears it once its ending has kept what it keeps.
static NOTED: Mutex<Option<(PathBuf, String, Vec<Noted>)>> = Mutex::new(None);

/// Held while a text leaves the chat (a send, a keep, a discard) and the
/// noting that follows it, so a forced exit never keeps a text half gone.
static MOVING: Mutex<()> = Mutex::new(());

/// Set once a panic or a forced exit has kept what was noted, so the
/// other keeps nothing more.
static KEPT: AtomicBool = AtomicBool::new(false);

fn noted() -> MutexGuard<'static, Option<(PathBuf, String, Vec<Noted>)>> {
    NOTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Note what the chat would keep now ([`keep_noted`]), written by
/// `writer`.
pub(super) fn note(dir: &Path, writer: &str, texts: Vec<Noted>) {
    *noted() = Some((dir.to_path_buf(), writer.to_string(), texts));
}

/// Nothing is left to keep: the chat's ending kept it.
pub(super) fn clear() {
    *noted() = None;
}

/// Hold while a text leaves the chat, through the noting after it.
pub(super) fn moving() -> MutexGuard<'static, ()> {
    MOVING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Write each text noted to a file of its own, a text being sent as
/// [`Kind::Sending`], then remove the claimed files the written ones
/// carried.
fn keep_texts(dir: &Path, writer: &str, texts: Vec<Noted>) {
    let records: Vec<(Record, Vec<PathBuf>)> = texts.iter().map(|t| (Record { kind: if t.flight { Kind::Sending } else { t.record.kind }, ..t.record.clone() }, t.origins.clone())).collect();
    let mut done = Vec::new();
    for (t, outcome) in texts.into_iter().zip(write(dir, writer, &records)) {
        if outcome.is_ok() {
            done.extend(t.origins);
        }
    }
    settle(done);
}

/// Take a lock without waiting, a poisoned one included.
fn try_take<T>(m: &'static Mutex<T>) -> Option<MutexGuard<'static, T>> {
    match m.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

/// Take a lock within `bound`, a poisoned one included.
fn take_within<T>(m: &'static Mutex<T>, bound: Duration) -> Option<MutexGuard<'static, T>> {
    let start = Instant::now();
    loop {
        if let Some(g) = try_take(m) {
            return Some(g);
        }
        if start.elapsed() >= bound {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Keep what was last noted, as a panic ends the chat. A panic inside a
/// send finds [`MOVING`] held and keeps the text being sent as
/// [`Kind::Sending`]. A panic inside the noting itself, which holds
/// [`NOTED`], keeps nothing.
pub fn keep_noted() {
    if KEPT.swap(true, Ordering::SeqCst) {
        return;
    }
    let _moving = try_take(&MOVING);
    let Some(mut n) = try_take(&NOTED) else { return };
    if let Some((dir, writer, texts)) = n.take() {
        keep_texts(&dir, &writer, texts);
    }
}

/// Keep what was last noted, as a forced exit ends the chat
/// ([`agent_tui::on_forced_exit`]), from agent-tui's watch thread while
/// the run loop may be wedged. It waits up to a second for a text that is
/// leaving the chat to finish, so what it keeps is exact; past that it
/// keeps the text being sent as [`Kind::Sending`]. It holds both locks
/// until the process exits, so nothing is sent or kept after it.
pub fn keep_noted_waiting() {
    if KEPT.swap(true, Ordering::SeqCst) {
        return;
    }
    let moving = take_within(&MOVING, Duration::from_secs(1));
    let Some(mut n) = take_within(&NOTED, Duration::from_secs(1)) else { return };
    if let Some((dir, writer, texts)) = n.take() {
        keep_texts(&dir, &writer, texts);
    }
    std::mem::forget(n);
    std::mem::forget(moving);
}

/// Install the panic hook that keeps what the chat last noted unsent
/// ([`keep_noted`]), before the hook that was in place. agent-tui's own
/// hook restores the terminal first and then runs this one, which it finds
/// in place when the chat's screen opens.
pub fn keep_on_panic() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        keep_noted();
        prev(info);
    }));
}

/// Keep what the chat last noted unsent before every forced exit
/// ([`keep_noted_waiting`]).
pub fn keep_on_forced_exit() {
    agent_tui::on_forced_exit(keep_noted_waiting);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: Kind, tab: Option<Tab>, text: &str) -> Record {
        Record { kind, tab, dest: "dm a1".into(), label: "@Nyx (quiet)".into(), text: text.into() }
    }

    #[test]
    fn a_record_reads_back_as_written() {
        for tab in [Tab::Merged, Tab::Channel("infra".into()), Tab::Dm("a1".into()), Tab::Review("a1".into())] {
            let r = record(Kind::Held, Some(tab), "two lines\n\nand a blank between");
            assert_eq!(decode(&encode(&r, &Stamp::default())), (r, true));
        }
    }

    #[test]
    fn a_file_cut_short_is_not_whole() {
        let body = encode(&record(Kind::Draft, Some(Tab::Merged), "the whole text"), &Stamp::default());
        assert!(!decode(&body[..body.len() - 3]).1, "text short of its bytes");
        assert!(!decode(&body[..30]).1, "header cut");
    }

    /// The writer and the files carried on read back, from a file cut
    /// short in its text too; a file with none names none.
    #[test]
    fn a_stamp_reads_back_from_a_whole_or_cut_file() {
        let stamp = Stamp { writer: Some("12.34.0".into()), from: vec!["1-00000000000000000001-0".into(), "2-00000000000000000002-1".into()] };
        let body = encode(&record(Kind::Held, Some(Tab::Merged), "the text"), &stamp);
        assert_eq!(stamp_of(&body), stamp);
        assert_eq!(stamp_of(&body[..body.len() - 3]), stamp, "cut in its text");
        assert_eq!(stamp_of(&encode(&record(Kind::Held, None, "t"), &Stamp::default())), Stamp::default());
        assert_eq!(stamp_of("bare text of an earlier build"), Stamp::default());
    }

    #[test]
    fn an_earlier_builds_bare_text_is_a_draft_with_no_view() {
        let (r, whole) = decode("typed before views were noted\n");
        assert!(whole);
        assert_eq!((r.kind, r.tab, r.text.as_str()), (Kind::Draft, None, "typed before views were noted"));
    }

    #[test]
    fn names_tell_kept_claimed_and_temporary_files_apart() {
        assert!(matches!(name_of("12-00000000000000000001-0"), Name::Kept));
        assert!(matches!(name_of(".claim.77.12-00000000000000000001-0"), Name::Claimed { ref name } if name == "12-00000000000000000001-0"));
        assert!(matches!(name_of(".12-00000000000000000001-0.88.3.tmp"), Name::Temp { pid: 88, ref name } if name == "12-00000000000000000001-0"));
        assert!(matches!(name_of(".hidden"), Name::Other));
    }
}
