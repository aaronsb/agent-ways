//! A sandboxed attend world for integration tests: its own HOME, cache and
//! config, and a session record naming this test process, so every attend
//! run from here resolves to the fixture's session.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime};

/// Attend config for tests: only the peers sensor, polling every
/// `{peers}` seconds.
const QUIET_CONFIG: &str = "\
sensors:
  context:
    enabled: false
  git:
    enabled: false
  processes:
    enabled: false
  disclosure:
    enabled: false
  keepwarm:
    enabled: false
  peers:
    interval: {peers}
    min_interval: {peers}
cleanup:
  enabled: false
";

pub struct Fixture {
    pub home: PathBuf,
    pub origin: String,
    pub sid: String,
}

impl Fixture {
    pub fn new(tag: &str) -> Self {
        Self::with_peers_interval(tag, 1)
    }

    /// A fixture whose peers sensor polls every `secs` seconds.
    pub fn with_peers_interval(tag: &str, secs: u64) -> Self {
        let home = std::env::temp_dir().join(format!("attend-it-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".claude").join("sessions")).unwrap();
        let config = home.join("config").join("attend");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("config.yaml"), QUIET_CONFIG.replace("{peers}", &secs.to_string())).unwrap();
        let origin = home.join("proj");
        std::fs::create_dir_all(&origin).unwrap();
        let f = Fixture { origin: origin.to_string_lossy().into_owned(), sid: format!("sess-{tag}"), home };
        f.set_session_id(&f.sid.clone());
        f
    }

    /// Rewrite the session record, as Claude Code does on `/clear`.
    pub fn set_session_id(&self, sid: &str) {
        let pid = std::process::id();
        std::fs::write(
            self.home.join(".claude").join("sessions").join(format!("{pid}.json")),
            format!(r#"{{"pid":{pid},"sessionId":"{sid}","cwd":"{}"}}"#, self.origin),
        )
        .unwrap();
    }

    /// Remove the session record: attend then runs as an outside party (a
    /// human or a peer), not as this session.
    pub fn without_session<T>(&self, f: impl FnOnce() -> T) -> T {
        let rec = self.home.join(".claude").join("sessions").join(format!("{}.json", std::process::id()));
        let saved = std::fs::read(&rec).unwrap();
        std::fs::remove_file(&rec).unwrap();
        let out = f();
        std::fs::write(&rec, saved).unwrap();
        out
    }

    pub fn cache(&self) -> PathBuf {
        self.home.join(".cache").join("attend")
    }

    pub fn signals(&self) -> PathBuf {
        self.cache().join("signals")
    }

    pub fn project_tray(&self) -> String {
        claude_sessions::attend_key(&self.origin)
    }

    pub fn heartbeat(&self, sid: &str) -> PathBuf {
        self.cache().join("heartbeat").join(sid)
    }

    pub fn state_file(&self, sid: &str) -> PathBuf {
        self.cache().join("state").join(format!("{sid}.state"))
    }

    pub fn marker(&self, sid: &str) -> PathBuf {
        self.cache().join("enrolled").join(sid)
    }

    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_attend"));
        c.env("HOME", &self.home)
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("XDG_DATA_HOME", self.home.join("data"))
            .env("USER", "fixture-human")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("ATTEND_RELOADED_FROM")
            .current_dir(&self.origin)
            .stdin(Stdio::null());
        c
    }

    pub fn attend(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("run attend")
    }

    pub fn ok(&self, args: &[&str]) -> String {
        let out = self.attend(args);
        assert!(out.status.success(), "attend {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn drain(&self, format: &str) -> String {
        self.ok(&["inbox", "--drain", "--format", format])
    }

    /// Write a signal from another session into `tray`, `age` old.
    pub fn put(&self, tray: &str, name: &str, body: &str, age: Duration) {
        let dir = self.signals().join(tray);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.signal"));
        std::fs::write(&path, format!("claude:other-session|elsewhere|/tmp/elsewhere|{body}\n")).unwrap();
        age_file(&path, age);
    }

    /// Drain in hook form with `payload` as the Stop hook's stdin.
    pub fn drain_hook_with(&self, payload: &str) -> String {
        use std::io::Write;
        let mut child = self
            .command()
            .args(["inbox", "--drain", "--format", "hook"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("run attend");
        child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Start `attend run`; its stdout (the Monitor lines) goes to a file.
    pub fn run(&self) -> Run {
        self.run_with(&[])
    }

    /// [`Fixture::run`] with extra environment.
    pub fn run_with(&self, env: &[(&str, &str)]) -> Run {
        let log = self.home.join(format!("run-{}.out", self.sid));
        let mut cmd = self.command();
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = cmd
            .arg("run")
            .stdout(Stdio::from(std::fs::File::create(&log).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(self.home.join("run.err")).unwrap()))
            .spawn()
            .expect("spawn attend run");
        Run { child, log }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.home).ok();
    }
}

pub fn age_file(path: &Path, age: Duration) {
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() - age).unwrap();
}

pub struct Run {
    child: Child,
    log: PathBuf,
}

impl Run {
    pub fn output(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Wait until `done` holds, or panic after `limit` naming what was seen.
    pub fn wait_for(&self, what: &str, limit: Duration, done: impl Fn(&Run) -> bool) {
        let start = Instant::now();
        while start.elapsed() < limit {
            if done(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("timed out waiting for {what}; attend run printed:\n{}", self.output());
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub const MINUTE: Duration = Duration::from_secs(60);
