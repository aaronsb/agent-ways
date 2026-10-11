//! Per-session "last inbound signal" tracking.
//!
//! Sensor-peers and the drain write the most-recently-surfaced peer signal
//! here so that `attend reply` can auto-thread a response without the
//! calling agent having to discover the id. The state is scoped to the
//! running attend process's own Claude session id, so multiple concurrent
//! sessions don't step on each other's reply targets.
//!
//! Wire format is plain text: the signal id (filename stem, the same shape
//! `re:<id>` references) on the first line, and where it was said on the
//! second (see [`Said`]). The second line lets a reply stay in the parent's
//! room after the parent's file is gone. A file with only the id, as written
//! before the room was recorded, still reads, with no room.
//!
//! Path: `~/.cache/attend/state/<session_id>.last-inbound`
//!
//! # Invariants
//!
//! **One attend agent per canonicalized cwd.** The `record()` call site in
//! `lib.rs::read_signals` filters incoming signals by comparing the
//! parsed `source_cwd` against the observer's own `focus.working_dir`,
//! and skips recording when they match. This prevents a previous
//! incarnation of the same agent (new session uuid, same cwd) from
//! polluting last_inbound and causing `attend reply` to auto-thread to
//! the agent's own past self. The filter is correct as long as two
//! simultaneous attend agents are not running in the same canonicalized
//! working directory. Claude Code's one-agent-per-project norm satisfies
//! this in practice; if that ever changes, this filter needs to grow a
//! session-uuid-set check keyed off `~/.claude/sessions/*.json` metadata
//! instead of a bare cwd comparison.

use attend_groups::Room;
use std::fs;
use std::path::PathBuf;

/// Where an inbound message was said, as a reply to it needs to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// `#open`.
    Open,
    /// A channel, by name.
    Channel(String),
    /// This project's tray, sent with `--to`. `Some(cwd)` when a Claude
    /// session at `cwd` sent it, so a reply goes back to that project;
    /// `None` for a sender outside Claude Code, whose shell no session
    /// listens at.
    Project(Option<String>),
}

impl Said {
    /// Where a signal from `from`, working at `cwd`, was said when it was
    /// found in `room`.
    pub fn of(room: &Room, from: &str, cwd: &str) -> Self {
        match room {
            Room::Open => Self::Open,
            Room::Channel(name) => Self::Channel(name.clone()),
            Room::Project => Self::Project(from.starts_with("claude:").then(|| cwd.to_string())),
        }
    }

    /// Whether `cwd` names a project a reply can go back to: non-empty, and
    /// one line, so it survives the record's line framing intact.
    pub fn routable_cwd(cwd: &str) -> bool {
        !cwd.is_empty() && !cwd.contains(['\n', '\r'])
    }

    fn encode(&self) -> String {
        match self {
            Self::Open => "#open".to_string(),
            Self::Channel(name) => format!("@{name}"),
            Self::Project(None) => "project".to_string(),
            Self::Project(Some(cwd)) => format!("project {cwd}"),
        }
    }

    fn decode(line: &str) -> Option<Self> {
        match line {
            "#open" => Some(Self::Open),
            "project" => Some(Self::Project(None)),
            _ => line
                .strip_prefix('@')
                .filter(|name| !name.is_empty())
                .map(|name| Self::Channel(name.to_string()))
                .or_else(|| {
                    line.strip_prefix("project ")
                        .filter(|cwd| Self::routable_cwd(cwd))
                        .map(|cwd| Self::Project(Some(cwd.to_string())))
                }),
        }
    }
}

/// The newest inbound signal a conduit surfaced to a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastInbound {
    pub id: String,
    /// Where it was said; `None` for a record written before the room was
    /// kept, or one whose room line does not parse.
    pub said: Option<Said>,
}

/// Build the path that stores the last-inbound signal id for the given
/// attend-owner session id. Does not create the directory.
pub fn path(session_id: &str) -> PathBuf {
    attend_presence::cache::state_dir().join(format!("{session_id}.last-inbound"))
}

