//! The settings screens on a real terminal: a pty, the real binary, and an
//! apply that waits on a slow command whose stand-in starts a process of
//! its own and waits on it, as `ways agent key check` waits on a provider.
//! Then the session ends the two hard ways: a termination signal, and the
//! terminal going away. Each time the process must exit within seconds,
//! the command and the process it started must be gone, and after a signal
//! the terminal must be as the shell left it (main screen, cursor shown,
//! mouse reporting off, line mode and echo on).

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A pty of `cols` by `rows`: (master, slave).
fn openpty(cols: u16, rows: u16) -> (OwnedFd, OwnedFd) {
    let (mut m, mut s) = (0, 0);
    let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: openpty fills two descriptors we then own.
    let r = unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) };
    assert_eq!(r, 0, "openpty: {}", std::io::Error::last_os_error());
    // The screens must not inherit the master, or closing ours would not
    // hang the terminal up.
    // SAFETY: setting a flag on a descriptor we own.
    unsafe { libc::fcntl(m, libc::F_SETFD, libc::FD_CLOEXEC) };
    // SAFETY: both are fresh descriptors from openpty.
    unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) }
}

fn esc(seq: &str) -> String {
    format!("{}{seq}", '\u{1b}')
}

fn alive(pid: i32) -> bool {
    // A zombie still answers signal 0; read its state to tell.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => !stat.split_whitespace().nth(2).is_some_and(|s| s == "Z"),
        Err(_) => false,
    }
}

/// `ways settings install` on a pty, an apply started that waits on the
/// stand-in, and the pid of the process the stand-in started.
struct Screens {
    root: PathBuf,
    master: Option<OwnedFd>,
    slave: Option<OwnedFd>,
    child: Child,
    out: Arc<Mutex<Vec<u8>>>,
    /// Tells the reader to let go of its handle on the master.
    stop: Arc<std::sync::atomic::AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
    grandchild: i32,
    runner: i32,
}

impl Screens {
    fn start(tag: &str) -> Screens {
        let mut s = Screens::open(tag);
        // The targets row's menu, add, a directory, confirmed, then review
        // and apply. (Plan only reads: it runs at once and is never queued.)
        for k in ["a", "j", "\r", "/tmp/x", "\r", "y", "w", "a"] {
            s.press(k);
        }
        s.wait_for("applying");
        s.grandchild = s.pid("sleep.pid");
        s.runner = s.pid("runner.pid");
        assert!(alive(s.grandchild) && alive(s.runner));
        s
    }

    /// The screens open and idle on the install tab.
    fn open(tag: &str) -> Screens {
        let root = std::env::temp_dir().join(format!("ways-settings-pty-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        for d in [".config", "proj", ".claude", ".local/share", ".local/state", ".cache"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        // The stand-in does not exec: it starts a process and waits on it.
        let runner = root.join("runner.sh");
        let script = format!(
            "#!/bin/sh\necho $$ > {r}\nsleep 45 &\necho $! > {g}\nwait\n",
            r = root.join("runner.pid").display(),
            g = root.join("sleep.pid").display()
        );
        std::fs::write(&runner, script).unwrap();
        std::fs::set_permissions(&runner, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let (master, slave) = openpty(100, 30);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
        cmd.args(["settings", "install"])
            .current_dir(home.join("proj"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TERM", "xterm-256color")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("CLAUDE_PROJECT_DIR", home.join("proj"))
            .env("WAYS_SETTINGS_RUNNER", &runner)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()));
        // SAFETY: setsid and TIOCSCTTY are async-signal-safe; the pty becomes
        // the child's controlling terminal, as a shell's would be.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY, 0);
                Ok(())
            });
        }
        let child = cmd.spawn().unwrap();
        let out = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut file = std::fs::File::from(master.try_clone().unwrap());
        let (sink, halt) = (out.clone(), stop.clone());
        // Polled, so the reader can be told to close its handle: a hangup
        // needs every handle on the master closed.
        let reader = std::thread::spawn(move || {
            let mut b = [0u8; 4096];
            while !halt.load(std::sync::atomic::Ordering::SeqCst) {
                let mut pfd = libc::pollfd { fd: file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                // SAFETY: one pollfd we own.
                if unsafe { libc::poll(&mut pfd, 1, 50) } <= 0 {
                    continue;
                }
                match file.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => sink.lock().unwrap().extend_from_slice(&b[..n]),
                }
            }
        });
        let s = Screens { root, master: Some(master), slave: Some(slave), child, out, stop, reader: Some(reader), grandchild: 0, runner: 0 };
        s.wait_for("targets");
        s
    }

