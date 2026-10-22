//! The prompt lane's handoff to a Claude Code judge host (ADR-708 §2, §5).
//!
//! A session's judge state is in two places. Its engine resolution and its
//! host registration live under the durable state root, beside the subagent
//! switch: the SessionStart clear runs after a compaction too, and the
//! session holds its engine for its whole life. Its jobs and answers carry
//! conversation text, so they live in the session's runtime directory, which
//! that clear removes.
//!
//! - `resolution.json`: the session's engine and why, written once. The
//!   first request on either lane publishes it with an exclusive create, and
//!   every later request reads it.
//! - `host`: present once a Claude Code host registered for the session.
//! - `jobs/<id>.json`: a job written, `jobs/<id>.taken` once the host claims
//!   it, `jobs/<id>.answer` once it answers. The gate removes all three when
//!   it is done waiting, so a late answer finds no job and is dropped.
//!
//! Every file appears whole: written beside its name, then renamed or linked
//! into place.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
pub use ways_agent_core::engine::SessionEngine;
use ways_agent_core::protocol::{JudgeAnswer, JudgeJob};

/// How often a waiter looks for a file. The model call takes about a second,
/// so 10 ms is noise on the round trip.
pub const POLL: Duration = Duration::from_millis(10);

/// How long a job written by a hook that was killed mid-wait stays
/// claimable past its own deadline. A host never claims an older one.
const STALE_GRACE: Duration = Duration::from_millis(1500);

/// A job a host claimed: its id, for the answer, and the request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Taken {
    pub id: String,
    #[serde(flatten)]
    pub job: JudgeJob,
}

/// What a gate's wait for its answer came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Waited {
    Answered(JudgeAnswer),
    /// Nothing arrived by the deadline. `taken` says whether the host
    /// claimed the job, and so may have spent a model call on it.
    Silent { taken: bool },
}

/// Why an answer was not stored.
#[derive(Debug)]
pub enum AnswerError {
    /// No claimed job to answer: the gate stopped waiting, or no job was
    /// taken.
    NoJob(String),
    Io(io::Error),
}

impl std::fmt::Display for AnswerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnswerError::NoJob(why) => f.write_str(why),
            AnswerError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// One session's judge state.
#[derive(Debug, Clone)]
pub struct Store {
    state: PathBuf,
    run: PathBuf,
}

impl Store {
    /// The store of `session_id`, or `None` when it is not a plain session id
    /// ([`super::is_plain_session_id`]).
    pub fn for_session(session_id: &str) -> Option<Store> {
        super::is_plain_session_id(session_id).then(|| Store {
            state: ways_agent_core::engine::judge_state_root().join(session_id),
            run: super::session_dir(session_id).join("judge"),
        })
    }

    /// A store at explicit directories, for tests.
    #[cfg(test)]
    pub fn at(state: PathBuf, run: PathBuf) -> Store {
        Store { state, run }
    }

    fn resolution_path(&self) -> PathBuf {
        self.state.join(ways_agent_core::engine::RESOLUTION_FILE)
    }

    fn host_path(&self) -> PathBuf {
        self.state.join("host")
    }

    fn jobs(&self) -> PathBuf {
        self.run.join("jobs")
    }

