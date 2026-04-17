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
//! The watcher is recursive because focus-group dirs (`@name/`) and
//! the directed-send inbox for this host's own cwd both live as
//! sibling directories under the base. A non-recursive watcher would
//! miss both — and would also miss *new* groups created after the
//! TUI started, which is the common case (another agent runs
//! `attend focus on deploy` during the session).
//!
//! `notify`'s callback is synchronous, so we bridge to the async
//! channel with [`async_channel::Sender::send_blocking`]. Keeping the
//! watcher off smol means a slow renderer can't back-pressure the
//! filesystem listener into dropping events.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use async_channel::Sender;
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::signal::{parse_file, Signal};

/// Decide whether a filesystem path under the signals base is a
/// signal we want to stream into the TUI.
///
/// Accepts three shapes:
/// - `<base>/_broadcast/<name>.signal` — everyone
/// - `<base>/@<group>/<name>.signal` — any focus group (we render
///   them all; filtering by *subscribed* groups happens later in
///   the component as the membership model lands)
/// - `<base>/<own_encoded>/<name>.signal` — direct sends targeting
///   this host's cwd (produced by PR 2's `@nickname` routing)
///
/// Rejects:
/// - Directed sends to *other* cwds (noise — not addressed to us)
/// - Non-`.signal` files (state files like `_groups.yaml`, tmp files)
pub fn accept_path(base: &Path, own_encoded: &str, path: &Path) -> bool {
    if path.extension().and_then(|s| s.to_str()) != Some("signal") {
        return false;
    }
    let Ok(rel) = path.strip_prefix(base) else {
        return false;
    };
    let mut comps = rel.components();
    let Some(first) = comps.next() else {
        return false;
    };
    let name = first.as_os_str().to_string_lossy();
    if name == "_broadcast" {
        return true;
    }
    if name.starts_with('@') {
        return true;
    }
    if name == own_encoded {
        return true;
    }
    false
}

pub fn spawn_watcher(base: PathBuf, own_encoded: String, tx: Sender<Signal>) -> notify::Result<()> {
    std::fs::create_dir_all(&base).ok();
    // Pre-create the broadcast + own-cwd subdirs so notify's
    // recursive watch installs sub-watchers on them before any
    // signal is written. Without this, a write that races with
    // create_dir inside `write_signal` can fire its filesystem
    // event before the recursive watcher has observed the parent —
    // the signal arrives on disk but the watcher callback never
    // sees it. Group dirs don't need the same treatment: they're
    // rare, and any missed "first signal in a new group" is
    // indistinguishable from backlog a later signal will force the
    // UI to re-scan for.
    std::fs::create_dir_all(base.join("_broadcast")).ok();
    if !own_encoded.is_empty() {
        std::fs::create_dir_all(base.join(&own_encoded)).ok();
    }

    // Backfill existing signals from the three accepted shapes so
    // the stream isn't empty on launch. Ordered by mtime across all
    // source dirs so replay matches wall-clock arrival order.
    let mut backfill: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let dirs_to_scan = scan_targets(&base, &own_encoded);
    for dir in &dirs_to_scan {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("signal") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(ts) = meta.modified() {
                    backfill.push((ts, p));
                }
            }
        }
    }
    backfill.sort_by_key(|(t, _)| *t);
    for (_, path) in backfill {
        if let Some(sig) = parse_file(&path) {
            let _ = tx.send_blocking(sig);
        }
    }

    let tx_cb = tx.clone();
    let base_for_cb = base.clone();
    let own_for_cb = own_encoded.clone();
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
                if !accept_path(&base_for_cb, &own_for_cb, &path) {
                    continue;
                }
                if let Some(sig) = parse_file(&path) {
                    let _ = tx_cb.send_blocking(sig);
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

/// Enumerate the directories whose existing signals we should
/// backfill on startup. Kept separate from the event filter because
/// backfill walks the fs once up front — the filter runs per event.
fn scan_targets(base: &Path, own_encoded: &str) -> Vec<PathBuf> {
    let mut dirs = vec![base.join("_broadcast")];
    // Mirror the guard in `spawn_watcher`: when `env::current_dir()`
    // fails, `own_encoded` is empty and `base.join("")` degenerates
    // to `base` — we'd then walk the whole top level for no good
    // reason. Skip that dir when there's nothing to watch.
    if !own_encoded.is_empty() {
        dirs.push(base.join(own_encoded));
    }
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('@') {
                dirs.push(entry.path());
            }
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

    #[test]
    fn accepts_broadcast_signal() {
        let p = base().join("_broadcast").join("x.signal");
        assert!(accept_path(&base(), "-home-me", &p));
    }

    #[test]
    fn accepts_group_signal() {
        let p = base().join("@deploy").join("x.signal");
        assert!(accept_path(&base(), "-home-me", &p));
    }

    #[test]
    fn accepts_own_cwd_signal() {
        let p = base().join("-home-me").join("x.signal");
        assert!(accept_path(&base(), "-home-me", &p));
    }

    #[test]
    fn rejects_other_cwd_signal() {
        // Directed to a different agent's cwd — not for us.
        let p = base().join("-home-someone-else").join("x.signal");
        assert!(!accept_path(&base(), "-home-me", &p));
    }

    #[test]
    fn rejects_non_signal_file() {
        let p = base().join("_broadcast").join("readme.txt");
        assert!(!accept_path(&base(), "-home-me", &p));
    }

    #[test]
    fn rejects_groups_yaml() {
        let p = base().join("_groups.yaml");
        assert!(!accept_path(&base(), "-home-me", &p));
    }

    #[test]
    fn rejects_path_outside_base() {
        let p = PathBuf::from("/etc/passwd");
        assert!(!accept_path(&base(), "-home-me", &p));
    }

    #[test]
    fn rejects_traversal_style_paths() {
        // Defence-in-depth: the filter sees full paths from notify,
        // but a `.signal`-suffixed traversal segment inside the base
        // must not appear to be broadcast/group/own. We rely on
        // `strip_prefix` + `Components` for safety, which normalizes
        // `..` as a ParentDir component that can't equal `_broadcast`,
        // `@<name>`, or our own_encoded.
        let p = base().join("..").join("outside").join("x.signal");
        assert!(!accept_path(&base(), "-home-me", &p));
    }
}
