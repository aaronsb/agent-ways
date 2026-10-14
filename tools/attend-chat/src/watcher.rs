//! Directory watcher that streams signals into the TUI.
//!
//! `spawn_watcher` does three things, in order:
//!
//!   1. scans relevant directories for existing `.signal` files
//!      (sorted by mtime ascending) so a freshly-started TUI catches
//!      up on recent traffic instead of opening empty;
//!   2. installs a `notify` recommended watcher on the signals base
//!      *recursively*; creates and writes to files that pass
//!      [`accept_path`] are forwarded as [`Signal`] values onto the
//!      caller's channel;
//!   3. parks the watcher on a dedicated thread so its lifetime is
//!      tied to the process, not to any caller scope.
//!
//! The watcher is recursive because channel dirs (`@name/`), every
//! project's tray and every person's tray live as sibling directories
//! under the base. A non-recursive watcher would miss them, and would
//! also miss *new* ones made after the TUI started, which is the common
//! case (another agent runs `attend channel join deploy` during the
//! session). Every tray is read, not only the chat's own: the person's
//! direct messages to another project are on disk only in that
//! project's tray, and an agent's own DM view holds what it sent and
//! received with anyone (ADR-405, issue #995). Which of them a view of
//! the person's shows is [`crate::signal::Channel`]'s classification.
//!
//! The mark store under `_tombstones/` (ADR-404 D3e) is no tray, and
//! the watcher reads it for what to leave out: a marked message, and what
//! a tombstoned session sent up to its tombstone, are withheld at backfill
//! and as they arrive. When the store changes, a message newly hidden is
//! withdrawn from the chat and one no longer hidden, after an `--undo`,
//! is read again and restored.
//!
//! `notify`'s callback runs on its own thread and sends into an
//! unbounded [`std::sync::mpsc`] channel, which the screen drains
//! between frames. Unbounded means a slow renderer can't back-pressure
//! the filesystem listener into dropping events.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use attend_groups::tombstones::{self, Marks};
use std::sync::mpsc::Sender;
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::signal::{parse_file, Own, Signal};

/// What the watcher sends the chat.
#[derive(Debug, Clone)]
pub enum Delivery {
    /// A signal read for the first time.
    Arrived(Signal),
    /// A signal a mark withheld, read again once the mark was removed: it
    /// takes its place among the messages by when it was sent.
    Restored(Signal),
    /// The signals a new mark hides, by tray and id ([`Signal::same_file`]).
    Withdrawn(Vec<(String, String)>),
}

impl From<Signal> for Delivery {
    fn from(sig: Signal) -> Self {
        Delivery::Arrived(sig)
    }
}

impl Delivery {
    /// The signal this delivers, arrived or restored.
    pub fn signal(self) -> Option<Signal> {
        match self {
            Delivery::Arrived(s) | Delivery::Restored(s) => Some(s),
            Delivery::Withdrawn(_) => None,
        }
    }
}

/// Whether `marks` hide the signal at `path` that `sender` sent at `sent`
/// (its mtime): its file is marked, or the sender's session is tombstoned
/// and the file is no newer than the tombstone.
fn hidden(marks: &Marks, path: &Path, sender: Option<&str>, sent: SystemTime) -> bool {
    let key = path.file_name().and_then(|n| n.to_str()).map(attend_state::seen_key);
    key.is_some_and(|k| marks.hides(&k)) || sender.is_some_and(|sender| marks.hides_sender(sender, sent))
}

/// Whether `path` is in the mark store under `base`.
fn in_mark_store(base: &Path, path: &Path) -> bool {
    path.starts_with(tombstones::dir(base))
}

/// Decide whether a filesystem path under the signals base is a
/// signal we want to stream into the TUI.
///
/// Accepts four shapes:
/// - `<base>/_broadcast/<name>.signal` — `#open`
/// - `<base>/@<group>/<name>.signal` — any channel
/// - `<base>/<tray>/<name>.signal` — a direct message in any project's
///   tray, the chat's own or another's
/// - `<base>/_people/<person>/<name>.signal` — a direct message to any
///   person (ADR-401, `to`)
///
/// Rejects non-`.signal` files (`_groups.yaml`, tmp files), a file at the
/// base itself or deeper in a tray than its own level, and any other
/// `_`-named dir, which attend keeps for its own state.
pub fn accept_path(base: &Path, path: &Path) -> bool {
    if path.extension().and_then(|s| s.to_str()) != Some("signal") {
        return false;
    }
    let Ok(rel) = path.strip_prefix(base) else {
        return false;
    };
    let comps: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect();
    let Some(first) = comps.first() else {
        return false;
    };
    if first == attend_groups::BROADCAST_DIR || first.starts_with('@') {
        return true;
    }
    if first == attend_groups::PEOPLE_DIR {
        return comps.len() == 3;
    }
    comps.len() == 2 && is_tray(first)
}