    /// Registers a Claude Code judge host for the session. Idempotent.
    pub fn register_host(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.state)?;
        std::fs::write(self.host_path(), agent_fmt::when::now_secs().to_string())
    }

    pub fn host_registered(&self) -> bool {
        self.host_path().is_file()
    }

    /// The session's resolution, once one was written.
    pub fn resolution(&self) -> Option<SessionEngine> {
        SessionEngine::read(&self.state)
    }

    /// The session's resolution, resolving it with `resolve` when it has
    /// none. `resolve` gets whether a host registered. The file is published
    /// by an exclusive create, so of two first requests racing, one
    /// resolution stands and both return it.
    pub fn resolve_once(&self, resolve: impl FnOnce(bool) -> SessionEngine) -> io::Result<SessionEngine> {
        if let Some(found) = self.resolution() {
            return Ok(found);
        }
        std::fs::create_dir_all(&self.state)?;
        let mine = resolve(self.host_registered());
        let tmp = self.state.join(format!(".resolution.{}", unique()));
        std::fs::write(&tmp, serde_json::to_vec(&mine)?)?;
        // link(2) fails when the name exists: the exclusive create, with the
        // content already in place for any reader.
        let linked = std::fs::hard_link(&tmp, self.resolution_path());
        let _ = std::fs::remove_file(&tmp);
        match linked {
            Ok(()) => Ok(mine),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => self
                .resolution()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "the session's resolution file does not parse")),
            Err(e) => Err(e),
        }
    }

    /// Writes `job` for the host and returns its id.
    pub fn write_job(&self, job: &JudgeJob) -> io::Result<String> {
        let dir = self.jobs();
        std::fs::create_dir_all(&dir)?;
        let id = unique();
        place(&dir, &format!("{id}.json"), &serde_json::to_vec(job)?)?;
        Ok(id)
    }

    /// Claims the oldest job no host claimed, if any. A job past its own
    /// deadline is left out and removed: the hook that wrote it is gone.
    pub fn take(&self) -> io::Result<Option<Taken>> {
        let dir = self.jobs();
        let mut pending: Vec<String> = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(str::to_string))
                .filter(|id| !id.starts_with('.'))
                .collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        pending.sort();
        for id in pending {
            let path = dir.join(format!("{id}.json"));
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Ok(job) = serde_json::from_str::<JudgeJob>(&text) else {
                let _ = std::fs::remove_file(&path);
                continue;
            };
            let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
            if age.is_some_and(|a| a > Duration::from_millis(job.timeout_ms) + STALE_GRACE) {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            // The rename is the claim: of two takers, one moves the file.
            if std::fs::rename(&path, dir.join(format!("{id}.taken"))).is_ok() {
                return Ok(Some(Taken { id, job }));
            }
        }
        Ok(None)
    }

    /// Stores the host's answer to job `id`, or to the newest claimed job
    /// when `id` is `None`.
    pub fn answer(&self, id: Option<&str>, answer: &JudgeAnswer) -> Result<String, AnswerError> {
        let dir = self.jobs();
        let id = match id {
            Some(id) if valid_id(id) => id.to_string(),
            Some(id) => return Err(AnswerError::NoJob(format!("not a job id: {id:?}"))),
            None => {
                let taken: Vec<String> = std::fs::read_dir(&dir)
                    .map(|entries| {
                        entries
                            .flatten()
                            .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".taken")).map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                // The newest: the prompt lane's gates wait one at a time, and
                // an older claimed job's gate is about to clear it.
                match taken.into_iter().max() {
                    Some(newest) => newest,
                    None => return Err(AnswerError::NoJob("no claimed job to answer".into())),
                }
            }
        };
        if !dir.join(format!("{id}.taken")).is_file() {
            return Err(AnswerError::NoJob(format!("job {id} is not claimed or its gate stopped waiting")));
        }
        let body = serde_json::to_vec(answer).map_err(|e| AnswerError::Io(e.into()))?;
        place(&dir, &format!("{id}.answer"), &body).map_err(AnswerError::Io)?;
        Ok(id)
    }

    /// Waits for job `id`'s answer until `deadline`. An answer that does not
    /// parse counts as none.
    pub fn wait_answer(&self, id: &str, deadline: Instant) -> Waited {
        let dir = self.jobs();
        let path = dir.join(format!("{id}.answer"));
        loop {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(a) = serde_json::from_str::<JudgeAnswer>(&text) {
                    return Waited::Answered(a);
                }
            }
            if Instant::now() >= deadline {
                let taken = dir.join(format!("{id}.taken")).exists() || path.exists();
                return Waited::Silent { taken };
            }
            std::thread::sleep(POLL);
        }
    }

    /// Removes job `id` in every state, so a host claims it no more and a
    /// late answer finds nothing to answer.
    pub fn finish(&self, id: &str) {
        let dir = self.jobs();
        for ext in ["json", "taken", "answer"] {
            let _ = std::fs::remove_file(dir.join(format!("{id}.{ext}")));
        }
    }
}

/// Removes the judge state of sessions untouched for longer than `max_age`.
/// A session's state outlives its runtime directory on purpose, so nothing
/// else removes it. Run at SessionStart.
pub fn prune(max_age: Duration) {
    prune_in(&ways_agent_core::engine::judge_state_root(), max_age);
}

fn prune_in(root: &Path, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = std::fs::read_dir(&path)
            .map(|files| {
                files.flatten().all(|f| {
                    f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > max_age)
                })
            })
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// A job id: sortable by when it was written, unique across processes.
fn unique() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos:020}-{}-{seq}", std::process::id())
}

/// An id as [`unique`] makes them: digits and dashes, so it names a file in
/// the job directory and nothing beside it.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}

