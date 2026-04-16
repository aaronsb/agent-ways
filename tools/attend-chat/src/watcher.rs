//! Directory watcher that streams signals into the TUI.
//!
//! `spawn_watcher` does three things, in order:
//!
//!   1. scans the directory for existing `.signal` files (sorted by
//!      mtime ascending) so a freshly-started TUI catches up on recent
//!      traffic instead of opening empty;
//!   2. installs a `notify` recommended watcher; creates and writes
//!      are forwarded as [`Signal`] values onto the caller's channel;
//!   3. parks the watcher on a dedicated thread so its lifetime is
//!      tied to the process, not to any caller scope.
//!
//! `notify`'s callback is synchronous, so we bridge to the async
//! channel with [`async_channel::Sender::send_blocking`]. Keeping the
//! watcher off smol means a slow renderer can't back-pressure the
//! filesystem listener into dropping events.
//!
//! Initialisation failures (watcher construction, `.watch()`) are
//! returned to the caller so it can decide whether to fall back or
//! abort — post-init errors from inside the callback are best-effort
//! and swallowed, because surfacing them into the TUI would require a
//! status channel we don't need yet.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use async_channel::Sender;
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::signal::{parse_file, Signal};

pub fn spawn_watcher(dir: PathBuf, tx: Sender<Signal>) -> notify::Result<()> {
    std::fs::create_dir_all(&dir).ok();

    // Backfill existing signals so the stream isn't empty on launch.
    // Ordered by mtime so replay matches wall-clock arrival order.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut items: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                e.metadata().and_then(|m| m.modified()).ok().map(|t| (t, p))
            })
            .collect();
        items.sort_by_key(|(t, _)| *t);
        for (_, path) in items {
            if let Some(sig) = parse_file(&path) {
                let _ = tx.send_blocking(sig);
            }
        }
    }

    let tx_cb = tx.clone();
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            // An atomic rename fires two events — `Name(To)` and
            // `Name(Both)` — both referencing the final filename. We
            // accept `Create(_)` (platforms that emit it, notably
            // kqueue) and `Modify(Name(To))` (the rename-destination
            // arrival on Linux), and explicitly drop `Name(Both)` and
            // `Name(From)` so we don't deliver each signal twice.
            //
            // TODO(bsd): kqueue reports bare `Modify(_)` rather than
            // the RenameMode subvariants on some BSD builds of notify;
            // if we start running on FreeBSD this matcher may miss
            // events. Revisit with a platform-specific test when we
            // add BSD CI.
            let accept = matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To))
            );
            if !accept {
                return;
            }
            for path in event.paths {
                if path.extension().and_then(|s| s.to_str()) != Some("signal") {
                    continue;
                }
                if let Some(sig) = parse_file(&path) {
                    let _ = tx_cb.send_blocking(sig);
                }
            }
        },
        Config::default().with_poll_interval(Duration::from_millis(250)),
    )?;
    watcher.watch(&dir, RecursiveMode::NonRecursive)?;

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