/// Whether a dir under the base named `name` is a project's tray: neither
/// a channel nor one of attend's own `_` dirs, nor a step out of the base.
fn is_tray(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('_') && !name.starts_with('@') && !name.starts_with('.')
}

/// `own_tray` is the chat's project tray by its encoded name, and `person`
/// the member id of the person at the chat (ADR-170); either is empty for
/// none. They classify what is read ([`crate::signal::Own`]).
pub fn spawn_watcher(base: PathBuf, own_tray: String, person: String, tx: Sender<Delivery>) -> notify::Result<()> {
    std::fs::create_dir_all(&base).ok();
    // Pre-create the broadcast, own-cwd and person subdirs so notify's
    // recursive watch installs sub-watchers on them before any signal is
    // written. A dir made later gets its sub-watcher only after it is made,
    // so a write that follows its `create_dir` at once raises no event; the
    // callback reads such a dir when it sees the dir made.
    std::fs::create_dir_all(base.join(attend_groups::BROADCAST_DIR)).ok();
    if !own_tray.is_empty() {
        std::fs::create_dir_all(base.join(&own_tray)).ok();
    }
    if !person.is_empty() {
        std::fs::create_dir_all(attend_groups::person_dir(&base, &person)).ok();
    }

    // Backfill existing signals from the accepted shapes so the stream
    // isn't empty on launch, in send order across all source dirs so
    // replay matches wall-clock arrival order.
    let mut backfill: Vec<PathBuf> = scan_targets(&base).iter().flat_map(|d| signal_files(d)).collect();
    backfill.sort_by_cached_key(|p| send_order(p));
    let feed = Feed::new(&base, Own { tray: own_tray, person }, tx);
    for path in backfill {
        feed.deliver(path);
    }

    let events = feed.clone();
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            let feed = &events;
            let Ok(event) = res else { return };
            // The mark store changed: a mark written or removed, a session
            // tombstoned or its tombstone moved or taken back.
            if event.paths.iter().any(|p| in_mark_store(&feed.base, p)) {
                if !matches!(event.kind, EventKind::Access(_)) {
                    feed.marks_changed();
                }
                return;
            }
            // A file or dir gone (swept, purged, renamed away) leaves the
            // record of what was delivered, so it stays as large as the
            // signals on disk.
            if matches!(event.kind, EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From))) {
                for path in &event.paths {
                    feed.forget(path);
                }
                return;
            }
            // Atomic rename fires two events — `Name(To)` and
            // `Name(Both)` — both referencing the final filename. We
            // accept `Create(_)` (platforms that emit it, notably
            // kqueue) and `Modify(Name(To))` (the rename-destination
            // arrival on Linux), and explicitly drop `Name(Both)` and
            // `Name(From)` so we don't deliver each signal twice.
            let accept = matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To))
            );
            if !accept {
                return;
            }
            for path in event.paths {
                if path.is_dir() {
                    feed.new_dir(path);
                } else {
                    feed.deliver(path);
                }
            }
        },
        Config::default().with_poll_interval(Duration::from_millis(250)),
    )?;
    watcher.watch(&base, RecursiveMode::Recursive)?;
    // A mark written between the backfill's read of the store and the watch
    // raised no event: read the store once more now the watch is in place.
    feed.marks_changed();

    // Park the watcher on a dedicated thread so its handle (and its
    // inotify descriptor on Linux) lives for the rest of the process.
    // Dropping the handle would stop the watcher.
    thread::spawn(move || {
        let _keep_alive = watcher;
        loop {
            thread::park();
        }
    });

    Ok(())
}

