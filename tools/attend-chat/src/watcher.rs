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
//! `notify`'s callback runs on its own thread and sends into an
//! unbounded [`std::sync::mpsc`] channel, which the screen drains
//! between frames. Unbounded means a slow renderer can't back-pressure
//! the filesystem listener into dropping events.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use std::sync::mpsc::Sender;
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::signal::{parse_file, Own, Signal};

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
pub fn spawn_watcher(base: PathBuf, own_tray: String, person: String, tx: Sender<Signal>) -> notify::Result<()> {
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
    // isn't empty on launch. Ordered by mtime across all
    // source dirs so replay matches wall-clock arrival order.
    let mut backfill: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for dir in scan_targets(&base) {
        for p in signal_files(&dir) {
            if let Ok(ts) = p.metadata().and_then(|m| m.modified()) {
                backfill.push((ts, p));
            }
        }
    }
    // Ties on mtime (the parts of a long message, written within one
    // timestamp tick) go by the send time the id carries.
    backfill.sort_by_cached_key(|(t, p)| {
        let id = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        (*t, agent_identity::signal_send_order(id), p.clone())
    });
    let own = Own { tray: own_tray, person };
    let mut backfill_paths = HashSet::new();
    for (_, path) in backfill {
        if let Some(sig) = parse_file(&path, &own) {
            let _ = tx.send(sig);
        }
        backfill_paths.insert(path);
    }

    let feed = Feed { base: base.clone(), own, tx: tx.clone(), delivered: Arc::new(Mutex::new(backfill_paths)) };
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
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
/// sender the chat drains, and every path delivered, so a file the watch reports
/// twice, or one both a new dir's scan and its own event find, is
/// delivered once.
#[derive(Clone)]
struct Feed {
    base: PathBuf,
    own: Own,
    tx: Sender<Signal>,
    delivered: Arc<Mutex<HashSet<PathBuf>>>,
}

/// How long after a dir is made its contents are read again, by when the
/// watch on it is in place.
const NEW_DIR_SETTLE: Duration = Duration::from_millis(250);

impl Feed {
    /// Send the signal at `path` on, unless it is not one the chat reads or
    /// was sent before.
    fn deliver(&self, path: PathBuf) {
        if !accept_path(&self.base, &path) {
            return;
        }
        let mut delivered = self.delivered.lock().unwrap_or_else(|e| e.into_inner());
        if delivered.contains(&path) {
            return;
        }
        if let Some(sig) = parse_file(&path, &self.own) {
            delivered.insert(path);
            let _ = self.tx.send(sig);
        }
    }

    /// A dir made under the base: a new tray or channel. notify reports it
    /// before it adds the watch on it, so a signal written into it at once
    /// (the first direct message to a project: `mkdir`, then the write)
    /// raises no event of its own. Read what it holds now, and again once
    /// the watch is in place.
    fn new_dir(&self, dir: PathBuf) {
        for file in signal_files(&dir) {
            self.deliver(file);
        }
        let feed = self.clone();
        thread::spawn(move || {
            thread::sleep(NEW_DIR_SETTLE);
            for file in signal_files(&dir) {
                feed.deliver(file);
            }
        });
    }
}

/// The `.signal` files in `dir`, oldest first, ties by the send time the
/// id carries.
fn signal_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("signal"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_cached_key(|(t, p)| (*t, agent_identity::signal_send_order(p.file_name().and_then(|n| n.to_str()).unwrap_or("")), p.clone()));
    files.into_iter().map(|(_, p)| p).collect()
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
        let _ = std::fs::remove_dir_all(&base);
    }
}
