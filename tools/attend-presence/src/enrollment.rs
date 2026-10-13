//! Whether a session enrolled in attend (#720).
//!
//! A session enrolls by running `attend run` (`/attend`), by joining a
//! channel, or by turning listening on (`attend listen on`, ADR-406). Enrollment is what the Stop-hook drain delivers to, and it is a
//! durable fact about the session, kept apart from liveness: a stale
//! heartbeat, or `cleanup_stale` pruning a channel membership, does not
//! un-enroll a session. Only an explicit opt-out does.
//!
//! The record is `<cache>/enrolled/<session-id>`, one line each for:
//! - every way the session enrolled (`run`, `join`, `listen`);
//! - `optout`, when `attend channel scene private` or `attend listen off` ran while
//!   an `attend run` held the session: the run's enrollment ends once no
//!   run holds it;
//! - `host <name> [<key>]`, what hosts the session's delivery when it is
//!   not the default (Monitor or a shell): `host mod <key>` when the attend
//!   mod turned listening on (ADR-406), so the drain speaks that host's
//!   tools. `<key>` is the Claude Code process the mod ran in: the host
//!   holds only for that process, since a resumed session may have no mod;
//! - `claude <key>`, the Claude Code process the session ran in (pid and
//!   start time), so a session whose id changes under the same process
//!   (`/clear`) can find its enrollment again. `by-claude/<key>` indexes it;
//! - `since <secs>`, when the session last went from not listening to
//!   listening, in seconds since the epoch. From then it is a member of
//!   `#open` (ADR-404 D3a), so its cold start replays what was posted there
//!   since (D3b).
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
    /// `attend channel join`, or a scene that joins a channel.
    Join,
    /// `attend listen on`: listening without joining a channel.
    Listen,
}

impl Source {
    /// The word the record keeps it under, also what `attend listen
    /// status --json` reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Run => "run",
            Source::Join => "join",
            Source::Listen => "listen",
        }
    }
}

/// What hosts a session's delivery, when it is not the default. The default
/// host is Monitor (`attend run`) or a shell, whose agent answers with the
/// `attend` CLI; it is recorded as no host at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// The attend mod (ADR-406): the agent answers with the `attend_reply`
    /// and `attend_send` tools, and reads deliveries in the mod's format.
    Mod,
}

impl Host {
    /// The word the record keeps it under, also what `attend listen
    /// status --json` reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Host::Mod => "mod",
        }
    }

    fn parse(word: &str) -> Option<Self> {
        match word {
            "mod" => Some(Host::Mod),
            _ => None,
        }
    }
}

/// One session's enrollment record.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Record {
    sources: Vec<Source>,
    optout: bool,
    host: Option<HostRecord>,
    claude: Option<String>,
    since: Option<u64>,
    /// The ids this session had before each `/clear`, oldest first.
    previous: Vec<String>,
}

/// How many earlier ids a record keeps.
const PREVIOUS_KEPT: usize = 8;

/// A recorded host and the Claude Code process it was recorded in.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HostRecord {
    host: Host,
    claude: Option<String>,
}

