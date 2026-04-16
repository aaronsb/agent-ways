//! End-to-end smoke test for the watcher → channel → parse path.
//!
//! This is the bridge between the writer in `signal::write_broadcast`
//! and the reader in `watcher::spawn_watcher`; it's the one thing that
//! would catch wire-format drift between the two without a human
//! reading both files. We keep everything inside a temp `$HOME` so
//! the real signal cache stays untouched.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use attend_chat::{signal, watcher};

fn tmp_home() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "attend-chat-it-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn recv_with_timeout(
    rx: &async_channel::Receiver<signal::Signal>,
    timeout: Duration,
) -> Option<signal::Signal> {
    let deadline = Instant::now() + timeout;
    loop {
        match rx.try_recv() {
            Ok(sig) => return Some(sig),
            Err(async_channel::TryRecvError::Empty) => {
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(async_channel::TryRecvError::Closed) => return None,
        }
    }
}

#[test]
fn write_broadcast_arrives_through_watcher() {
    let home = tmp_home();
    // Using `set_var` is !Send on some platforms; cargo runs
    // integration tests per file, and we only flip `HOME` here.
    std::env::set_var("HOME", &home);

    let (tx, rx) = async_channel::unbounded::<signal::Signal>();
    watcher::spawn_watcher(signal::broadcast_dir(), tx)
        .expect("watcher must initialise against a fresh temp dir");

    // Drain anything the backfill produced (should be nothing in a
    // fresh temp dir, but be defensive).
    while rx.try_recv().is_ok() {}

    let filename = signal::write_broadcast("hello integration test")
        .expect("write_broadcast must succeed");

    let sig = recv_with_timeout(&rx, Duration::from_secs(2))
        .expect("watcher must surface the write within 2s");
    assert_eq!(sig.message, "hello integration test");
    assert!(sig.from.starts_with("external:"));
    assert!(filename.ends_with(".signal"));
    assert!(sig.id.starts_with(sig.from.trim_start_matches("external:").split('@').next().unwrap_or("")));
}
