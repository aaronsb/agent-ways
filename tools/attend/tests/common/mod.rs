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
        let home = attend_presence::session::fixture_temp_dir().join(format!("attend-it-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".claude").join("sessions")).unwrap();
        let config = home.join("config").join("attend");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("config.yaml"), QUIET_CONFIG.replace("{peers}", &secs.to_string())).unwrap();
        let origin = home.join("proj");
        assert_origin_kept(&origin);
        std::fs::create_dir_all(&origin).unwrap();
        let f = Fixture { origin: origin.to_string_lossy().into_owned(), sid: format!("sess-{tag}"), home };
        f.set_session_id(&f.sid.clone());
        f.stand_in_ways();
        f.last_turn(Some(Duration::ZERO));
        f
    }

    /// A `ways` first on attend's PATH that answers exactly
    /// `ways context --json --session <id>`, run from the fixture's
    /// project, with the reading [`Fixture::last_turn_of`] wrote for that
    /// id. Any other call, or an id with no reading, fails as a missing
    /// `ways` would, which the wake gate reads as a lapsed cache. The gate
    /// reads the cache clock there.
    fn stand_in_ways(&self) {
        let bin = self.home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let ways = bin.join("ways");
        let readings = self.home.join("readings");
        std::fs::create_dir_all(&readings).unwrap();
        let script = format!(
            "#!/bin/sh\n\
             [ \"$#\" = 4 ] && [ \"$1 $2 $3\" = 'context --json --session' ] && [ \"$(pwd -P)\" = '{origin}' ] \\\n  \
             && [ -f '{readings}/'\"$4\".json ] && exec cat '{readings}/'\"$4\".json\n\
             exit 1\n",
            origin = std::fs::canonicalize(&self.origin).unwrap().display(),
            readings = readings.display(),
        );
        std::fs::write(&ways, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&ways, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Set this session's cache clock: see [`Fixture::last_turn_of`]. A new
    /// fixture's last turn is now, so its cache is warm.
    pub fn last_turn(&self, ago: Option<Duration>) {
        self.last_turn_of(&self.sid, ago);
    }

    /// Set session `sid`'s cache clock: its newest assistant turn was `ago`
    /// back, on a large context at the one-hour tier; `None` leaves no
    /// transcript, as for a session that has had no turn yet.
    pub fn last_turn_of(&self, sid: &str, ago: Option<Duration>) {
        self.last_turn_at_tier(sid, ago, "1h");
    }

    /// [`Fixture::last_turn_of`] at cache tier `tier` (`1h` or `5m`).
    pub fn last_turn_at_tier(&self, sid: &str, ago: Option<Duration>, tier: &str) {
        let Some(ago) = ago else {
            std::fs::remove_file(self.reading(sid)).ok();
            std::fs::remove_file(self.transcript(sid)).ok();
            return;
        };
        let at = (SystemTime::now() - ago).duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        self.write_reading(
            sid,
            &format!(
                r#"{{"tokens_used":200520,"model":"claude-fable-5-1","usage_tail":[{{"at_epoch":{at},"model":"claude-fable-5-1","input":20,"cache_read":200000,"cache_creation":500,"output":5,"tier":"{tier}"}}]}}"#
            ),
        );
    }

    /// Session `sid` has a transcript and no assistant turn in it yet (a
    /// local command such as `/mcp` wrote it): `ways context` reads no API
    /// usage and estimates the context from the file's bytes, as the real
    /// one prints for such a transcript.
    pub fn no_turn_yet(&self, sid: &str) {
        self.write_reading(sid, r#"{"tokens_used":4392,"model":"unknown","method":"bytes","usage_tail":[]}"#);
    }

    /// Session `sid` has a transcript that `ways context` cannot read.
    pub fn unreadable_transcript(&self, sid: &str) {
        std::fs::remove_file(self.reading(sid)).ok();
        self.touch_transcript(sid);
    }

    /// The reading the stand-in `ways` serves for `sid`, with the
    /// transcript it stands for.
    fn write_reading(&self, sid: &str, json: &str) {
        std::fs::write(self.reading(sid), json).unwrap();
        self.touch_transcript(sid);
    }

    fn reading(&self, sid: &str) -> PathBuf {
        self.home.join("readings").join(format!("{sid}.json"))
    }

    /// Where Claude Code keeps session `sid`'s transcript in this fixture.
    pub fn transcript(&self, sid: &str) -> PathBuf {
        self.home.join(".claude").join("projects").join(claude_sessions::project_slug(&self.origin)).join(format!("{sid}.jsonl"))
    }

    fn touch_transcript(&self, sid: &str) {
        let path = self.transcript(sid);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    /// Rewrite the session record, as Claude Code does on `/clear`. The
    /// record says the session started a day ago, so mail addressed to the
    /// project within that day was addressed to it.
    pub fn set_session_id(&self, sid: &str) {
        let pid = std::process::id();
        std::fs::write(
            self.home.join(".claude").join("sessions").join(format!("{pid}.json")),
            self.record(&pid.to_string(), sid, Some(SystemTime::now() - DAY)),
        )
        .unwrap();
    }

    /// A session record's JSON, as Claude Code writes it; `started` is its
    /// `startedAt`, left out when `None`.
    pub fn record(&self, pid: &str, sid: &str, started: Option<SystemTime>) -> String {
        let started = started
            .map(|t| format!(r#","startedAt":{}"#, t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()))
            .unwrap_or_default();
        format!(r#"{{"pid":{pid},"sessionId":"{sid}","cwd":"{}"{started}}}"#, self.origin)
    }

    /// Run attend as session `sid` of another Claude Code process in the
    /// same project, one that started `started_ago` back (`None`: a record
    /// with no `startedAt`). A shell stands in for that process and writes
    /// its own session record, so its process key differs from the
    /// fixture's and attend sees a new session, not a `/clear` of this one.
    pub fn attend_in_process(&self, sid: &str, started_ago: Option<Duration>, args: &[&str]) -> String {
        let record = self.record("PID", sid, started_ago.map(|a| SystemTime::now() - a));
        let script = r#"printf %s "$REC" | sed "s/PID/$$/" > "$HOME/.claude/sessions/$$.json"; "$@"; s=$?; rm -f "$HOME/.claude/sessions/$$.json"; exit $s"#;
        let out = self
            .command_from(Path::new("sh"))
            .env("REC", record)
            .args(["-c", script, "sh", env!("CARGO_BIN_EXE_attend")])
            .args(args)
            .output()
            .expect("run attend in another process");
        assert!(out.status.success(), "attend {args:?} as {sid}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
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

    /// Whether session `sid`'s enrollment record holds a way. A record
    /// whose last way went keeps only its process, and enrolls nothing.
    pub fn enrolled(&self, sid: &str) -> bool {
        ["run", "join", "listen", "add"].iter().any(|line| self.record_has(sid, line))
    }

    /// Whether session `sid`'s enrollment record has `line` as a whole
    /// line. The record's `claude` key names the host, which may hold any
    /// of these words (a GitHub runner is `runnervm…`), so a substring
    /// match is no test of the record.
    pub fn record_has(&self, sid: &str, line: &str) -> bool {
        std::fs::read_to_string(self.marker(sid)).is_ok_and(|r| r.lines().any(|l| l == line))
    }

    pub fn command(&self) -> Command {
        self.command_from(Path::new(env!("CARGO_BIN_EXE_attend")))
    }

    pub fn command_from(&self, bin: &Path) -> Command {
        let mut path = std::ffi::OsString::from(self.home.join("bin"));
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap_or_default());
        let mut c = Command::new(bin);
        c.env("HOME", &self.home)
            .env("PATH", path)
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_CONFIG_HOME", self.home.join("config"))
            .env("XDG_DATA_HOME", self.home.join("data"))
            .env("XDG_STATE_HOME", self.home.join("state"))
            .env("USER", "fixture-human")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("ATTEND_RELOADED_FROM")
            .current_dir(&self.origin)
            .stdin(Stdio::null());
        c
    }

    pub fn attend(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("run attend")
    }

    /// [`Fixture::attend`] with extra environment.
    pub fn attend_with(&self, args: &[&str], env: &[(&str, &std::ffi::OsStr)]) -> Output {
        let mut cmd = self.command();
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.args(args).output().expect("run attend")
    }

    pub fn ok(&self, args: &[&str]) -> String {
        let out = self.attend(args);
        assert!(out.status.success(), "attend {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn drain(&self, format: &str) -> String {
        self.ok(&["message", "inbox", "--drain", "--format", format])
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
            .args(["message", "inbox", "--drain", "--format", "hook"])
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
        self.run_from(Path::new(env!("CARGO_BIN_EXE_attend")), env)
    }

    /// `attend run` from the binary at `bin`. A spawn that meets the file
    /// still held open for writing (`ExecutableFileBusy`: a binary just
    /// copied, while another test's fork inherited the writer) is tried
    /// again shortly.
    pub fn run_from(&self, bin: &Path, env: &[(&str, &str)]) -> Run {
        let log = self.home.join(format!("run-{}.out", self.sid));
        for _ in 0..50 {
            let mut cmd = self.command_from(bin);
            for (k, v) in env {
                cmd.env(k, v);
            }
            let spawned = cmd
                .arg("run")
                .stdout(Stdio::from(std::fs::File::create(&log).unwrap()))
                .stderr(Stdio::from(std::fs::File::create(self.home.join("run.err")).unwrap()))
                .spawn();
            match spawned {
                Ok(child) => return Run { child, log },
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => panic!("spawn attend run: {e}"),
            }
        }
        panic!("spawn attend run: the binary stayed busy");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.home).ok();
    }
}

/// Fail here, naming the path, when session identity would rewrite a fixture
/// project's path. A project under a managed worktree resolves to the
/// checkout's root, and the assertions downstream then fail without saying
/// why (#902).
pub fn assert_origin_kept(project: &Path) {
    assert!(
        attend_presence::session::origin_kept(project),
        "fixture project {} sits under a managed worktree (/.claude/worktrees/), so session identity would resolve it \
         to the checkout's root; make fixtures under attend_presence::session::fixture_temp_dir() (#902)",
        project.display()
    );
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
    /// Whether the process has exited.
    pub fn exited(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }

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
pub const DAY: Duration = Duration::from_secs(24 * 3600);

/// A program named `claude` in the fixture's home: a link to `/bin/sh`,
/// which the process scan takes for Claude Code by its program's name, as
/// it takes a `claude` install. It sits outside the fixture's PATH, so no
/// call of `claude` finds it.
fn claude_program(f: &Fixture) -> PathBuf {
    let dir = f.home.join("claude-code");
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("claude");
    match std::os::unix::fs::symlink("/bin/sh", &program) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => panic!("link {}: {e}", program.display()),
        _ => program,
    }
}

/// A live Claude Code session `sid` at `<home>/<project>`: a process whose
/// program is named claude ([`claude_program`]), with a session record.
/// Killed on drop. Only the peers sensor's process scan finds it.
pub struct Peer {
    child: Child,
    pub cwd: String,
}

impl Peer {
    /// Start it, and return once the process scan sees it: a freshly
    /// spawned child's command line reads empty for a moment, and a scan in
    /// that moment takes the session for exited.
    pub fn start(f: &Fixture, sid: &str, project: &str) -> Peer {
        let cwd = f.home.join(project);
        assert_origin_kept(&cwd);
        std::fs::create_dir_all(&cwd).unwrap();
        let cwd = cwd.to_string_lossy().into_owned();
        let child = Command::new(claude_program(f)).args(["-c", "sleep 60; :"]).stdout(Stdio::null()).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !attend_presence::process::is_claude(child.id()) {
            assert!(Instant::now() < deadline, "the peer process {} never showed its command line", child.id());
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::write(
            f.home.join(".claude").join("sessions").join(format!("{}.json", child.id())),
            format!(r#"{{"pid":{},"sessionId":"{sid}","cwd":"{cwd}"}}"#, child.id()),
        )
        .unwrap();
        Peer { child, cwd }
    }

    pub fn tray(&self, f: &Fixture) -> PathBuf {
        f.signals().join(claude_sessions::attend_key(&self.cwd))
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

/// A Claude Code process of its own in the fixture's project, which runs
/// attend as its child: a shell whose program is named claude
/// ([`claude_program`]), with a session record. An add records its process
/// key as the session's own, and `/clear` ([`Claude::clear`]) gives it a new
/// id under the same process. Killed on drop.
pub struct Claude {
    child: Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    record: PathBuf,
    out: PathBuf,
    started: SystemTime,
}

impl Claude {
    /// Start it as session `sid`, and return once the process scan sees it.
    pub fn start(f: &Fixture, sid: &str) -> Claude {
        let mut child = f
            .command_from(&claude_program(f))
            .args(["-c", r#"while IFS= read -r line; do eval "$line"; echo "$?"; done"#])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !attend_presence::process::is_claude(child.id()) {
            assert!(Instant::now() < deadline, "the claude process {} never showed its command line", child.id());
            std::thread::sleep(Duration::from_millis(5));
        }
        let record = f.home.join(".claude").join("sessions").join(format!("{}.json", child.id()));
        let (stdin, stdout) = (child.stdin.take().unwrap(), std::io::BufReader::new(child.stdout.take().unwrap()));
        let out = f.home.join(format!("claude-{}.out", child.id()));
        let c = Claude { child, stdin, stdout, record, out, started: SystemTime::now() };
        c.clear(f, sid);
        c
    }

    /// Rewrite its session record under `sid`, as Claude Code does on
    /// `/clear`.
    pub fn clear(&self, f: &Fixture, sid: &str) {
        std::fs::write(&self.record, f.record(&self.child.id().to_string(), sid, Some(self.started))).unwrap();
    }

    /// Run attend with `args` as this session, and return its stdout.
    pub fn attend(&mut self, args: &[&str]) -> String {
        use std::io::{BufRead, Write};
        let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let mut line = quote(env!("CARGO_BIN_EXE_attend"));
        for a in args {
            line += &format!(" {}", quote(a));
        }
        line += &format!(" >{} 2>&1 </dev/null\n", quote(&self.out.to_string_lossy()));
        self.stdin.write_all(line.as_bytes()).unwrap();
        self.stdin.flush().unwrap();
        let mut status = String::new();
        self.stdout.read_line(&mut status).unwrap();
        let out = std::fs::read_to_string(&self.out).unwrap_or_default();
        assert_eq!(status.trim(), "0", "attend {args:?}: {out}");
        out
    }
}

impl Drop for Claude {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}