/// Where the watcher delivers: the signals base, the chat's own trays, the
/// sender the chat drains, and every file read and still on disk, so a file
/// the watch reports twice, or one both a new dir's scan and its own event
/// find, is delivered once; with the marks as last read, so a mark taken
/// back restores what it hid.
#[derive(Clone)]
struct Feed {
    base: PathBuf,
    own: Own,
    tx: Sender<Delivery>,
    seen: Arc<Mutex<Seen>>,
    /// Set while a read of the changed mark store is waiting to run, and
    /// when the store last changed.
    rereading: Arc<AtomicBool>,
    marks_touched: Arc<Mutex<Instant>>,
}

/// What the feed has read, under one lock: the marks decide which files
/// are shown.
#[derive(Default)]
struct Seen {
    marks: Marks,
    read: HashMap<PathBuf, Read>,
}

/// A file the feed read: which signal it is, who sent it and when (a
/// signal is written once), and whether the chat was sent it or a mark
/// withholds it.
struct Read {
    tray: String,
    id: String,
    sender: Option<String>,
    sent: SystemTime,
    shown: bool,
}

/// How long after a dir is made its contents are read again, by when the
/// watch on it is in place.
const NEW_DIR_SETTLE: Duration = Duration::from_millis(250);

/// How long the mark store must go unchanged before it is read again. A
/// delete links its marks one by one, so the events of one batch come as
/// a burst, read once when it ends.
const MARKS_SETTLE: Duration = Duration::from_millis(100);

impl Feed {
    /// A feed for signals base `base`, with the marks it holds now.
    fn new(base: &Path, own: Own, tx: Sender<Delivery>) -> Feed {
        let seen = Seen { marks: Marks::load(base), ..Default::default() };
        Feed {
            base: base.to_path_buf(),
            own,
            tx,
            seen: Arc::new(Mutex::new(seen)),
            rereading: Default::default(),
            marks_touched: Arc::new(Mutex::new(Instant::now())),
        }
    }

    fn seen(&self) -> std::sync::MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Send the signal at `path` on, unless it is not one the chat reads,
    /// was read before or is hidden by a mark.
    fn deliver(&self, path: PathBuf) {
        if !accept_path(&self.base, &path) {
            return;
        }
        let mut seen = self.seen();
        if seen.read.contains_key(&path) {
            return;
        }
        // Only what was read is recorded, so a file that did not parse is
        // tried again when it changes.
        let Some(sig) = parse_file(&path, &self.own) else { return };
        let sender = sig.sender_member();
        let sent = path.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        let shown = !hidden(&seen.marks, &path, sender.as_deref(), sent);
        seen.read.insert(path, Read { tray: sig.tray.clone(), id: sig.id.clone(), sender, sent, shown });
        if shown {
            let _ = self.tx.send(Delivery::Arrived(sig));
        }
    }

    /// `path` is gone: a file leaves the record, a dir takes what was
    /// under it.
    fn forget(&self, path: &Path) {
        self.seen().read.retain(|p, _| !p.starts_with(path));
    }

    /// The mark store changed: read it again once it has gone
    /// [`MARKS_SETTLE`] without a change.
    fn marks_changed(&self) {
        *self.marks_touched.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
        if self.rereading.swap(true, Ordering::SeqCst) {
            return;
        }
        let feed = self.clone();
        thread::spawn(move || {
            loop {
                let quiet = feed.marks_touched.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                if quiet >= MARKS_SETTLE {
                    break;
                }
                thread::sleep(MARKS_SETTLE - quiet);
            }
            feed.rereading.store(false, Ordering::SeqCst);
            feed.reread_marks();
        });
    }