/// Writes `body` as `dir/name`, whole: to a dot-file beside it, then renamed.
fn place(dir: &Path, name: &str, body: &[u8]) -> io::Result<()> {
    let tmp = dir.join(format!(".{name}.{}", unique()));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(body)?;
    drop(f);
    std::fs::rename(&tmp, dir.join(name)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ways_agent_core::engine::{Resolution, EXPLICIT, NO_HOST};
    use ways_agent_core::profile::Provider;

    fn store(name: &str) -> (Store, PathBuf) {
        let base = std::env::temp_dir().join(format!("ways-judge-jobs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        (Store::at(base.join("state"), base.join("run")), base)
    }

    fn engine(name: &str, provider: Provider, reason: &str) -> SessionEngine {
        SessionEngine { resolution: Resolution { engine: Some(name.into()), reason: reason.into() }, provider: Some(provider) }
    }

    fn job(timeout_ms: u64) -> JudgeJob {
        JudgeJob { model: "claude-haiku-4-5".into(), system: "s".into(), prompt: "p".into(), max_tokens: 160, timeout_ms }
    }

    #[test]
    fn the_first_resolution_stands_for_the_session() {
        let (s, base) = store("once");
        let first = s.resolve_once(|host| {
            assert!(!host);
            engine("anthropic", Provider::Anthropic, NO_HOST)
        });
        assert_eq!(first.unwrap().resolution.engine.as_deref(), Some("anthropic"));
        // A host that registers later does not move it, and the resolver is
        // not asked again.
        s.register_host().unwrap();
        let again = s.resolve_once(|_| panic!("resolved twice")).unwrap();
        assert_eq!((again.resolution.engine.as_deref(), again.is_claude_code()), (Some("anthropic"), false));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn racing_first_requests_leave_one_resolution() {
        for round in 0..20 {
            let (s, base) = store(&format!("race-{round}"));
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
            let got: Vec<SessionEngine> = (0..8)
                .map(|i| {
                    let (s, b) = (s.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        b.wait();
                        s.resolve_once(|_| engine(&format!("e{i}"), Provider::Anthropic, EXPLICIT)).unwrap()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect();
            assert!(got.iter().all(|g| g == &got[0]), "round {round}: {got:?}");
            assert_eq!(s.resolution().as_ref(), Some(&got[0]));
            let leftovers = std::fs::read_dir(base.join("state")).unwrap().count();
            assert_eq!(leftovers, 1, "round {round}: only resolution.json remains");
            std::fs::remove_dir_all(&base).ok();
        }
    }

    #[test]
    fn a_job_is_claimed_once_answered_and_read_back() {
        let (s, base) = store("round");
        assert_eq!(s.take().unwrap(), None);
        let id = s.write_job(&job(2000)).unwrap();
        let taken = s.take().unwrap().expect("a job");
        assert_eq!((taken.id.as_str(), &taken.job), (id.as_str(), &job(2000)));
        assert_eq!(s.take().unwrap(), None, "claimed once");
        let a = JudgeAnswer { text: Some("{}".into()), ..Default::default() };
        assert_eq!(s.answer(None, &a).unwrap(), id);
        assert_eq!(s.wait_answer(&id, Instant::now()), Waited::Answered(a));
        s.finish(&id);
        assert_eq!(std::fs::read_dir(base.join("run/jobs")).unwrap().count(), 0);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn the_taken_job_prints_with_its_id() {
        let t = Taken { id: "1-2-3".into(), job: job(1800) };
        let line = serde_json::to_string(&t).unwrap();
        assert!(line.starts_with(r#"{"id":"1-2-3","model":"claude-haiku-4-5""#), "{line}");
        assert!(line.contains(r#""timeout_ms":1800"#));
    }

    #[test]
    fn silence_says_whether_the_host_claimed_the_job() {
        let (s, base) = store("silent");
        let id = s.write_job(&job(2000)).unwrap();
        assert_eq!(s.wait_answer(&id, Instant::now()), Waited::Silent { taken: false });
        s.take().unwrap().unwrap();
        assert_eq!(s.wait_answer(&id, Instant::now() + POLL * 2), Waited::Silent { taken: true });
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_late_answer_finds_no_job() {
        let (s, base) = store("late");
        let id = s.write_job(&job(2000)).unwrap();
        s.take().unwrap().unwrap();
        s.finish(&id);
        let err = s.answer(Some(&id), &JudgeAnswer::default()).unwrap_err();
        assert!(matches!(err, AnswerError::NoJob(_)), "{err}");
        assert!(!base.join(format!("run/jobs/{id}.answer")).exists());
        assert!(matches!(s.answer(None, &JudgeAnswer::default()), Err(AnswerError::NoJob(_))));
        assert!(matches!(s.answer(Some("../x"), &JudgeAnswer::default()), Err(AnswerError::NoJob(_))));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_job_past_its_deadline_is_never_claimed() {
        let (s, base) = store("stale");
        let id = s.write_job(&job(0)).unwrap();
        std::thread::sleep(STALE_GRACE + Duration::from_millis(50));
        assert_eq!(s.take().unwrap(), None);
        assert!(!base.join(format!("run/jobs/{id}.json")).exists(), "removed");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn prune_removes_only_sessions_untouched_past_the_age() {
        let (s, base) = store("prune");
        s.register_host().unwrap();
        prune_in(&base, Duration::from_secs(3600));
        assert!(s.host_registered(), "fresh state stays");
        std::thread::sleep(Duration::from_millis(20));
        prune_in(&base, Duration::from_millis(1));
        assert!(!base.join("state").exists(), "stale state goes");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_session_id_that_is_not_plain_has_no_store() {
        assert!(Store::for_session("../x").is_none());
        assert!(Store::for_session("abc-123").is_some());
    }

    #[test]
    fn a_resolution_file_round_trips_with_its_provider() {
        let e = engine("claude-code", Provider::ClaudeCode, EXPLICIT);
        let line = serde_json::to_string(&e).unwrap();
        assert_eq!(line, r#"{"engine":"claude-code","reason":"explicit","provider":"claude-code"}"#);
        assert!(serde_json::from_str::<SessionEngine>(&line).unwrap().is_claude_code());
    }
}
