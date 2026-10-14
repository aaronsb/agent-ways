//! Signal wire format, paths, and I/O for `attend-chat`.
//!
//! The TUI is a first-class endpoint on the signal bus — it reads and
//! writes the same `.signal` files the CLI does. The line parser and the
//! filename are shared through agent-identity; the paths and I/O around
//! them are duplicated here.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
#[allow(dead_code)] // `id`/`cwd`/`reply_to` land when the sidebar and
                   // threading UI ship in follow-up ADR-120 PRs.
pub struct Signal {
    pub id: String,
    pub from: String,
    pub project: String,
    pub cwd: String,
    pub reply_to: Option<String>,
    pub message: String,
    pub ts: u64,
    /// Which channel this signal arrived on — derived from the inbox
    /// dir it was read from, never from the body (wire format
    /// unchanged). Drives the per-tab transcript filter (#393).
    pub channel: Channel,
}

impl Signal {
    /// The sender's membership id (ADR-170): a claude's session id, a
    /// human's sanitized username. `None` for a local notice.
    pub fn sender_member(&self) -> Option<String> {
        if let Some(sid) = self.from.strip_prefix("claude:") {
            return Some(sid.to_string());
        }
        let rest = self.from.strip_prefix("external:")?;
        Some(agent_identity::sanitize_id_component(rest.split('@').next().unwrap_or(rest)))
    }

    /// Whether this is a direct message between the person at the chat and
    /// `member`, either way: one `member` sent to the person's tray, or one
    /// the person sent with `member` among its recipients.
    pub fn is_dm_with(&self, member: &str) -> bool {
        match &self.channel {
            Channel::Person => self.sender_member().as_deref() == Some(member),
            Channel::Sent(to) => to.split(',').any(|t| t == member),
            _ => false,
        }
    }

    /// Whether this is a direct message `member` sent or received, with
    /// anyone: what the agent's own DM view holds (ADR-405, Review DMs).
    /// One to the person is sent by it; any other names its recipients in
    /// `to`.
    pub fn is_dm_of(&self, member: &str) -> bool {
        let to = match &self.channel {
            Channel::Person => "",
            Channel::Direct(to) | Channel::Sent(to) | Channel::Elsewhere(to) => to,
            Channel::Open | Channel::Group(_) | Channel::Info => return false,
        };
        self.sender_member().as_deref() == Some(member) || to.split(',').any(|t| t == member)
    }

    /// Whether this is the same send as `other`, read from another tray: a
    /// message the person addressed to agents in several projects is
    /// written once to each project's tray, every copy with the whole `to`.
    /// Copies are written together, so their times differ by a second at
    /// most.
    pub fn same_send(&self, other: &Signal) -> bool {
        matches!((&self.channel, &other.channel), (Channel::Sent(a), Channel::Sent(b)) if a == b)
            && self.from == other.from
            && self.message == other.message
            && self.ts.abs_diff(other.ts) <= 1
    }
}

/// Provenance of a signal, at inbox-dir granularity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Channel {
    /// `_broadcast/` — the base `#open` channel.
    Open,
    /// `@<name>/` — a named channel.
    Group(String),
    /// A direct message to the chat's own project tray, from someone else:
    /// its `to`, the comma-separated session ids it went to (ADR-401, `to`),
    /// empty on a line with no envelope.
    Direct(String),
    /// A direct message the person at the chat sent, read back from the
    /// tray it went to, whichever project's: its `to`, as above.
    Sent(String),
    /// The person tray of the person at this chat, `_people/<member-id>/`:
    /// a reply or a direct message to them (ADR-401, `to`). Shown in every
    /// view.
    Person,
    /// A direct message between others, read from a tray that is not the
    /// chat's: another project's, or another person's. Its `to`, as above,
    /// or the person's member id. No view of the person's shows it.
    Elsewhere(String),
    /// Local-only status blocks (`/peers` output) — never on the bus,
    /// rendered in every tab so command feedback can't hide.
    Info,
}

pub fn signals_base() -> PathBuf {
    attend_presence::cache::signals_dir()
}

pub fn broadcast_dir() -> PathBuf {
    signals_base().join(attend_groups::BROADCAST_DIR)
}

/// Encode a cwd path into the signal directory name the peer sensor
/// scans: `claude_sessions::attend_key`, the one name `attend` and the
/// sensor use too.
pub fn encode_cwd(path: &str) -> String {
    claude_sessions::attend_key(path)
}

