//! The unsent directory: text a chat kept, one file a text, for the next
//! chat to restore ([`super::ChatPane::restore_unsent`]).
//!
//! A chat writes each file whole, by a temporary file and a rename, named
//! for the process and the moment (`<pid>-<nanos>-<n>`). The file names the
//! view the text was written in, its route, its label, and whether Enter
//! held it or it was a draft. A chat that restores a file first claims it,
//! by renaming it to `.claim.<pid>.<name>`, so two chats starting at once
//! never restore one file twice. The claimed file stays until its text is
//! sent, held again, or kept again; a chat that ends without doing any of
//! these leaves it claimed by a process now gone, and the next chat claims
//! it again. Nothing here removes a file whose text has not gone elsewhere,
//! except a temporary file cut short by a crash, which holds no whole text.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use crate::tabs::Tab;

/// The first line of a file this version writes. A file without it was
/// written by an earlier build of the delayed send and holds bare text.
const MAGIC: &str = "attend-chat unsent 2";

/// Whether Enter held the text or it was a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Enter held it, and it was not sent.
    Held,
    /// The operator never pressed Enter on it.
    Draft,
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

/// `r` as a file holds it: the header lines, a blank line, the text. The
/// header's `bytes` is the text's length, so a file cut short is told
/// from a whole one.
pub(super) fn encode(r: &Record) -> String {
    let kind = match r.kind {
        Kind::Held => "held",
        Kind::Draft => "draft",
    };
    let view = r.tab.as_ref().map(tab_key).unwrap_or_default();
    let line = |s: &str| s.replace('\n', " ");
    format!("{MAGIC}\nkind: {kind}\nview: {}\ndest: {}\nlabel: {}\nbytes: {}\n\n{}", line(&view), line(&r.dest), line(&r.label), r.text.len(), r.text)
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
    let field = |k: &str| head.lines().find_map(|l| l.strip_prefix(k).and_then(|v| v.strip_prefix(": "))).unwrap_or("");
    let bytes: Option<usize> = field("bytes").parse().ok();
    let kind = if field("kind") == "held" { Kind::Held } else { Kind::Draft };
    let whole = ended && bytes == Some(text.len());
    (Record { kind, tab: tab_of(field("view")), dest: field("dest").into(), label: field("label").into(), text: text.to_string() }, whole)
}