    /// Read the mark store again. What the marks now hide is withdrawn from
    /// the chat; what they no longer hide is read again and restored, oldest
    /// first; a withheld file gone from disk leaves the record.
    fn reread_marks(&self) {
        let marks = Marks::load(&self.base);
        let mut seen = self.seen();
        let mut withdrawn = Vec::new();
        let mut unhidden = Vec::new();
        for (path, r) in seen.read.iter_mut() {
            match (r.shown, hidden(&marks, path, r.sender.as_deref(), r.sent)) {
                (true, true) => {
                    r.shown = false;
                    withdrawn.push((r.tray.clone(), r.id.clone()));
                }
                (false, false) => unhidden.push(path.clone()),
                _ => {}
            }
        }
        unhidden.sort_by_cached_key(|p| send_order(p));
        let mut restored = Vec::new();
        for path in unhidden {
            match parse_file(&path, &self.own) {
                Some(sig) => {
                    if let Some(r) = seen.read.get_mut(&path) {
                        r.shown = true;
                    }
                    restored.push(sig);
                }
                None if !path.exists() => {
                    seen.read.remove(&path);
                }
                None => {}
            }
        }
        seen.marks = marks;
        if !withdrawn.is_empty() {
            let _ = self.tx.send(Delivery::Withdrawn(withdrawn));
        }
        for sig in restored {
            let _ = self.tx.send(Delivery::Restored(sig));
        }
    }

    /// A dir made under the base: a new tray or channel, or `_people/`
    /// itself with a person's tray already in it. notify reports it before
    /// it adds the watch on it, so a signal written into it at once (the
    /// first direct message to a project: `mkdir`, then the write) raises
    /// no event of its own, nor does a dir made in it at once. Read what it
    /// and the dirs in it hold now, and again once the watch is in place.
    fn new_dir(&self, dir: PathBuf) {
        self.read_dir_tree(&dir);
        let feed = self.clone();
        thread::spawn(move || {
            thread::sleep(NEW_DIR_SETTLE);
            feed.read_dir_tree(&dir);
        });
    }

    /// Deliver the signals in `dir` and in the dirs directly under it, the
    /// depth a tray sits below the base.
    fn read_dir_tree(&self, dir: &Path) {
        let mut dirs = vec![dir.to_path_buf()];
        if let Ok(entries) = std::fs::read_dir(dir) {
            dirs.extend(entries.filter_map(|e| e.ok()).filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()));
        }
        for d in dirs {
            for file in signal_files(&d) {
                self.deliver(file);
            }
        }
    }
}

/// The order signals are delivered in: by mtime, ties (the parts of a long
/// message, written within one timestamp tick) by the send time the id
/// carries.
fn send_order(path: &Path) -> (SystemTime, (u128, u64), PathBuf) {
    let at = path.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
    (at, agent_identity::signal_send_order(path.file_name().and_then(|n| n.to_str()).unwrap_or("")), path.to_path_buf())
}

/// The `.signal` files in `dir`, oldest first, in [`send_order`].
fn signal_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> =
        entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().and_then(|s| s.to_str()) == Some("signal")).collect();
    files.sort_by_cached_key(|p| send_order(p));
    files
}