/// Directory that delivers signals to the claude session rooted at
/// `cwd`. The peer sensor scans `signals_base/<encoded>/` for
/// messages addressed specifically to it.
pub fn cwd_dir(cwd: &str) -> PathBuf {
    signals_base().join(encode_cwd(cwd))
}

/// The trays the chat reads as its own: its project's tray, by its encoded
/// name, and the person tray of the person at the chat, by member id.
/// Either is empty for none.
#[derive(Clone, Debug, Default)]
pub struct Own {
    pub tray: String,
    pub person: String,
}

/// Parse a `.signal` file through the shared ADR-120 parser
/// (`agent_identity::parse_signal`), so a `re:` prefix that is not a signal
/// id reads as legacy prose here exactly as it does in the attend CLI, and
/// classify it by the tray it sits in against `own` ([`Channel`]).
/// Returns `None` for anything that doesn't look like a signal we can render.
pub fn parse_file(path: &Path, own: &Own) -> Option<Signal> {
    if path.extension().and_then(|s| s.to_str()) != Some("signal") {
        return None;
    }
    let raw = agent_identity::signal::read_signal_file(path).ok()?;
    let parsed = agent_identity::parse_signal(agent_identity::wire_line(&raw))?;

    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();
    let ts = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mine = !own.person.is_empty() && parsed.from_kind == Some("human") && parsed.from_id == Some(own.person.as_str());
    Some(Signal {
        id,
        from: parsed.from.to_string(),
        project: parsed.project.to_string(),
        cwd: parsed.cwd.to_string(),
        reply_to: parsed.reply_to.map(str::to_string),
        message: parsed.message.to_string(),
        ts,
        channel: channel_for_path(path, own, mine, parsed.to.unwrap_or("")),
    })
}

/// Classify a signal file's channel by the tray it sits in, the same
/// first-component logic `watcher::accept_path` admits by. `#open` and a
/// channel are what they are. A direct message is the person's own when
/// `mine`, read back from whichever tray it went to; else it reaches the
/// person (their person tray), the chat's project (its tray), or neither.
/// `to` is the envelope's.
fn channel_for_path(path: &Path, own: &Own, mine: bool, to: &str) -> Channel {
    let dir = path
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let tray_parent = path.parent().and_then(Path::parent).and_then(|d| d.file_name()).and_then(|n| n.to_str());
    if tray_parent == Some(attend_groups::PEOPLE_DIR) {
        if dir == own.person {
            Channel::Person
        } else if mine {
            Channel::Sent(to.to_string())
        } else {
            Channel::Elsewhere(if to.is_empty() { dir } else { to }.to_string())
        }
    } else if dir == attend_groups::BROADCAST_DIR {
        Channel::Open
    } else if let Some(g) = dir.strip_prefix('@') {
        Channel::Group(g.to_string())
    } else if mine {
        Channel::Sent(to.to_string())
    } else if !own.tray.is_empty() && dir == own.tray {
        Channel::Direct(to.to_string())
    } else {
        Channel::Elsewhere(to.to_string())
    }
}

/// Write a broadcast signal to `_broadcast/`, `to = *`. The line, the
/// sender identity and the atomic write are `agent_identity::signal`'s,
/// shared with `attend message send`. Threaded replies (`re:<id>`) are produced by
/// `attend message send` only; the TUI does not originate threaded sends yet.
pub fn write_broadcast(message: &str) -> io::Result<String> {
    write_signal(&broadcast_dir(), message, "*")
}

/// Write a signal into an arbitrary destination directory, addressed to
/// `to` (ADR-401): `*` for `#open`, `#<name>` for a channel, the session
/// ids of a direct message. The broadcast and directed (`@Nickname`)
/// paths both ride this — same wire format, same atomic tmp+rename, only
/// the target differs. attend-chat writes as the person at its keyboard,
/// `from_kind = human`.
pub fn write_signal(dest: &Path, message: &str, to: &str) -> io::Result<String> {
    // The same 64 KiB limit `attend message send` keeps (ADR-412); the compose box
    // shows the refusal and keeps the draft.
    agent_identity::signal::check_message_len(message)?;
    let (sender_id, from, project, cwd) = sender_identity();

    // Build the filename stem (== signal id that `re:<id>` replies
    // reference). `signal_filename` normalizes the sender id — the TUI's
    // sender is `$USER@<terminal>`, whose raw `@`/`.` would fail
    // `is_valid_signal_id` and break `attend message reply` auto-threading
    // (issue #368) — and makes the name collision-proof. `from` keeps the
    // raw id.
    let filename = agent_identity::signal_filename(&sender_id);
    let member = human_member_id();
    let envelope = agent_identity::Envelope { from_kind: Some("human"), from_id: Some(&member), on_behalf_of: None, to: Some(to) };
    let content = agent_identity::signal::format_tagged(&from, &project, &cwd, &envelope, None, message);
    agent_identity::signal::write_signal_file(dest, &filename, &content)?;
    Ok(filename)
}

