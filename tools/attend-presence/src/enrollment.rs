//! Whether a session enrolled in attend (#720).
//!
//! A session enrolls by running `attend run` (`/attend`) or by joining a
//! channel. Enrollment is what the Stop-hook drain delivers to, and it is a
//! durable fact about the session, kept apart from liveness: a stale
//! heartbeat, or `cleanup_stale` pruning a channel membership, does not
//! un-enroll a session. Only an explicit opt-out does.
//!
//! The record is `<cache>/enrolled/<session-id>`, one line each for:
//! - every way the session enrolled (`run`, `join`);
//! - `optout`, when `attend scene private` ran while an `attend run` held
//!   the session: the run's enrollment ends once no run holds it;
//! - `claude <key>`, the Claude Code process the session ran in (pid and
//!   start time), so a session whose id changes under the same process
//!   (`/clear`) can find its enrollment again. `by-claude/<key>` indexes it.
//!
//! The session is enrolled while the record holds a way. Every edit is a
//! read-modify-write under one lock (`agent_settings::writer::Lock`), and
//! every file is written atomically (`write_atomic`).

use std::fs;
use std::io;
use std::path::PathBuf;

/// A way a session enrolled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `attend run`.
    Run,
    /// `attend join`, or a scene that joins a channel.
    Join,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Run => "run",
            Source::Join => "join",
        }
    }
}

/// One session's enrollment record.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Record {
    sources: Vec<Source>,
    optout: bool,
    claude: Option<String>,
}

impl Record {
    fn parse(text: &str) -> Self {
        let mut r = Record::default();
        for line in text.lines().map(str::trim) {
            match line {
                "run" => r.sources.push(Source::Run),
                "join" => r.sources.push(Source::Join),
                "optout" => r.optout = true,
                _ => {
                    if let Some(key) = line.strip_prefix("claude ") {
                        r.claude = Some(key.to_string());
                    }
                }
            }
        }
        r
    }

    fn render(&self) -> String {
        let mut out: String = self.sources.iter().map(|s| format!("{}\n", s.as_str())).collect();
        if self.optout {
            out.push_str("optout\n");
        }
        if let Some(key) = &self.claude {
            out.push_str(&format!("claude {key}\n"));
        }
        out
    }
}

fn dir() -> PathBuf {
    crate::cache::dir().join("enrolled")
}

fn path(session_id: &str) -> PathBuf {
    dir().join(session_id)
}

fn index_path(claude_key: &str) -> PathBuf {
    dir().join("by-claude").join(claude_key)
}

fn read(session_id: &str) -> Option<Record> {
    fs::read_to_string(path(session_id)).ok().map(|t| Record::parse(&t))
}

/// Write `record` for `session_id`, or remove it when it holds no way.
fn write(session_id: &str, record: &Record) -> io::Result<()> {
    let p = path(session_id);
    if record.sources.is_empty() {
        return match fs::remove_file(&p) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    agent_settings::writer::write_atomic(&p, record.render())?;
    if let Some(key) = &record.claude {
        agent_settings::writer::write_atomic(&index_path(key), session_id)?;
    }
    Ok(())
}

/// Run `edit` on `session_id`'s record under the enrollment lock and write
/// the result.
fn edit(session_id: &str, edit: impl FnOnce(&mut Record)) -> io::Result<()> {
    let _lock = agent_settings::writer::Lock::acquire(&dir().join(".edit"))?;
    let mut record = read(session_id).unwrap_or_default();
    edit(&mut record);
    write(session_id, &record)
}

/// The key of the Claude Code process this process runs under.
fn own_claude_key() -> Option<String> {
    crate::session::identity_for_pid(std::process::id()).claude_key()
}

/// Whether `session_id` is enrolled. A recorded opt-out takes effect here
/// once no `attend run` holds the session, and the record is removed.
pub fn is_enrolled(session_id: &str) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let Some(record) = read(session_id) else { return false };
    if record.optout && !crate::heartbeat::run_is_live(session_id) {
        edit(session_id, |r| {
            if r.optout {
                r.sources.clear();
            }
        })
        .ok();
        return false;
    }
    !record.sources.is_empty()
}

/// The ways `session_id` enrolled, empty when it is not enrolled.
pub fn sources(session_id: &str) -> Vec<Source> {
    read(session_id).map(|r| r.sources).unwrap_or_default()
}

