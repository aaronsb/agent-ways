//! A fresh directory per test. The pid and clock alone collide when two
//! parallel tests start in one clock tick (#788), so a per-process counter
//! makes each name unique.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Create and return an empty directory under the temp dir, named for `tag`.
pub(crate) fn unique(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let p = std::env::temp_dir().join(format!(
        "attend-chat-{tag}-test-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
