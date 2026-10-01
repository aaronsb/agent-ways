//! The resident agent (ADR-502 §1-4, §8).
//!
//! One per user, listening on a Unix socket of mode 0600 and answering only
//! connections from the same uid. It starts under an exclusive lock, so two
//! hooks racing to start it leave one agent; it exits when idle, and when its
//! own binary is replaced on disk, so an update takes effect at the next
//! prompt. Each connection carries one request and gets its own thread;
//! provider calls share one connection pool and a machine-wide cap.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::judge::{self, Candidate};
use crate::profile::{self, Mode, Provider, Settings, UserLayer};
use crate::protocol::{self, Envelope, JudgeRequest, Judged, Reply, Request, Status, Verdict};
use crate::{keys, net};

/// The longest request line the agent reads.
const MAX_REQUEST: u64 = 4 * 1024 * 1024;
/// Latency samples kept for the status percentiles.
const SAMPLES: usize = 200;

pub struct Options {
    pub idle: Duration,
}

/// Runs the agent until it is idle, replaced, or told to shut down. Returns
/// `Ok` without serving when another agent holds the lock.
pub fn serve(options: Options) -> Result<()> {
    let sock = protocol::socket_path();
    let dir = sock.parent().context("socket path has no parent")?.to_path_buf();
    protocol::secure_dir(&dir).map_err(anyhow::Error::msg)?;
    let lock_path = sock.with_extension("lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    // SAFETY: flock on a file descriptor we own; LOCK_NB returns at once.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Ok(());
    }
    // Holding the lock, a socket left behind is stale. Anything else at the
    // path is not ours to delete.
    match std::fs::symlink_metadata(&sock) {
        Ok(m) if m.file_type().is_socket() => std::fs::remove_file(&sock)?,
        Ok(_) => bail!("{} exists and is not a socket; not replacing it", sock.display()),
        Err(_) => {}
    }
    let listener = UnixListener::bind(&sock).with_context(|| format!("binding {}", sock.display()))?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))?;

    let state = Arc::new(State::new());
    let exe = Exe::current();
    watchdog(Arc::clone(&state), sock.clone(), options.idle, exe.clone());

    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            // Out of descriptors, say: wait rather than spin.
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        let state = Arc::clone(&state);
        let sock = sock.clone();
        let exe = exe.clone();
        state.conns.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            state.touch();
            let shutdown = handle(stream, &state);
            state.touch();
            let others = state.conns.fetch_sub(1, Ordering::SeqCst) - 1;
            // A replaced binary exits once no connection is open; the
            // watchdog catches the case where another still was.
            if shutdown || (others == 0 && exe.as_ref().is_some_and(Exe::replaced)) {
                leave(&sock);
            }
        });
    }
    drop(lock);
    Ok(())
}

/// Removes the socket and exits. The lock goes with the process.
fn leave(sock: &Path) -> ! {
    let _ = std::fs::remove_file(sock);
    std::process::exit(0);
}

fn watchdog(state: Arc<State>, sock: PathBuf, idle: Duration, exe: Option<Exe>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let busy = state.conns.load(Ordering::SeqCst) > 0;
        if !busy && (state.idle_for() >= idle || exe.as_ref().is_some_and(Exe::replaced)) {
            leave(&sock);
        }
    });
}

/// Handles one connection. True when the request was a shutdown.
fn handle(stream: UnixStream, state: &State) -> bool {
    if !same_user(&stream) {
        return false;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new((&stream).take(MAX_REQUEST)).read_line(&mut line).is_err() {
        return false;
    }
    let (reply, shutdown) = match serde_json::from_str::<Envelope>(&line) {
        Err(e) => (Reply::Error { message: format!("bad request: {e}") }, false),
        Ok(env) if env.protocol != protocol::PROTOCOL => (
            Reply::Error { message: format!("protocol {} not served; this agent speaks {}", env.protocol, protocol::PROTOCOL) },
            false,
        ),
        Ok(env) => match env.request {
            Request::Judge(req) => (state.judge(req), false),
            Request::Status => (Reply::Status(state.status()), false),
            Request::Shutdown => (Reply::Ok, true),
        },
    };
    let envelope = protocol::ReplyEnvelope { agent: agent_id(), reply };
    if let Ok(mut text) = serde_json::to_string(&envelope) {
        text.push('\n');
        let _ = (&stream).write_all(text.as_bytes());
    }
    shutdown
}

fn agent_id() -> protocol::AgentId {
    protocol::AgentId {
        version: env!("CARGO_PKG_VERSION").to_string(),
        exe: std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
    }
}

/// True when the peer runs as this user.
fn same_user(stream: &UnixStream) -> bool {
    // SAFETY: getuid has no preconditions and cannot fail.
    let me = unsafe { libc::getuid() };
    peer_uid(stream) == Some(me)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: SO_PEERCRED fills a ucred of the length we pass.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    (rc == 0).then_some(cred.uid)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: getpeereid writes the two ids of a connected Unix socket.
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    (rc == 0).then_some(uid)
}

/// The binary this agent runs from, to notice an update replacing it.
#[derive(Clone)]
struct Exe {
    path: PathBuf,
    stamp: (SystemTime, u64),
}

impl Exe {
    fn current() -> Option<Exe> {
        let path = std::env::current_exe().ok()?;
        let stamp = Self::stamp(&path)?;
        Some(Exe { path, stamp })
    }

    fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
        let meta = std::fs::metadata(path).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }

    /// Changed or gone: an update that swaps the file changes its stamp, and
    /// one that removes it fails the stat.
    fn replaced(&self) -> bool {
        Self::stamp(&self.path) != Some(self.stamp)
    }
}