/// Record that `session_id` enrolled by `source`. Idempotent. Enrolling
/// again clears a pending opt-out, and records the Claude Code process.
pub fn enroll(session_id: &str, source: Source) -> io::Result<()> {
    let claude = own_claude_key();
    edit(session_id, |r| {
        if !r.sources.contains(&source) {
            r.sources.push(source);
        }
        r.optout = false;
        if claude.is_some() {
            r.claude = claude;
        }
    })
}

/// Withdraw one way `session_id` enrolled. The session stays enrolled while
/// another way remains.
pub fn withdraw(session_id: &str, source: Source) -> io::Result<()> {
    edit(session_id, |r| r.sources.retain(|s| *s != source))
}

/// `attend scene private`: the explicit opt-out. Withdraws the join, and
/// the run too unless an `attend run` holds the session; then the run's
/// enrollment ends once it no longer does ([`is_enrolled`] applies it).
pub fn opt_out(session_id: &str) -> io::Result<()> {
    let live = crate::heartbeat::run_is_live(session_id);
    edit(session_id, |r| {
        r.sources.retain(|s| *s != Source::Join);
        if live && r.sources.contains(&Source::Run) {
            r.optout = true;
        } else {
            r.sources.clear();
        }
    })
}

/// Move `old`'s enrollment to `new`, for a session whose id changed under
/// the same process (`/clear`). Keeps any way `new` already had.
pub fn carry(old: &str, new: &str) -> io::Result<()> {
    let Some(from) = read(old) else { return Ok(()) };
    edit(new, |r| {
        for s in &from.sources {
            if !r.sources.contains(s) {
                r.sources.push(*s);
            }
        }
        r.optout |= from.optout;
        if r.claude.is_none() {
            r.claude = from.claude.clone();
        }
    })?;
    edit(old, |r| *r = Record::default())
}

/// Whether any enrollment was ever recorded under a Claude Code process
/// key. A cheap test before computing this process's key.
pub fn any_indexed() -> bool {
    dir().join("by-claude").is_dir()
}

/// The enrolled session recorded for the Claude Code process `claude_key`,
/// when it is not `current`: the id this process had before `/clear`.
pub fn previous_id(claude_key: &str, current: &str) -> Option<String> {
    let sid = fs::read_to_string(index_path(claude_key)).ok()?;
    let sid = sid.trim();
    (sid != current && read(sid).is_some_and(|r| r.claude.as_deref() == Some(claude_key))).then(|| sid.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_cache(f: impl FnOnce()) {
        let _g = crate::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!(
            "attend-enrol-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let prev = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("XDG_CACHE_HOME", &root);
        f();
        match prev {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn enrollment_lasts_until_its_last_way_is_withdrawn() {
        with_cache(|| {
            assert!(!is_enrolled("s"));
            enroll("s", Source::Join).unwrap();
            enroll("s", Source::Run).unwrap();
            enroll("s", Source::Join).unwrap();
            assert_eq!(sources("s"), vec![Source::Join, Source::Run]);
            withdraw("s", Source::Join).unwrap();
            assert!(is_enrolled("s"), "run still holds it");
            withdraw("s", Source::Run).unwrap();
            assert!(!is_enrolled("s"));
            withdraw("s", Source::Run).unwrap();
        });
    }

    /// #725 re-review, finding 8: `attend run`'s enroll and `attend
    /// leave`'s withdraw race on one record; neither may lose the other's
    /// write.
    #[test]
    fn concurrent_edits_lose_no_way() {
        with_cache(|| {
            for round in 0..1000 {
                let sid = format!("race-{round}");
                enroll(&sid, Source::Join).unwrap();
                let go = std::sync::Arc::new(std::sync::Barrier::new(2));
                let a = {
                    let (sid, go) = (sid.clone(), go.clone());
                    std::thread::spawn(move || {
                        go.wait();
                        enroll(&sid, Source::Run).unwrap()
                    })
                };
                let b = {
                    let (sid, go) = (sid.clone(), go.clone());
                    std::thread::spawn(move || {
                        go.wait();
                        withdraw(&sid, Source::Join).unwrap()
                    })
                };
                a.join().unwrap();
                b.join().unwrap();
                assert_eq!(sources(&sid), vec![Source::Run], "round {round}");
            }
        });
    }

    #[test]
    fn carry_moves_every_way_to_the_new_id() {
        with_cache(|| {
            enroll("old", Source::Run).unwrap();
            enroll("old", Source::Join).unwrap();
            carry("old", "new").unwrap();
            assert!(!is_enrolled("old"));
            assert_eq!(sources("new"), vec![Source::Run, Source::Join]);
        });
    }
}