/// Write each record to a file of its own in `dir`. Every record is
/// tried. The outcome of each, in order, is the path written, or the path
/// tried with the error.
pub(super) fn write(dir: &Path, records: &[Record]) -> Vec<Result<PathBuf, (PathBuf, io::Error)>> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    records
        .iter()
        .map(|r| {
            let nanos = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = dir.join(format!("{}-{nanos:020}-{seq}", std::process::id()));
            match agent_settings::writer::write_atomic(&path, encode(r)) {
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
}

/// What [`claim`] found.
#[derive(Debug, Default)]
pub(super) struct Found {
    /// The files claimed, in the order they were kept.
    pub(super) claimed: Vec<Claimed>,
    /// The names of files left in place because they could not be read.
    pub(super) unreadable: Vec<String>,
}

/// Whether process `pid` is running.
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

/// The kept file's name a claimed or temporary name stands for, and the
/// pid that claimed or was writing it.
enum Name {
    Kept,
    Claimed { pid: u32, name: String },
    Temp { pid: u32, name: String },
    Other,
}

fn name_of(n: &str) -> Name {
    if let Some(rest) = n.strip_prefix(".claim.") {
        return match rest.split_once('.') {
            Some((pid, name)) => pid.parse().map_or(Name::Other, |pid| Name::Claimed { pid, name: name.into() }),
            None => Name::Other,
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

/// Claim every file in `dir` that no running chat holds, and read it.
///
/// - A kept file is renamed to this chat's claimed name before it is read,
///   so a chat starting at the same moment cannot take it too.
/// - A file claimed by a chat that is no longer running is claimed again.
/// - A temporary file left by a writer no longer running is moved to its
///   kept name when it holds a whole text, and removed when it is cut
///   short, since no whole text ever reached it.
/// - A file that cannot be read, or is not UTF-8, goes back to its kept
///   name and is named in [`Found::unreadable`].
/// - An earlier build kept one file at `dir`'s own path: it is moved into
///   the directory as a kept file first.
pub(super) fn claim(dir: &Path) -> Found {
    let me = std::process::id();
    if dir.is_file() {
        let aside = dir.with_extension(format!("moving.{me}"));
        if std::fs::rename(dir, &aside).is_ok() && std::fs::create_dir_all(dir).is_ok() {
            let _ = std::fs::rename(&aside, dir.join(format!("0-{:020}-0", 0)));
        }
    }
    let mut found = Found::default();
    let mut mine: Vec<(u128, String, PathBuf)> = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        if !path.is_file() {
            continue;
        }
        let kept = match name_of(&n) {
            Name::Kept => n.clone(),
            Name::Claimed { pid, name } if pid != me && !alive(pid) => name,
            Name::Temp { pid, name } if pid != me && !alive(pid) => {
                let whole = std::fs::read(&path).ok().and_then(|b| String::from_utf8(b).ok()).is_some_and(|b| b.starts_with(MAGIC) && decode(&b).1);
                let target = dir.join(&name);
                if !whole {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                if target.exists() || std::fs::rename(&path, &target).is_err() {
                    continue;
                }
                name
            }
            _ => continue,
        };
        let from = if matches!(name_of(&n), Name::Temp { .. }) { dir.join(&kept) } else { path };
        let to = dir.join(format!(".claim.{me}.{kept}"));
        // A rename that fails lost the race to another chat.
        if std::fs::rename(&from, &to).is_ok() {
            mine.push((written(&kept), kept, to));
        }
    }
    mine.sort();
    for (_, name, path) in mine {
        match std::fs::read(&path).map(String::from_utf8) {
            Ok(Ok(body)) => {
                let (record, _) = decode(&body);
                if record.text.trim().is_empty() {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                found.claimed.push(Claimed { path, record });
            }
            _ => {
                let _ = std::fs::rename(&path, dir.join(&name));
                found.unreadable.push(name);
            }
        }
    }
    found
}

/// Remove kept files whose text has gone elsewhere: sent, held again, or
/// kept again in a file of its own.
pub(super) fn settle(paths: impl IntoIterator<Item = PathBuf>) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

/// What the chat would keep if it ended now, noted at each tick for a
/// panic: the directory, the records, and the claimed files they replace.
type Noted = (PathBuf, Vec<Record>, Vec<PathBuf>);

static NOTED: Mutex<Option<Noted>> = Mutex::new(None);

/// Note what a panic now would keep ([`keep_noted`]).
pub(super) fn note(dir: &Path, records: Vec<Record>, origins: Vec<PathBuf>) {
    if let Ok(mut n) = NOTED.lock() {
        *n = Some((dir.to_path_buf(), records, origins));
    }
}

/// Keep what was last noted, as a panic ends the chat: each record to a
/// file of its own, then the claimed files they replace removed. A lock
/// held by the panicking thread itself is passed over, as is a noting
/// that never ran.
pub fn keep_noted() {
    let Ok(mut n) = NOTED.try_lock() else { return };
    let Some((dir, records, origins)) = n.take() else { return };
    if write(&dir, &records).iter().all(Result::is_ok) {
        settle(origins);
    }
}

/// Install the panic hook that keeps what the chat last noted unsent
/// ([`keep_noted`]), before the hook that was in place. agent-tui's own
/// hook restores the terminal first and then runs this one, which it finds
/// in place when the chat's screen opens; text typed since the last tick,
/// a tenth of a second, is the most a panic can take.
pub fn keep_on_panic() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        keep_noted();
        prev(info);
    }));
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
            assert_eq!(decode(&encode(&r)), (r, true));
        }
    }

    #[test]
    fn a_file_cut_short_is_not_whole() {
        let body = encode(&record(Kind::Draft, Some(Tab::Merged), "the whole text"));
        assert!(!decode(&body[..body.len() - 3]).1, "text short of its bytes");
        assert!(!decode(&body[..30]).1, "header cut");
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
        assert!(matches!(name_of(".claim.77.12-00000000000000000001-0"), Name::Claimed { pid: 77, ref name } if name == "12-00000000000000000001-0"));
        assert!(matches!(name_of(".12-00000000000000000001-0.88.3.tmp"), Name::Temp { pid: 88, ref name } if name == "12-00000000000000000001-0"));
        assert!(matches!(name_of(".hidden"), Name::Other));
    }
}