struct State {
    started: Instant,
    /// Connections accepted and not yet answered. Exits wait for zero.
    conns: AtomicUsize,
    last_activity: AtomicU64,
    http: ureq::Agent,
    slots: Mutex<usize>,
    freed: Condvar,
    /// One key check at a time, so a burst of first prompts checks once.
    check_lock: Mutex<()>,
    stats: Mutex<Stats>,
}

#[derive(Default)]
struct Stats {
    requests: u64,
    judged: u64,
    fallbacks: BTreeMap<String, u64>,
    latencies: VecDeque<u64>,
}

impl State {
    fn new() -> State {
        State {
            started: Instant::now(),
            conns: AtomicUsize::new(0),
            last_activity: AtomicU64::new(now_s()),
            http: net::agent(Duration::from_secs(60)),
            slots: Mutex::new(0),
            freed: Condvar::new(),
            check_lock: Mutex::new(()),
            stats: Mutex::new(Stats::default()),
        }
    }

    fn touch(&self) {
        self.last_activity.store(now_s(), Ordering::Relaxed);
    }

    fn idle_for(&self) -> Duration {
        Duration::from_secs(now_s().saturating_sub(self.last_activity.load(Ordering::Relaxed)))
    }

    fn in_flight(&self) -> usize {
        *self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Settings re-read per request, so `ways agent use`, `mode` and key
    /// changes apply without a restart.
    fn settings() -> Result<Option<Settings>> {
        let user = UserLayer::load(&profile::user_layer_path())?;
        profile::resolve(&user, |p| keys::locate(p).is_some())
    }

    fn judge(&self, req: JudgeRequest) -> Reply {
        let begun = Instant::now();
        let outcome = self.judge_inner(&req, begun);
        let latency_ms = begun.elapsed().as_millis() as u64;
        let mut stats = self.stats.lock().unwrap_or_else(|e| e.into_inner());
        stats.requests += 1;
        match outcome {
            Ok(mut judged) => {
                stats.judged += 1;
                stats.latencies.push_back(latency_ms);
                if stats.latencies.len() > SAMPLES {
                    stats.latencies.pop_front();
                }
                judged.latency_ms = latency_ms;
                Reply::Judged(judged)
            }
            Err(reason) => {
                let key = reason.split(':').next().unwrap_or("other").to_string();
                *stats.fallbacks.entry(key).or_default() += 1;
                Reply::Fallback { reason, latency_ms }
            }
        }
    }

    fn judge_inner(&self, req: &JudgeRequest, begun: Instant) -> Result<Judged, String> {
        if req.candidates.is_empty() {
            return Err("no_candidates".to_string());
        }
        let settings = Self::settings().map_err(|e| format!("config: {e:#}"))?;
        let Some(settings) = settings else { return Err("off".to_string()) };
        if settings.mode == Mode::Off {
            return Err("off".to_string());
        }
        let p = &settings.profile;
        let Some((key, source)) = keys::read(p.provider).map_err(|e| format!("key: {e:#}"))? else {
            return Err("no_key".to_string());
        };
        self.verified(p.provider, &key, &source, &p.model)?;
        let deadline = Duration::from_millis(p.timeout_ms);
        let _slot = self.acquire(p.concurrency, deadline.saturating_sub(begun.elapsed()))?;
        let remaining = deadline.saturating_sub(begun.elapsed());
        if remaining.is_zero() {
            return Err("deadline".to_string());
        }
        let turns = judge::render_turns(&req.turns, p.turns, p.max_turn_chars);
        let prompt = judge::render_prompt(&turns, &req.candidates);
        let p_yes = net::judge(&self.http, p.provider, &key, &p.model, &prompt, req.candidates.len(), remaining)?;
        Ok(Judged {
            engine: settings.engine.clone(),
            provider: p.provider,
            model: p.model.clone(),
            mode: settings.mode,
            threshold: p.threshold,
            verdicts: req
                .candidates
                .iter()
                .zip(p_yes)
                .map(|(Candidate { id, .. }, p_yes)| Verdict { id: id.clone(), p_yes })
                .collect(),
            latency_ms: 0,
        })
    }

    /// A working key is the operator's approval to gate (ADR-196 §6): judge
    /// only with a key whose last recorded check passed. A key never checked,
    /// or replaced since, is checked once here and the result recorded; a
    /// check that could not reach a verdict is retried after five minutes.
    fn verified(&self, provider: Provider, key: &str, source: &keys::Source, model: &str) -> Result<(), String> {
        let transient = |r: &str| matches!(r, "unreachable" | "rate_limited" | "failed");
        let record = match keys::last_check(provider) {
            Some(r) if r.describes(source) && !(transient(&r.result) && r.age_s() >= 300) => r,
            _ => {
                let _guard = self.check_lock.lock().unwrap_or_else(|e| e.into_inner());
                let result = net::check(provider, key, model);
                let record = keys::CheckRecord::now(result.record_word(), model, source);
                keys::record_check(provider, &record);
                record
            }
        };
        match record.result.as_str() {
            "valid" => Ok(()),
            other => Err(format!("key_unverified: last check {other}")),
        }
    }

    /// Waits up to `wait` for one of `cap` provider slots.
    fn acquire(&self, cap: usize, wait: Duration) -> Result<Slot<'_>, String> {
        let mut used = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let until = Instant::now() + wait;
        while *used >= cap {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("busy".to_string());
            }
            used = self.freed.wait_timeout(used, left).unwrap_or_else(|e| e.into_inner()).0;
        }
        *used += 1;
        Ok(Slot(self))
    }

    fn status(&self) -> Status {
        let settings = Self::settings().ok().flatten();
        // Read before taking `stats`: `acquire` holds `slots`, and no thread
        // may hold `stats` while it waits for `slots`.
        let in_flight = self.in_flight();
        let stats = self.stats.lock().unwrap_or_else(|e| e.into_inner());
        let mut sorted: Vec<u64> = stats.latencies.iter().copied().collect();
        sorted.sort_unstable();
        let pct = |q: f64| (!sorted.is_empty()).then(|| sorted[((sorted.len() - 1) as f64 * q).round() as usize]);
        Status {
            version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            uptime_s: self.started.elapsed().as_secs(),
            engine: settings.as_ref().map(|s| s.engine.clone()),
            model: settings.as_ref().map(|s| s.profile.model.clone()),
            mode: settings.as_ref().map(|s| s.mode),
            requests: stats.requests,
            judged: stats.judged,
            fallbacks: stats.fallbacks.clone(),
            in_flight,
            concurrency: settings.as_ref().map(|s| s.profile.concurrency).unwrap_or(0),
            latency_p50_ms: pct(0.5),
            latency_p95_ms: pct(0.95),
        }
    }
}

