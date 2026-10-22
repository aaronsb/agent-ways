//! A fresh directory per test. The pid and clock alone collide when two
//! parallel tests start in one clock tick (#788), so a per-process counter
//! makes each name unique. And the one lock for the environment: unit tests
//! run in parallel threads of one process, which share `HOME` and the XDG
//! variables.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A test directory that removes itself, contents and all, when dropped.
/// Bind it to a name that lives as long as the test, and as long as anything
/// that was handed the path: `let _ = unique(..)` drops it on the spot.
pub(crate) struct TestDir(PathBuf);

impl std::ops::Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Create an empty directory under the fixture temp dir (never under a
/// managed worktree, #902), named for `tag`, removed when the returned guard
/// drops.
pub(crate) fn unique(tag: &str) -> TestDir {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let p = attend_presence::session::fixture_temp_dir().join(format!(
        "attend-chat-{tag}-test-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    TestDir(p)
}

/// Hold for the whole body of a test that sets `HOME` or an `XDG_*`
/// variable, or reads a path derived from them, so another test cannot
/// change the environment between its reads. A test that panicked while
/// holding it leaves the lock usable.
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