/// Enumerate the directories whose existing signals we should
/// backfill on startup: `#open`, every channel, every project's tray and
/// every person's tray. Kept separate from the event filter because
/// backfill walks the fs once up front — the filter runs per event.
fn scan_targets(base: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![base.join(attend_groups::BROADCAST_DIR)];
    let subdirs = |dir: &Path| -> Vec<(String, PathBuf)> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        entries.filter_map(|e| e.ok()).filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| (e.file_name().to_string_lossy().to_string(), e.path())).collect()
    };
    for (name, path) in subdirs(base) {
        if name == attend_groups::PEOPLE_DIR {
            dirs.extend(subdirs(&path).into_iter().map(|(_, p)| p));
        } else if name.starts_with('@') || is_tray(&name) {
            dirs.push(path);
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from("/tmp/fake-base")
    }

    fn accepts(rel: &[&str]) -> bool {
        accept_path(&base(), &rel.iter().fold(base(), |p, c| p.join(c)))
    }

    #[test]
    fn accepts_broadcast_and_channel_signals() {
        assert!(accepts(&["_broadcast", "x.signal"]));
        assert!(accepts(&["@deploy", "x.signal"]));
    }

    /// Every project's tray, the chat's own and any other: the person's
    /// direct messages to another project are on disk only there (#995).
    #[test]
    fn accepts_every_projects_tray() {
        assert!(accepts(&["-home-me", "x.signal"]));
        assert!(accepts(&["-home-someone-else", "x.signal"]));
        assert!(!accepts(&["-home-me", "nested", "x.signal"]), "a tray is flat");
    }

    #[test]
    fn accepts_every_persons_tray() {
        assert!(accepts(&["_people", "aaron", "x.signal"]));
        assert!(accepts(&["_people", "bob", "x.signal"]));
        assert!(!accepts(&["_people", "x.signal"]), "no person");
    }

    #[test]
    fn rejects_attends_own_dirs_and_files() {
        assert!(!accepts(&["_tombstones", "x.signal"]), "attend's own state");
        assert!(!accepts(&["x.signal"]), "at the base itself");
        assert!(!accepts(&["_broadcast", "readme.txt"]), "not a signal");
        assert!(!accepts(&["_groups.yaml"]));
    }

    #[test]
    fn rejects_path_outside_base() {
        assert!(!accept_path(&base(), &PathBuf::from("/etc/passwd")));
    }

    #[test]
    fn rejects_traversal_style_paths() {
        // Defence-in-depth: the filter sees full paths from notify, but a
        // `.signal`-suffixed traversal segment inside the base must not
        // read as a tray. `strip_prefix` + `Components` keep `..` as a
        // ParentDir component, which `is_tray` refuses.
        assert!(!accepts(&["..", "outside", "x.signal"]));
        assert!(!accepts(&["..", "x.signal"]));
    }

    fn feed_at(base: &Path) -> (Feed, std::sync::mpsc::Receiver<Delivery>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Feed::new(base, Own::default(), tx), rx)
    }

    fn write(dir: &Path, name: &str) -> PathBuf {
        write_from(dir, name, "abc")
    }

    fn write_from(dir: &Path, name: &str, sender: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{name}.signal"));
        std::fs::write(&p, format!("claude:{sender}|proj|/home/x|hi\n")).unwrap();
        p
    }

    /// Date the file at `path` `secs` from now, back for a negative count.
    fn date(path: &Path, secs: i64) {
        let at = if secs < 0 { SystemTime::now() - Duration::from_secs(secs.unsigned_abs()) } else { SystemTime::now() + Duration::from_secs(secs as u64) };
        std::fs::File::options().write(true).open(path).unwrap().set_modified(at).unwrap();
    }

    /// What the feed sent, each signal by its id.
    fn sent(rx: &std::sync::mpsc::Receiver<Delivery>) -> Vec<String> {
        rx.try_iter()
            .map(|d| match d {
                Delivery::Arrived(s) => s.id,
                Delivery::Restored(s) => format!("restored {}", s.id),
                Delivery::Withdrawn(files) => format!("withdrawn {}", files.iter().map(|(t, i)| format!("{t}/{i}")).collect::<Vec<_>>().join(" ")),
            })
            .collect()
    }

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| format!("{n}.signal")).collect()
    }

    /// A marked message is never sent; one marked after it was sent is
    /// withdrawn; an undo of either sends it again, as restored.
    #[test]
    fn a_marked_message_never_shows_and_an_undo_brings_it_back() {
        let base = crate::test_dir::unique("marks");
        let tray = base.join("-home-a");
        let (a, b) = (write(&tray, "a"), write(&tray, "b"));
        tombstones::mark_messages(&base, &keys(&["a"])).unwrap();
        let (feed, rx) = feed_at(&base);
        feed.deliver(a);
        feed.deliver(b);
        assert_eq!(sent(&rx), ["b"], "the marked message is withheld");

        tombstones::undo(&base, None).unwrap();
        tombstones::mark_messages(&base, &keys(&["b"])).unwrap();
        feed.reread_marks();
        assert_eq!(sent(&rx), ["withdrawn -home-a/b", "restored a"]);

        tombstones::undo(&base, None).unwrap();
        feed.reread_marks();
        assert_eq!(sent(&rx), ["restored b"]);
        feed.reread_marks();
        assert!(sent(&rx).is_empty(), "each once");
    }

    /// A tombstoned session's messages up to the tombstone are withheld;
    /// one sent after it, back under `--resume`, shows.
    #[test]
    fn a_tombstoned_senders_messages_hide_up_to_the_cut() {
        let base = crate::test_dir::unique("tomb");
        let dead = "22222222-2222-4222-8222-222222222222";
        let tray = base.join("_broadcast");
        let (before, after) = (write_from(&tray, "before", dead), write_from(&tray, "after", dead));
        date(&before, -60);
        date(&after, 60);
        let other = write(&tray, "other");
        date(&other, -60);
        tombstones::mark_session(&base, dead).unwrap();
        let (feed, rx) = feed_at(&base);
        for p in [before, other, after] {
            feed.deliver(p);
        }
        assert_eq!(sent(&rx), ["other", "after"]);

        tombstones::unmark_session(&base, dead).unwrap();
        feed.reread_marks();
        assert_eq!(sent(&rx), ["restored before"]);
    }

    /// A burst of changes to the mark store is read once it ends, however
    /// long it runs.
    #[test]
    fn the_mark_store_is_read_once_its_changes_stop() {
        let base = crate::test_dir::unique("settle");
        write(&base.join("_broadcast"), "a");
        let (feed, rx) = feed_at(&base);
        feed.deliver(base.join("_broadcast").join("a.signal"));
        assert_eq!(sent(&rx), ["a"]);
        tombstones::mark_messages(&base, &keys(&["a"])).unwrap();
        for _ in 0..10 {
            feed.marks_changed();
            thread::sleep(MARKS_SETTLE / 5);
            assert!(sent(&rx).is_empty(), "read during the burst");
        }
        thread::sleep(MARKS_SETTLE * 3);
        assert_eq!(sent(&rx), ["withdrawn _broadcast/a"]);
    }

    /// The watcher withholds marks at backfill, and a mark or an undo
    /// written while it runs reaches the chat through the watch.
    #[test]
    fn the_watch_follows_the_mark_store() {
        let base = crate::test_dir::unique("watch");
        let tray = base.join("_broadcast");
        write(&tray, "kept");
        write(&tray, "gone");
        tombstones::mark_messages(&base, &keys(&["gone"])).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_watcher(base.to_path_buf(), String::new(), String::new(), tx).unwrap();
        assert_eq!(sent(&rx), ["kept"], "the backfill withholds the mark");

        let wait = |want: &str| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut got = Vec::new();
            while std::time::Instant::now() < deadline && !got.iter().any(|g| g == want) {
                got.extend(sent(&rx));
                thread::sleep(Duration::from_millis(20));
            }
            got
        };
        tombstones::undo(&base, None).unwrap();
        assert_eq!(wait("restored gone"), ["restored gone"]);
        tombstones::mark_messages(&base, &keys(&["kept"])).unwrap();
        assert_eq!(wait("withdrawn _broadcast/kept"), ["withdrawn _broadcast/kept"]);
    }

    /// A file delivered once is not delivered again; gone, it leaves the
    /// record, and a dir gone takes what was under it.
    #[test]
    fn the_record_of_what_was_delivered_follows_the_disk() {
        let base = crate::test_dir::unique("feed");
        let (feed, rx) = feed_at(&base);
        let a = write(&base.join("-home-a"), "a");
        let b = write(&base.join("-home-b"), "b");
        feed.deliver(a.clone());
        feed.deliver(a.clone());
        feed.deliver(b.clone());
        assert_eq!(rx.try_iter().count(), 2, "each once");
        feed.forget(&a);
        feed.forget(&base.join("-home-b"));
        assert!(feed.seen().read.is_empty());
    }

    /// `_people/` made after the chat started, with a person's tray made in
    /// it before its watch is in place: the new dir's read reaches the tray.
    #[test]
    fn a_new_people_dir_is_read_with_the_trays_in_it() {
        let base = crate::test_dir::unique("feed");
        let (feed, rx) = feed_at(&base);
        write(&base.join("_people").join("bob"), "to-bob");
        feed.read_dir_tree(&base.join("_people"));
        assert_eq!(sent(&rx), ["to-bob"]);
    }

    /// The backfill walks what the filter accepts: `#open`, channels,
    /// project trays and person trays, never attend's own `_` dirs.
    #[test]
    fn the_backfill_scans_every_tray() {
        let base = crate::test_dir::unique("scan");
        for d in ["_broadcast", "@deploy", "-home-me", "-home-other", "_people/aaron", "_people/bob", "_tombstones"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("_groups.yaml"), "").unwrap();
        let mut got: Vec<String> = scan_targets(&base).iter().map(|p| p.strip_prefix(&base).unwrap().to_string_lossy().to_string()).collect();
        got.sort();
        assert_eq!(got, ["-home-me", "-home-other", "@deploy", "_broadcast", "_people/aaron", "_people/bob"]);
    }
}