/// Record `signal_id`, said in `said`, as the most-recent inbound signal
/// for `session_id`. Creates the parent directory if needed. A project room
/// whose cwd is not [`Said::routable_cwd`] is not recorded: the record keeps
/// the id alone, and a reply to it is refused rather than guessed.
///
/// Errors are ignored: the callers (the peers sensor, the drain) cannot
/// usefully recover from a failed write here, and a missing file on the
/// reply side degrades to "no threaded reply," not a crash.
pub fn record(session_id: &str, signal_id: &str, said: &Said) {
    let p = path(session_id);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).ok();
    }
    let content = match said {
        Said::Project(Some(cwd)) if !Said::routable_cwd(cwd) => format!("{signal_id}\n"),
        _ => format!("{signal_id}\n{}\n", said.encode()),
    };
    fs::write(&p, content).ok();
}

/// Read the last-inbound signal for `session_id`, if one exists and its id
/// line is non-empty. The id line is trimmed in case of a hand-edit; the
/// room line loses only its line terminator, so a cwd reads back exactly.
pub fn read(session_id: &str) -> Option<LastInbound> {
    let raw = fs::read_to_string(path(session_id)).ok()?;
    let mut lines = raw.lines();
    let id = lines.next().map(str::trim).filter(|id| !id.is_empty())?.to_string();
    let said = lines.next().and_then(Said::decode);
    Some(LastInbound { id, said })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_contains_session_id_and_ext() {
        let p = path("test-session-123");
        assert!(p.to_string_lossy().contains("test-session-123.last-inbound"));
        assert!(p.starts_with(attend_presence::cache::state_dir()));
    }

    #[test]
    fn record_then_read_roundtrips() {
        // Use a unique session id to avoid colliding with any real state.
        let sid = format!("test-ri-{}", std::process::id());
        for said in [
            Said::Open,
            Said::Channel("deploy".to_string()),
            Said::Project(None),
            Said::Project(Some("/home/a/proj x".to_string())),
        ] {
            record(&sid, "parent-abc-1234", &said);
            assert_eq!(read(&sid), Some(LastInbound { id: "parent-abc-1234".to_string(), said: Some(said) }));
        }
        // Clean up.
        fs::remove_file(path(&sid)).ok();
    }

    #[test]
    fn a_record_with_only_the_id_reads_with_no_room() {
        let sid = format!("test-ri-old-{}", std::process::id());
        fs::create_dir_all(path(&sid).parent().unwrap()).unwrap();
        for old in ["parent-abc-1234", "parent-abc-1234\n", "parent-abc-1234\nsomething else\n"] {
            fs::write(path(&sid), old).unwrap();
            assert_eq!(read(&sid), Some(LastInbound { id: "parent-abc-1234".to_string(), said: None }), "{old:?}");
        }
        fs::remove_file(path(&sid)).ok();
    }

    /// A project room with an empty or multi-line cwd names no project to
    /// answer in, so it is never recorded, and one already on disk reads as
    /// no room: never as an external sender's, whose answer goes to `#open`.
    #[test]
    fn a_project_room_without_a_one_line_cwd_reads_as_no_room() {
        let sid = format!("test-ri-cwd-{}", std::process::id());
        let no_room = Some(LastInbound { id: "parent-abc-1234".to_string(), said: None });
        for cwd in ["", "/a\nb", "/a\r", "/a\rb"] {
            record(&sid, "parent-abc-1234", &Said::Project(Some(cwd.to_string())));
            assert_eq!(read(&sid), no_room, "{cwd:?}");
        }
        for on_disk in ["parent-abc-1234\nproject \n", "parent-abc-1234\nproject \r\n"] {
            fs::write(path(&sid), on_disk).unwrap();
            assert_eq!(read(&sid), no_room, "{on_disk:?}");
        }
        // Spaces in a cwd are its own; only the line terminator goes.
        record(&sid, "parent-abc-1234", &Said::Project(Some("/a b ".to_string())));
        assert_eq!(read(&sid).and_then(|l| l.said), Some(Said::Project(Some("/a b ".to_string()))));
        fs::remove_file(path(&sid)).ok();
    }

    #[test]
    fn said_follows_the_room_and_the_sender() {
        assert_eq!(Said::of(&Room::Open, "claude:s", "/p"), Said::Open);
        assert_eq!(Said::of(&Room::Channel("c".to_string()), "claude:s", "/p"), Said::Channel("c".to_string()));
        assert_eq!(Said::of(&Room::Project, "claude:s", "/p"), Said::Project(Some("/p".to_string())));
        assert_eq!(Said::of(&Room::Project, "external:me@kitty", "/home/me"), Said::Project(None));
    }

    #[test]
    fn read_missing_returns_none() {
        assert_eq!(read("definitely-not-a-real-session-id-zzz-9999"), None);
    }
}