    /// The terminal goes away: the reader lets go and every handle on the
    /// master closes, and with `slave` the test's handle on that side too,
    /// so the screens' stderr is dead as well.
    fn hang_up(&mut self, slave: bool) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = self.reader.take().map(|r| r.join());
        drop(self.master.take());
        if slave {
            drop(self.slave.take());
        }
    }

    /// The pty is in line mode with echo and signals, as a shell leaves it.
    fn assert_line_mode(&self) {
        // SAFETY: termios is plain data the call fills.
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::tcgetattr(self.slave.as_ref().unwrap().as_raw_fd(), &mut t) }, 0);
        assert!(t.c_lflag & libc::ICANON != 0 && t.c_lflag & libc::ECHO != 0 && t.c_lflag & libc::ISIG != 0, "the pty is left in raw mode");
    }

    /// The escapes after the screens last took the alternate screen put the
    /// terminal back.
    fn assert_reset(&self) {
        let text = self.text();
        let after = &text[text.rfind(&esc("[?1049h")).expect("the screens took the alternate screen")..];
        for (seq, what) in [("[?1049l", "the main screen"), ("[?25h", "the cursor"), ("[?1000l", "mouse reporting off"), ("[?1003l", "motion reporting off")] {
            assert!(after.contains(&esc(seq)), "{what} is not restored: {after:?}");
        }
    }

    fn press(&self, k: &str) {
        let mut f = std::fs::File::from(self.master.as_ref().unwrap().try_clone().unwrap());
        f.write_all(k.as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.out.lock().unwrap()).into_owned()
    }

    fn wait_for(&self, needle: &str) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if self.text().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("`{needle}` never appeared:\n{}", self.text());
    }

    fn pid(&self, file: &str) -> i32 {
        let p: &Path = &self.root.join(file);
        let start = Instant::now();
        while !p.exists() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::read_to_string(p).unwrap_or_default().trim().parse().unwrap_or_else(|_| panic!("{file} was not written"))
    }

    /// Wait for the screens to exit, at most `secs`.
    fn exit_within(&mut self, secs: u64) -> (ExitStatus, Duration) {
        let start = Instant::now();
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                return (s, start.elapsed());
            }
            if start.elapsed() > Duration::from_secs(secs) {
                let pid = self.child.id();
                let threads: Vec<String> = std::fs::read_dir(format!("/proc/{pid}/task"))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|t| {
                        let stat = std::fs::read_to_string(t.path().join("stat")).unwrap_or_default();
                        let wchan = std::fs::read_to_string(t.path().join("wchan")).unwrap_or_default();
                        format!("{} {wchan}", stat.split(") ").nth(1).unwrap_or("").split_whitespace().take(1).collect::<String>())
                    })
                    .collect();
                let _ = self.child.kill();
                panic!("the screens did not exit within {secs} s; threads {threads:?}; signal mask {}", std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default().lines().filter(|l| l.starts_with("Sig")).collect::<Vec<_>>().join(" | "));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The command and the process it started are both gone.
    fn assert_jobs_gone(&self) {
        let start = Instant::now();
        while (alive(self.runner) || alive(self.grandchild)) && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(self.runner), "the command was left running");
        assert!(!alive(self.grandchild), "the process the command started was left running");
    }
}

impl Drop for Screens {
    fn drop(&mut self) {
        for pid in [self.grandchild, self.runner] {
            if pid > 0 {
                // SAFETY: cleanup of our own test processes.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
        let _ = self.child.kill();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_signal_during_an_apply_restores_the_terminal_and_ends_the_command_and_its_children() {
    let mut s = Screens::start("term");
    // SAFETY: a signal to our own child.
    unsafe { libc::kill(s.child.id() as i32, libc::SIGTERM) };
    let (status, took) = s.exit_within(5);
    assert_eq!(status.code(), Some(128 + libc::SIGTERM), "exit is 128 plus the signal");
    assert!(took < Duration::from_secs(2), "the stop waited on the command's children: {took:?}");
    std::thread::sleep(Duration::from_millis(200));
    s.assert_reset();
    s.assert_line_mode();
    s.assert_jobs_gone();
}

#[test]
fn two_different_signals_end_the_command_and_restore_the_terminal() {
    for _ in 0..3 {
        let mut s = Screens::start("two");
        // SAFETY: signals to our own child, back to back, so the second
        // arrives before the loop takes up the first.
        unsafe {
            libc::kill(s.child.id() as i32, libc::SIGTERM);
            libc::kill(s.child.id() as i32, libc::SIGINT);
        }
        let (status, _) = s.exit_within(5);
        assert!(matches!(status.code(), Some(c) if c == 128 + libc::SIGTERM || c == 128 + libc::SIGINT), "{status:?}");
        std::thread::sleep(Duration::from_millis(200));
        s.assert_jobs_gone();
        s.assert_reset();
        s.assert_line_mode();
    }
}

#[test]
fn a_hangup_with_stderr_gone_too_exits_129_without_a_core() {
    for _ in 0..5 {
        let mut s = Screens::open("dead");
        s.hang_up(true);
        let (status, _) = s.exit_within(5);
        assert_eq!(std::os::unix::process::ExitStatusExt::signal(&status), None, "ended by a signal (abort dumps core): {status:?}");
        assert_eq!(status.code(), Some(128 + libc::SIGHUP));
    }
}

#[test]
fn a_second_ctrl_c_stops_a_command_that_started_its_own_process_at_once() {
    let mut s = Screens::start("ctrlc");
    s.press("\u{3}");
    let start = Instant::now();
    s.press("\u{3}");
    s.wait_for("stopped by ^C");
    assert!(start.elapsed() < Duration::from_millis(1500), "the stop waited on the command's children: {:?}", start.elapsed());
    s.assert_jobs_gone();
    s.press("\u{1b}");
    s.press("X");
    s.press("y");
    s.press("q");
    let (status, _) = s.exit_within(5);
    assert!(status.success(), "{status:?}");
}

#[test]
fn closing_the_terminal_ends_the_screens_and_the_command() {
    let mut s = Screens::start("hangup");
    // The terminal goes away: every handle on its master is closed.
    s.hang_up(false);
    let (status, took) = s.exit_within(5);
    assert!(took < Duration::from_secs(4), "{took:?}");
    assert_eq!(status.code(), Some(128 + libc::SIGHUP), "a hangup ends as SIGHUP does: {:?}", std::os::unix::process::ExitStatusExt::signal(&status));
    s.assert_jobs_gone();
}