impl Record {
    fn parse(text: &str) -> Self {
        let mut r = Record::default();
        for line in text.lines().map(str::trim) {
            match line {
                "run" => r.sources.push(Source::Run),
                "join" => r.sources.push(Source::Join),
                "listen" => r.sources.push(Source::Listen),
                "optout" => r.optout = true,
                _ => {
                    if let Some(key) = line.strip_prefix("claude ") {
                        r.claude = Some(key.to_string());
                    } else if let Some(sid) = line.strip_prefix("previous ") {
                        r.previous.push(sid.trim().to_string());
                    } else if let Some(secs) = line.strip_prefix("since ") {
                        r.since = secs.trim().parse().ok();
                    } else if let Some(rest) = line.strip_prefix("host ") {
                        let mut words = rest.split_whitespace();
                        r.host = words
                            .next()
                            .and_then(Host::parse)
                            .map(|host| HostRecord { host, claude: words.next().map(str::to_string) });
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
        if let Some(h) = &self.host {
            match &h.claude {
                Some(key) => out.push_str(&format!("host {} {key}\n", h.host.as_str())),
                None => out.push_str(&format!("host {}\n", h.host.as_str())),
            }
        }
        if let Some(key) = &self.claude {
            out.push_str(&format!("claude {key}\n"));
        }
        if let Some(secs) = self.since {
            out.push_str(&format!("since {secs}\n"));
        }
        for sid in &self.previous {
            out.push_str(&format!("previous {sid}\n"));
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
/// `host`, when given, records what hosts the session's delivery, for this
/// Claude Code process; `None` keeps whatever host the record holds, so a
/// join or a shell's `listen on` in a mod-hosted session leaves it
/// mod-hosted. A session that was not enrolled starts listening now
/// ([`listening_since`]).
pub fn enroll(session_id: &str, source: Source, host: Option<Host>) -> io::Result<()> {
    let claude = own_claude_key();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    edit(session_id, |r| {
        if r.sources.is_empty() {
            r.since = Some(now);
        }
        if !r.sources.contains(&source) {
            r.sources.push(source);
        }
        r.optout = false;
        if let Some(host) = host {
            r.host = Some(HostRecord { host, claude: claude.clone() });
        }
        if claude.is_some() {
            r.claude = claude;
        }
    })
}

/// When `session_id` last started listening, while it is enrolled; `None`
/// for a record written before attend kept the time.
pub fn listening_since(session_id: &str) -> Option<std::time::SystemTime> {
    let r = read(session_id).filter(|r| !r.sources.is_empty())?;
    r.since.map(|s| std::time::UNIX_EPOCH + std::time::Duration::from_secs(s))
}

/// What hosts `session_id`'s delivery, when it is enrolled and not hosted
/// by the default (Monitor or a shell). A host recorded in another Claude
/// Code process does not hold: enrollment outlives the process, and a
/// session resumed in a new one may run without the mod, or with it
/// dormant. The mod records itself again when it turns listening on.
pub fn host(session_id: &str) -> Option<Host> {
    let h = read(session_id).filter(|r| !r.sources.is_empty()).and_then(|r| r.host)?;
    (h.claude == own_claude_key()).then_some(h.host)
}

/// Withdraw one way `session_id` enrolled. The session stays enrolled while
/// another way remains.
pub fn withdraw(session_id: &str, source: Source) -> io::Result<()> {
    edit(session_id, |r| r.sources.retain(|s| *s != source))
}

/// `attend channel scene private` and `attend listen off`: the explicit opt-out.
/// Withdraws the join and the listen, and the run too unless an `attend
/// run` holds the session; then the run's enrollment ends once it no longer
/// does ([`is_enrolled`] applies it).
pub fn opt_out(session_id: &str) -> io::Result<()> {
    let live = crate::heartbeat::run_is_live(session_id);
    edit(session_id, |r| {
        r.sources.retain(|s| *s == Source::Run);
        if live && r.sources.contains(&Source::Run) {
            r.optout = true;
        } else {
            r.sources.clear();
        }
    })
}

/// Move `old`'s enrollment to `new`, for a session whose id changed under
/// the same process (`/clear`). Keeps any way `new` already had, and records
/// `old` and the ids before it as `new`'s earlier ids ([`previous_ids`]).
pub fn carry(old: &str, new: &str) -> io::Result<()> {
    let Some(from) = read(old) else { return Ok(()) };
    edit(new, |r| {
        for sid in from.previous.iter().map(String::as_str).chain([old]) {
            if sid != new && !r.previous.iter().any(|p| p == sid) {
                r.previous.push(sid.to_string());
            }
        }
        let extra = r.previous.len().saturating_sub(PREVIOUS_KEPT);
        r.previous.drain(..extra);
        for s in &from.sources {
            if !r.sources.contains(s) {
                r.sources.push(*s);
            }
        }
        r.optout |= from.optout;
        if r.host.is_none() {
            r.host = from.host;
        }
        if r.claude.is_none() {
            r.claude = from.claude.clone();
        }
        if r.since.is_none() {
            r.since = from.since;
        }
    })?;
    edit(old, |r| *r = Record::default())
}

/// The ids `session_id` had before each `/clear` that carried its
/// enrollment, oldest first. A message sent to one of them is still the
/// session's to read, and a reply to one it sent still wakes it (ADR-401,
/// `to`).
pub fn previous_ids(session_id: &str) -> Vec<String> {
    read(session_id).map(|r| r.previous).unwrap_or_default()
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
    fn a_carry_records_the_earlier_ids() {
        with_cache(|| {
            enroll("a", Source::Listen, None).unwrap();
            carry("a", "b").unwrap();
            carry("b", "c").unwrap();
            assert_eq!(previous_ids("c"), ["a", "b"]);
            assert!(previous_ids("a").is_empty() && previous_ids("nobody").is_empty());
        });
    }

    #[test]
    fn enrollment_lasts_until_its_last_way_is_withdrawn() {
        with_cache(|| {
            assert!(!is_enrolled("s"));
            enroll("s", Source::Join, None).unwrap();
            enroll("s", Source::Run, None).unwrap();
            enroll("s", Source::Join, None).unwrap();
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
                enroll(&sid, Source::Join, None).unwrap();
                let go = std::sync::Arc::new(std::sync::Barrier::new(2));
                let a = {
                    let (sid, go) = (sid.clone(), go.clone());
                    std::thread::spawn(move || {
                        go.wait();
                        enroll(&sid, Source::Run, None).unwrap()
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
    fn listening_enrolls_and_the_opt_out_ends_it() {
        with_cache(|| {
            enroll("s", Source::Listen, None).unwrap();
            enroll("s", Source::Join, None).unwrap();
            assert!(is_enrolled("s"));
            assert_eq!(Record::parse(&read("s").unwrap().render()).sources, vec![Source::Listen, Source::Join]);
            opt_out("s").unwrap();
            assert!(!is_enrolled("s"), "no run holds it, so every way ends");
        });
    }

    /// The time a session started listening holds while it listens, starts
    /// over when it listens again after an opt-out, and moves with the id.
    #[test]
    fn listening_since_is_set_when_listening_starts() {
        with_cache(|| {
            let at = |s: u64| Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(s));
            assert_eq!(listening_since("s"), None);
            enroll("s", Source::Listen, None).unwrap();
            assert!(listening_since("s").is_some(), "set on enrolling");
            write("s", &Record { since: Some(1), ..read("s").unwrap() }).unwrap();
            enroll("s", Source::Join, None).unwrap();
            assert_eq!(listening_since("s"), at(1), "another way keeps it");
            assert_eq!(Record::parse(&read("s").unwrap().render()).since, Some(1));
            opt_out("s").unwrap();
            assert_eq!(listening_since("s"), None, "not listening");
            enroll("s", Source::Listen, None).unwrap();
            assert!(listening_since("s") > at(1), "listening again starts it over");
            carry("s", "t").unwrap();
            assert!(listening_since("t") > at(1), "it moves with the id");
        });
    }

    #[test]
    fn carry_moves_every_way_to_the_new_id() {
        with_cache(|| {
            enroll("old", Source::Run, None).unwrap();
            enroll("old", Source::Join, Some(Host::Mod)).unwrap();
            carry("old", "new").unwrap();
            assert!(!is_enrolled("old"));
            assert_eq!(sources("new"), vec![Source::Run, Source::Join]);
            assert_eq!(host("new"), Some(Host::Mod), "the host moves with the ways");
        });
    }

    /// ADR-406: the mod records itself as the host when it turns listening
    /// on; a later join or a shell's `listen on` keeps it, and the opt-out
    /// that ends the enrollment ends the host with it.
    #[test]
    fn the_host_is_kept_until_the_enrollment_ends() {
        with_cache(|| {
            enroll("s", Source::Listen, None).unwrap();
            assert_eq!(host("s"), None, "a shell's listen on is the default host");
            enroll("s", Source::Listen, Some(Host::Mod)).unwrap();
            enroll("s", Source::Join, None).unwrap();
            assert_eq!(host("s"), Some(Host::Mod));
            assert_eq!(Record::parse(&read("s").unwrap().render()).host.map(|h| h.host), Some(Host::Mod));
            opt_out("s").unwrap();
            assert_eq!(host("s"), None);
        });
    }

    /// The reviewer's finding on #927: enrollment outlives the Claude Code
    /// process, so a session resumed in a new process (perhaps with no mod)
    /// must not read as mod-hosted.
    #[test]
    fn a_host_recorded_in_another_process_does_not_hold() {
        with_cache(|| {
            enroll("s", Source::Listen, Some(Host::Mod)).unwrap();
            assert_eq!(host("s"), Some(Host::Mod));
            let other = Record { host: Some(HostRecord { host: Host::Mod, claude: Some("1-999".into()) }), ..read("s").unwrap() };
            write("s", &other).unwrap();
            assert_eq!(Record::parse(&read("s").unwrap().render()), other);
            assert_eq!(host("s"), None, "recorded under another process");
            assert!(is_enrolled("s"), "the enrollment itself still holds");
        });
    }
}