/// Compose a local-only transcript status block (`/peers` output and
/// kin). Rendered like any message cell but never written to the bus.
/// `from` carries no wire prefix on purpose: `known_identities` skips
/// unknown prefixes, so a status block can't pollute the legend or
/// `@`-completion, and the chip falls through to the raw-value branch
/// (`attend` / `<kind>`) — visually distinct from every real sender.
pub fn compose_status_block(kind: &str, body: &str) -> Signal {
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Signal {
        id: format!("local-status-{ts}"),
        from: "attend".to_string(),
        project: kind.to_string(),
        cwd: String::new(),
        reply_to: None,
        message: body.to_string(),
        ts,
        channel: Channel::Info,
    }
}

/// Derive this session's wire identity fields: `(sender_id, from,
/// project, cwd)`, as `write_signal` writes them.
///
/// The sender is `agent_identity::signal::identify_sender` with no
/// session: attend-chat is the human's coordination surface, so it is
/// `$USER@<terminal>`. `project` is `agent_identity::signal::project_label`.
fn sender_identity() -> (String, String, String, String) {
    let (sender_id, kind) = agent_identity::signal::identify_sender(None);
    let from = format!("{}:{}", kind, sender_id);
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let project = agent_identity::signal::project_label(&cwd);
    (sender_id, from, project, cwd)
}

