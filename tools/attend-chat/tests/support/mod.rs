//! A fixture home for a test binary that wants one of its own: `HOME` and
//! the XDG directories point into a temp directory set up once, before any
//! test reads them, and removed when the process exits. A test whose
//! frames must not move with other tests' writes lives in its own file,
//! which cargo runs as its own process with its own fixture.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// The fixture's root, made on first call, with `setup` run against the
/// signals base once the environment points at it.
pub fn fixture(name: &str, setup: impl FnOnce(&Path)) -> &'static Path {
    let mut setup = Some(setup);
    ROOT.get_or_init(|| {
        let root = attend_presence::session::fixture_temp_dir().join(format!("attend-chat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
        std::env::set_var("XDG_STATE_HOME", root.join("state"));
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        std::env::set_var("USER", "aaron");
        remove_at_exit();
        let base = attend_chat::signal::signals_base();
        std::fs::create_dir_all(&base).unwrap();
        (setup.take().expect("set up once"))(&base);
        root
    })
}

/// Keep `session` live for the whole run: its heartbeat is dated an hour
/// ahead, which liveness reads as fresh however long the run takes. A test
/// binary with no live session (an empty bus) leaves it unused.
#[allow(dead_code)]
pub fn pin_alive(session: &str) {
    attend_presence::heartbeat::touch(session).unwrap();
    let file = std::fs::File::options().write(true).open(attend_presence::heartbeat::heartbeat_path(session)).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(3600)).unwrap();
}

#[cfg(unix)]
fn remove_at_exit() {
    extern "C" fn remove() {
        if let Some(root) = ROOT.get() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
    // SAFETY: `remove` is a plain function with no arguments, as atexit
    // takes; it touches only the static set before the process exits.
    unsafe {
        libc::atexit(remove);
    }
}

#[cfg(not(unix))]
fn remove_at_exit() {}
