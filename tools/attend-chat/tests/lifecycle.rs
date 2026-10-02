//! The real binary's lifecycle. `agent-tui` never uninstalls its signal
//! handler, so the chat must end its process when its screen closes, and
//! must not take a terminal it does not have: run under a Monitor or a
//! pipe, it says so and exits before installing anything. Each child runs
//! in a fixture home; nothing here touches the real attend cache.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn home(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("attend-chat-life-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run the binary in a fixture home with no terminal on any stream,
/// failing if it has not exited within ten seconds.
fn run(tag: &str, args: &[&str]) -> Output {
    let h = home(tag);
    let mut child = Command::new(env!("CARGO_BIN_EXE_attend-chat"))
        .args(args)
        .env("HOME", &h)
        .env("XDG_CONFIG_HOME", h.join("config"))
        .env("XDG_CACHE_HOME", h.join("cache"))
        .env("XDG_STATE_HOME", h.join("state"))
        .env("XDG_DATA_HOME", h.join("data"))
        .env("USER", "tester")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    let start = Instant::now();
    while child.try_wait().expect("a status").is_none() {
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("attend-chat {args:?} did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().expect("its output");
    let _ = std::fs::remove_dir_all(&h);
    out
}

#[test]
fn without_a_terminal_it_refuses_and_exits() {
    let out = run("monitor", &[]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("needs a terminal"), "{err}");
    assert!(out.stdout.is_empty(), "no escape reaches a pipe");
}

#[test]
fn a_snapshot_prints_one_frame_and_the_process_ends() {
    let out = run("snap", &["--snap", "80x25", "--depth", "none", "--keys", "text:/he tab"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let frame = String::from_utf8_lossy(&out.stdout);
    assert!(frame.starts_with("agent-tui frame 80x25\n"), "{frame}");
    assert!(frame.contains("> /help "), "the keys went through the real handler: {frame}");
    assert!(!frame.contains('\u{1b}'), "a frame is text, not escapes");
}

#[test]
fn bad_arguments_are_a_usage_error() {
    assert_eq!(run("usage", &["--snap", "80"]).status.code(), Some(2));
    assert_eq!(run("usage2", &["--keys", "a"]).status.code(), Some(2));
    assert_eq!(run("usage3", &["--bogus"]).status.code(), Some(2));
}

/// The binary on a pty of its own: it draws, and ends its process on Esc,
/// on SIGTERM with 128 plus the signal, and when its terminal hangs up,
/// putting the terminal back each time it still can.
#[cfg(target_os = "linux")]
mod on_a_terminal {
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    struct Pty {
        child: Child,
        master: Option<std::fs::File>,
        seen: Arc<Mutex<Vec<u8>>>,
        /// Stops the reader, which holds a handle on the master too.
        stop: Arc<AtomicBool>,
        reader: Option<std::thread::JoinHandle<()>>,
        home: std::path::PathBuf,
    }

    fn start(tag: &str) -> Pty {
        let home = super::home(tag);
        let (mut m, mut s) = (0, 0);
        let ws = libc::winsize { ws_row: 25, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
        // SAFETY: openpty fills two descriptors we then own; the master is
        // kept from the child so dropping ours hangs the terminal up.
        let (master, slave) = unsafe {
            assert_eq!(libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws), 0);
            libc::fcntl(m, libc::F_SETFD, libc::FD_CLOEXEC);
            (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s))
        };
        let child = Command::new(env!("CARGO_BIN_EXE_attend-chat"))
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_DATA_HOME", home.join("data"))
            .env("USER", "tester")
            .env("TERM", "xterm-256color")
            .env_remove("NO_COLOR")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .spawn()
            .expect("the binary starts");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut reader = std::fs::File::from(master.try_clone().unwrap());
        let sink = seen.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let halt = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut b = [0u8; 4096];
            while !halt.load(Ordering::SeqCst) {
                let mut pfd = libc::pollfd { fd: reader.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                // SAFETY: one pollfd we own.
                if unsafe { libc::poll(&mut pfd, 1, 50) } > 0 {
                    match reader.read(&mut b) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => sink.lock().unwrap().extend_from_slice(&b[..n]),
                    }
                }
            }
        });
        let pty = Pty { child, master: Some(std::fs::File::from(master)), seen, stop, reader: Some(handle), home };
        pty.wait_for(b"merged");
        pty
    }

    impl Pty {
        fn has(&self, needle: &[u8]) -> bool {
            self.seen.lock().unwrap().windows(needle.len()).any(|w| w == needle)
        }

        fn wait_for(&self, needle: &[u8]) {
            let start = Instant::now();
            while !self.has(needle) {
                assert!(start.elapsed() < Duration::from_secs(10), "the screen never showed {:?}", String::from_utf8_lossy(needle));
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        /// The exit code, failing if the process still runs after ten seconds.
        fn exit_code(&mut self) -> Option<i32> {
            let start = Instant::now();
            loop {
                if let Some(st) = self.child.try_wait().unwrap() {
                    let _ = std::fs::remove_dir_all(&self.home);
                    return st.code();
                }
                if start.elapsed() > Duration::from_secs(10) {
                    let _ = self.child.kill();
                    panic!("attend-chat kept running after its screen should have closed");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        /// Whether the alternate screen was left: the terminal put back.
        fn restored(&self) -> bool {
            let mut leave = vec![27u8];
            leave.extend_from_slice(b"[?1049l");
            self.has(&leave)
        }
    }

    #[test]
    fn esc_closes_the_screen_and_ends_the_process() {
        let mut p = start("esc");
        p.master.as_mut().unwrap().write_all(&[27]).unwrap();
        assert_eq!(p.exit_code(), Some(0));
        std::thread::sleep(Duration::from_millis(100));
        assert!(p.restored());
    }

    #[test]
    fn a_termination_signal_ends_it_with_128_plus_the_signal() {
        let mut p = start("term");
        // SAFETY: a signal to our own child.
        unsafe { libc::kill(p.child.id() as i32, libc::SIGTERM) };
        assert_eq!(p.exit_code(), Some(128 + libc::SIGTERM));
        std::thread::sleep(Duration::from_millis(100));
        assert!(p.restored());
    }

    #[test]
    fn a_hung_up_terminal_ends_it() {
        let mut p = start("hup");
        // Every handle on the master goes: the reader's, then ours.
        p.stop.store(true, Ordering::SeqCst);
        p.reader.take().unwrap().join().unwrap();
        drop(p.master.take());
        assert_eq!(p.exit_code(), Some(128 + libc::SIGHUP));
    }
}