/// Focus-group membership identity for the human at the keyboard
/// (ADR-170): the sanitized username alone, no terminal suffix. One
/// entry per human regardless of terminal or cwd — the same dedupe
/// rule the chip registry applies to external senders. This is the
/// member id written into `_groups.yaml` by `/join` and the heartbeat
/// key the TUI touches while running.
pub fn human_member_id() -> String {
    agent_identity::sanitize_id_component(&agent_identity::signal::user_name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tmp_signal(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(format!("{}.signal", name));
        let mut f = fs::File::create(&p).unwrap();
        writeln!(f, "{}", body).unwrap();
        p
    }

    #[test]
    fn parses_legacy_format() {
        let d = tempdir_like();
        let p = tmp_signal(&d, "sig-1", "claude:abc|proj|/home/x|hello world");
        let s = parse_file(&p, &own()).unwrap();
        assert_eq!(s.from, "claude:abc");
        assert_eq!(s.project, "proj");
        assert_eq!(s.cwd, "/home/x");
        assert_eq!(s.message, "hello world");
        assert!(s.reply_to.is_none());
    }

    #[test]
    fn parses_threaded_format() {
        let d = tempdir_like();
        let p = tmp_signal(&d, "sig-2", "claude:abc|proj|/home/x|re:abc123|reply body");
        let s = parse_file(&p, &own()).unwrap();
        assert_eq!(s.reply_to.as_deref(), Some("abc123"));
        assert_eq!(s.message, "reply body");
    }

    #[test]
    fn preserves_pipes_in_legacy_body() {
        let d = tempdir_like();
        let p = tmp_signal(&d, "sig-3", "claude:abc|proj|/home/x|a | b");
        let s = parse_file(&p, &own()).unwrap();
        assert_eq!(s.message, "a | b");
        assert!(s.reply_to.is_none());
    }

    #[test]
    fn prose_starting_re_with_a_pipe_stays_legacy() {
        // `re: prose` is not a signal id (it has a space), so the attend CLI
        // reads this as a legacy message; the TUI must read it the same way.
        let d = tempdir_like();
        let p = tmp_signal(&d, "sig-4", "claude:abc|proj|/home/x|re: prose|x");
        let s = parse_file(&p, &own()).unwrap();
        assert!(s.reply_to.is_none());
        assert_eq!(s.message, "re: prose|x");
    }

    /// The chat's own trays, as the tests below name them.
    fn own() -> Own {
        Own { tray: "-home-me-proj".into(), person: human_member_id() }
    }

    #[test]
    fn parse_classifies_channel_by_inbox_dir() {
        // Provenance drives the per-tab filter (#393): broadcast → Open,
        // `@g/` → Group, the chat's own cwd tray → Direct, the person's own
        // tray → Person, any other tray → Elsewhere.
        let d = tempdir_like();
        let bdir = d.join("_broadcast");
        let gdir = d.join("@deploy");
        let cdir = d.join("-home-me-proj");
        let odir = d.join("-home-someone-else");
        let pdir = d.join("_people").join(human_member_id());
        let qdir = d.join("_people").join("bob");
        for dir in [&bdir, &gdir, &cdir, &odir, &pdir, &qdir] {
            fs::create_dir_all(dir).unwrap();
        }
        let body = "claude:abc|proj|/home/x|hi";
        let channel = |dir: &Path| parse_file(&tmp_signal(dir, "s", body), &own()).unwrap().channel;
        assert_eq!(channel(&bdir), Channel::Open);
        assert_eq!(channel(&gdir), Channel::Group("deploy".into()));
        assert_eq!(channel(&cdir), Channel::Direct(String::new()));
        assert_eq!(channel(&odir), Channel::Elsewhere(String::new()));
        assert_eq!(channel(&pdir), Channel::Person);
        assert_eq!(channel(&qdir), Channel::Elsewhere("bob".into()), "another person's tray, to that person");
        let none = Own::default();
        assert_eq!(parse_file(&tmp_signal(&cdir, "s", body), &none).unwrap().channel, Channel::Elsewhere(String::new()), "no tray of its own");
    }

    /// A direct message the person at this chat sent, read back from any
    /// tray, the chat's own or another project's or person's, is what it
    /// sent, to whom: the DM view of its recipient shows it. Anyone else's
    /// is direct in the chat's own tray and elsewhere in any other, with
    /// its `to`.
    #[test]
    fn a_dm_the_person_sent_reads_back_as_sent_from_any_tray() {
        let d = tempdir_like();
        let me = human_member_id();
        let line = |kind: &str, id: &str| {
            let env = agent_identity::Envelope { from_kind: Some(kind), from_id: Some(id), on_behalf_of: None, to: Some("s1") };
            agent_identity::signal::format_tagged("external:x@pts", "alpha", "/work/alpha", &env, None, "hello")
        };
        for (tray, theirs) in [("-home-me-proj", Channel::Direct("s1".into())), ("-work-alpha", Channel::Elsewhere("s1".into()))] {
            let dir = d.join(tray);
            fs::create_dir_all(&dir).unwrap();
            let channel = |name: &str, body: &str| parse_file(&tmp_signal(&dir, name, body), &own()).unwrap().channel;
            assert_eq!(channel("mine", &line("human", &me)), Channel::Sent("s1".into()), "{tray}");
            assert_eq!(channel("theirs", &line("human", "someone-else")), theirs, "{tray}");
            assert_eq!(channel("agent", &line("agent", &me)), theirs, "{tray}: an agent with the person's id is not the person");
        }
        let bob = d.join("_people").join("bob");
        fs::create_dir_all(&bob).unwrap();
        assert_eq!(parse_file(&tmp_signal(&bob, "mine", &line("human", &me)), &own()).unwrap().channel, Channel::Sent("s1".into()), "to another person");
    }

    #[test]
    fn write_then_parse_roundtrip() {
        // Point $HOME at a temp dir so broadcast_dir() resolves there
        // instead of the real cache, then assert the writer's output
        // is accepted by our own parser. Guards against silent wire-
        // format drift between the TUI's send path and its watcher.
        let _env = crate::test_dir::env_lock();
        let home = tempdir_like();
        // `set_var` is !Send on some platforms but this test is
        // single-threaded; Cargo isolates by default.
        std::env::set_var("HOME", &home);
        // The attend cache follows XDG_CACHE_HOME first: point it into the
        // temp home too, or a runner's own setting sends the write to the
        // real cache.
        std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));

        let filename = write_broadcast("round-trip body").unwrap();
        let path = broadcast_dir().join(&filename);
        assert!(path.starts_with(&home), "the write stays in the temp home: {}", path.display());
        let sig = parse_file(&path, &own()).expect("written signal must parse");
        assert_eq!(sig.message, "round-trip body");
        assert!(sig.from.starts_with("external:"));
        let raw = std::fs::read_to_string(&path).unwrap();
        let wire = agent_identity::parse_signal(agent_identity::wire_line(&raw)).unwrap();
        assert_eq!((wire.to, wire.from_kind), (Some("*"), Some("human")), "{raw}");
        assert!(sig.reply_to.is_none());
    }

    /// An agent's own DMs: every direct message it sent or received, the
    /// person's among them, and no channel post.
    #[test]
    fn an_agents_dms_are_those_it_sent_or_received() {
        let on = |from: &str, channel: Channel| Signal { id: "s".into(), from: format!("claude:{from}"), project: String::new(), cwd: String::new(), reply_to: None, message: String::new(), ts: 0, channel };
        let me = |channel: Channel| Signal { from: "external:aaron@pts".into(), ..on("x", channel) };
        assert!(on("a1", Channel::Person).is_dm_of("a1"), "to the person");
        assert!(me(Channel::Sent("a2,a1".into())).is_dm_of("a1"), "from the person");
        assert!(on("a2", Channel::Direct("a1".into())).is_dm_of("a1"), "to it, in the chat's tray");
        assert!(on("a2", Channel::Elsewhere("a1".into())).is_dm_of("a1"), "to it, elsewhere");
        assert!(on("a1", Channel::Elsewhere("a2".into())).is_dm_of("a1"), "from it, elsewhere");
        assert!(on("a1", Channel::Elsewhere("bob".into())).is_dm_of("a1"), "from it, to another person");
        assert!(!on("a2", Channel::Elsewhere("a3".into())).is_dm_of("a1"), "between others");
        assert!(!on("a2", Channel::Person).is_dm_of("a1"));
        for c in [Channel::Open, Channel::Group("g".into()), Channel::Info] {
            assert!(!on("a1", c.clone()).is_dm_of("a1"), "{c:?}");
        }
    }

    /// The copies of one send, read from the trays it went to, are one
    /// send; the same words sent again later, or to others, are not.
    #[test]
    fn the_copies_of_one_send_are_one_send() {
        let sent = |to: &str, ts: u64| Signal { id: format!("{to}-{ts}"), from: "external:aaron@pts".into(), project: String::new(), cwd: String::new(), reply_to: None, message: "hi".into(), ts, channel: Channel::Sent(to.into()) };
        assert!(sent("s1,s2", 100).same_send(&sent("s1,s2", 101)));
        assert!(!sent("s1,s2", 100).same_send(&sent("s1,s2", 160)), "sent again later");
        assert!(!sent("s1,s2", 100).same_send(&sent("s1", 100)), "to others");
        let mut direct = sent("s1,s2", 100);
        direct.channel = Channel::Direct("s1,s2".into());
        assert!(!direct.same_send(&sent("s1,s2", 100)), "only the person's own sends");
    }

    #[test]
    fn encode_cwd_is_the_attend_key() {
        // The name attend::util::encode_project and the peer sensor use
        // for a cwd's tray; drift here breaks direct routing silently.
        assert_eq!(encode_cwd("/srv/my proj"), "-srv-my-proj-bte5w6");
        assert_eq!(encode_cwd(""), "");
    }

    #[test]
    fn cwd_dir_is_under_signals_base() {
        // Point $HOME at a temp dir so signals_base resolves there.
        let _env = crate::test_dir::env_lock();
        let home = tempdir_like();
        std::env::set_var("HOME", &home);
        let d = cwd_dir("/home/aaron/proj");
        assert!(
            d.starts_with(signals_base()),
            "cwd_dir {d:?} should live under {:?}",
            signals_base()
        );
        assert_eq!(
            d.file_name().and_then(|s| s.to_str()),
            Some("-home-aaron-proj-wjpr18")
        );
    }

    fn tempdir_like() -> std::path::PathBuf {
        crate::test_dir::unique("signal")
    }

    /// A body over the shared 64 KiB limit is refused and nothing is
    /// written; one at the limit goes out (ADR-412).
    #[test]
    fn a_body_over_the_limit_is_refused_and_nothing_is_written() {
        let dir = tempdir_like();
        let max = agent_identity::signal::MAX_MESSAGE_BYTES;
        let err = write_signal(&dir, &"x".repeat(max + 1), "*").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("64 KiB"), "{err}");
        assert!(!dir.exists() || fs::read_dir(&dir).unwrap().next().is_none(), "nothing written");
        write_signal(&dir, &"x".repeat(max), "*").unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