/// A held provider slot, released on drop.
struct Slot<'a>(&'a State);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let mut used = self.0.slots.lock().unwrap_or_else(|e| e.into_inner());
        *used = used.saturating_sub(1);
        self.0.freed.notify_one();
    }
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Asks a running agent to stop. Ok(false) when none was running.
pub fn stop() -> Result<bool> {
    match crate::client::call(Request::Shutdown, Duration::from_secs(5), false) {
        Ok(Reply::Ok) => Ok(true),
        Ok(other) => bail!("unexpected reply: {other:?}"),
        Err(reason) if reason == "agent_absent" => Ok(false),
        Err(reason) => bail!("{reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_cap_concurrency_and_time_out_as_busy() {
        let state = State::new();
        let a = state.acquire(1, Duration::from_millis(10)).unwrap();
        assert_eq!(state.in_flight(), 1);
        assert_eq!(state.acquire(1, Duration::from_millis(20)).err(), Some("busy".to_string()));
        drop(a);
        assert_eq!(state.in_flight(), 0);
        assert!(state.acquire(1, Duration::from_millis(10)).is_ok());
    }

    #[test]
    fn status_and_acquire_run_concurrently_without_deadlock() {
        let state = Arc::new(State::new());
        let workers: Vec<_> = (0..4)
            .map(|i| {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    for _ in 0..2000 {
                        if i % 2 == 0 {
                            drop(state.acquire(2, Duration::from_millis(5)));
                        } else {
                            let _ = state.status();
                        }
                    }
                })
            })
            .collect();
        let deadline = Instant::now() + Duration::from_secs(20);
        for w in workers {
            while !w.is_finished() {
                assert!(Instant::now() < deadline, "status and acquire deadlocked");
                std::thread::sleep(Duration::from_millis(10));
            }
            w.join().unwrap();
        }
        assert_eq!(state.in_flight(), 0);
    }

    #[test]
    fn an_empty_candidate_list_falls_back_without_a_call() {
        let state = State::new();
        let req = JudgeRequest { session: "s".into(), tool: "t".into(), turns: vec![], candidates: vec![] };
        assert!(matches!(state.judge(req), Reply::Fallback { reason, .. } if reason == "no_candidates"));
    }
}
