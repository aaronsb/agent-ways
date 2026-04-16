//! Directory watcher that streams signals into the TUI.
//!
//! `spawn_watcher` does two things, in order:
//!
//!   1. scans the directory for existing `.signal` files (sorted by
//!      mtime ascending) so a freshly-started TUI catches up on recent
//!      traffic instead of opening empty;
//!   2. installs a `notify` recommended watcher; creates and writes
//!      are forwarded as [`Signal`] values onto the caller's channel.
//!
//! The watcher runs in a dedicated thread. `notify`'s callback is
//! synchronous, so we bridge to the async channel with
//! [`async_channel::Sender::send_blocking`]. Keeping the watcher
//! thread off smol means a slow renderer can't back-pressure the
//! filesystem listener into dropping events.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use async_channel::Sender;
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::signal::{parse_file, Signal};

pub fn spawn_watcher(dir: PathBuf, tx: Sender<Signal>) {
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

    thread::spawn(move || {
        let tx_cb = tx.clone();
        let watch_dir = dir.clone();
        let mut watcher: RecommendedWatcher = match RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                let Ok(event) = res else { return };
                // An atomic rename fires two events — `Name(To)` and
                // `Name(Both)` — both referencing the final filename.
                // We accept `Create(_)` (platforms that emit it) and
                // `Modify(Name(To))` (the rename-destination arrival
                // on Linux), and explicitly drop `Name(Both)` and
                // `Name(From)` so we don't deliver each signal twice.
                let accept = matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Modify(ModifyKind::Name(RenameMode::To))
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
        ) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("attend-chat: watcher init failed: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            eprintln!(
                "attend-chat: watch({}) failed: {}",
                watch_dir.display(),
                e
            );
            return;
        }

        // Keep the watcher alive for the life of the thread.
        loop {
            thread::park();
        }
    });
}

/// Re-export so the caller doesn't need to import `Path`.
#[allow(dead_code)]
pub fn is_signal(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("signal")
}
