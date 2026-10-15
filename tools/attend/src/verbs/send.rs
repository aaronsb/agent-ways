//! `send` and `reply`: write one signal to the bus.

use crate::util::{current_session, encode_project, groups_for, signals_base, Session};
use agent_identity::is_valid_signal_id;
use serde::Serialize;
use std::path::PathBuf;

/// A message to send. With no routing field it goes to `#open`.
#[derive(Debug, Clone, Default)]
pub struct SendRequest {
    pub message: String,
    /// Who it is for: a session's name or id, a person's member id, or a
    /// project path. It is a direct message (ADR-401, `to`).
    pub to: Option<String>,
    /// A channel name, with or without `#`: only its members receive it.
    /// `open` is `#open`. With `to` as well, the post stays in the channel,
    /// addressed to whom `to` names.
    pub channel: Option<String>,
    /// A signal id to thread under. `reply` fills it from the newest peer
    /// message when it is empty.
    pub re: Option<String>,
}

/// A message written to the bus: one signal, whatever its length (ADR-412).
#[derive(Debug, Clone, Serialize)]
pub struct Sent {
    /// The signal's id, which a reply to it threads under.
    pub id: String,
    /// `#open` for a broadcast, `channel`, or `directed` for a direct
    /// message.
    pub scope: &'static str,
    /// The signal's `to`: `*`, `#<channel>`, or the recipients' ids.
    pub to: String,
    /// The id this one threads under, if any.
    pub re: Option<String>,
    /// How many trays it was addressed to.
    pub dirs: usize,
    /// Trays the write failed in while another tray took it. A message no
    /// tray took is [`SendError::NotWritten`].
    pub failed: Vec<WriteFailure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WriteFailure {
    pub dir: PathBuf,
    pub error: String,
}

/// A live peer, as the `--to` refusal lists it.
#[derive(Debug, Clone, Serialize)]
pub struct PeerAt {
    pub cwd: String,
    pub project: String,
}

/// A channel with its recorded member count, as the `--channel` refusal
/// lists it. The count is the file's; live members may be fewer.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelCount {
    pub name: String,
    pub members: usize,
    pub pinned: bool,
}

/// Why a send or reply wrote nothing.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendError {
    /// `re` is empty or outside `[A-Za-z0-9_-]+`.
    InvalidReId,
    EmptyMessage,
    /// The body is longer than [`agent_identity::signal::MAX_MESSAGE_BYTES`].
    TooLong { bytes: usize, max: usize },
    /// No live peer works at the `to` path.
    NoPeerAt { path: String, peers: Vec<PeerAt>, suggestion: Option<String> },
    /// The `to` name is no live session and no person attend knows.
    NoRecipient { name: String, sessions: Vec<String> },
    /// The `to` name matches more than one session, or a session and a
    /// person.
    AmbiguousRecipient { name: String, candidates: Vec<String> },
    /// A channel post addressed to someone who is not a member of the
    /// channel: they would never read it.
    NotInChannel { name: String, channel: String },
    /// The message being answered came from a session that has exited.
    SenderExited { session: String, name: String },
    /// The `to` path is this session's own project, and no other session
    /// works there, so nobody would receive the message.
    OwnProject { path: String },
    /// A project path where one session is wanted: it names every session
    /// working there.
    ProjectPath { path: String },
    /// No channel has the `channel` name.
    NoChannel { name: String, channels: Vec<ChannelCount> },
    /// The channel exists but no live peer is in it; `only_self` when the
    /// sender is its one listener.
    NoLivePeers { name: String, only_self: bool, channels: Vec<ChannelCount> },
    /// `reply` found no peer message to thread under.
    NoInbound,
    /// `re` is a short `~id` handle that names no message this session
    /// receives.
    UnknownHandle { handle: String },
    /// The parent was said in a channel this session has since left. A
    /// reply anywhere else would carry a channel's answer out of it.
    ParentChannelLeft { channel: String },
    /// The parent was said in a channel that no longer exists.
    ParentChannelGone { channel: String },
    /// Nothing says where the parent was said: its file is gone from every
    /// tray and channel, and the record names no room. Guessing `#open`
    /// could carry a private answer into public.
    ParentRoomUnknown,
    /// No tray took the signal, so nothing went out.
    NotWritten { error: String },
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidReId => write!(f, "re must be a signal id matching [A-Za-z0-9_-]+"),
            Self::EmptyMessage => write!(f, "the message is empty"),
            Self::TooLong { bytes, max } => agent_identity::signal::TooLong { bytes: *bytes, max: *max }.fmt(f),
            Self::NoPeerAt { path, .. } => write!(f, "no active peer at {path}"),
            Self::NoRecipient { name, sessions } if sessions.is_empty() => {
                write!(f, "no live session or person is named '{name}'; drop --to to post in #open")
            }
            Self::NoRecipient { name, sessions } => write!(
                f,
                "no live session or person is named '{name}'; live sessions: {}; drop --to to post in #open",
                sessions.join(", ")
            ),
            Self::AmbiguousRecipient { name, candidates } => {
                write!(f, "'{name}' names more than one recipient: {}; name one of them", candidates.join(", "))
            }
            Self::NotInChannel { name, channel } => write!(f, "@{name} is not in #{channel}; nothing sent"),
            Self::SenderExited { session, name } => write!(
                f,
                "the message you are replying to came from {name} (session {session}), which has exited; nothing was sent"
            ),
            Self::OwnProject { path } => write!(
                f,
                "{path} is this session's own project and no other session works there, so nobody would receive it; drop --to to reach every peer, or name another project"
            ),
            Self::ProjectPath { path } => write!(f, "{path} is a project path; name one session there"),
            Self::NoChannel { name, .. } => write!(f, "no channel named '{name}'"),
            Self::NoLivePeers { name, only_self: true, .. } => {
                write!(f, "no live peers in channel '{name}' (you are the only listener)")
            }
            Self::NoLivePeers { name, .. } => write!(f, "no live peers in channel '{name}'"),
            Self::NoInbound => write!(f, "no prior inbound signal to thread against; to start a new topic, use attend message send"),
            Self::UnknownHandle { handle } => write!(
                f,
                "no message this session receives has the handle ~{handle}; drop re to answer the newest, or pass the full id (attend message inbox lists them)"
            ),
            Self::ParentChannelLeft { channel } => write!(
                f,
                "the message you are replying to was said in channel '{channel}', which you have left; rejoin it (attend channel join {channel}), pass --channel or --to, or use attend message send"
            ),
            Self::ParentChannelGone { channel } => write!(
                f,
                "the message you are replying to was said in channel '{channel}', which no longer exists; pass --channel or --to, or use attend message send"
            ),
            Self::ParentRoomUnknown => {
                write!(f, "can't tell where the parent was said; pass --channel or --to, or use attend message send")
            }
            Self::NotWritten { error } => write!(f, "the message could not be written: {error}"),
        }
    }
}

impl std::error::Error for SendError {}

/// Write `req.message` to the bus as this session. The sender is the Claude
/// Code session found by process ancestry, else the external `$USER@<terminal>`
/// identity a plain terminal has.
pub fn send(req: &SendRequest) -> Result<Sent, SendError> {
    let me = current_session();
    check(req)?;
    let dest = resolve(&me, req)?;
    write(&me, &dest, req)
}

/// Where a message goes, resolved, with the `to` it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Dest {
    /// `#open`. `to` is `*`, or the ids of those a post there is
    /// addressed to.
    Open { to: Option<String> },
    /// A channel. `to` is `#<name>`, or for a reply the parent's sender.
    Channel { name: String, to: Option<String> },
    /// A direct message to live sessions at one project: its tray, `to`
    /// their session ids.
    Sessions { cwd: String, ids: Vec<String> },
    /// A direct message to a person: their person tray, `to` their member
    /// id.
    Person(String),
}

/// Refuse what no route can carry: a malformed `re`, an empty or
/// oversized body.
fn check(req: &SendRequest) -> Result<(), SendError> {
    // A signal id must match the same character class the parser uses to
    // disambiguate threaded records from legacy messages that happen to
    // start with "re:". Signal filename stems are
    // `<sanitized-sender>-<nanos>-<seq>` (see `agent_identity::signal_filename`),
    // so `[A-Za-z0-9_-]+` comfortably covers the real shape and rejects
    // anything that would break the wire format (pipes, whitespace,
    // control chars) or trip the ambiguity fence in parse_signal.
    if req.re.as_deref().is_some_and(|id| !is_valid_signal_id(id)) {
        return Err(SendError::InvalidReId);
    }
    if req.message.is_empty() {
        return Err(SendError::EmptyMessage);
    }
    agent_identity::signal::check_message_len(&req.message).map_err(|t| SendError::TooLong { bytes: t.bytes, max: t.max })
}

/// The destination `req` names: its channel, else its `to`, else `#open`
/// (ADR-406 item 10).
fn resolve(me: &Session, req: &SendRequest) -> Result<Dest, SendError> {
    if let Some(channel) = &req.channel {
        // A post in a channel that also names someone stays in the channel,
        // addressed to them, so it wakes them; every member still reads it
        // (ADR-401, `to`).
        let to = req.to.as_deref().map(|to| resolve_recipient(me, to).map(|d| d.addressee())).transpose()?;
        let name = channel.trim().trim_start_matches(['#', '@']);
        if name == attend_groups::BASE_CHANNEL {
            return Ok(Dest::Open { to });
        }
        check_channel(me, name)?;
        // Only members read a channel: a post addressed to someone outside
        // it would reach no one, so it is refused rather than sent.
        if let (Some(ids), Some(named)) = (&to, &req.to) {
            let members = groups_for(me).members(name).unwrap_or_default();
            if ids.split(',').any(|id| !members.iter().any(|m| m == id)) {
                return Err(SendError::NotInChannel { name: named.trim().to_string(), channel: name.to_string() });
            }
        }
        return Ok(Dest::Channel { name: name.to_string(), to });
    }
    match &req.to {
        Some(to) => resolve_recipient(me, to),
        None => Ok(Dest::Open { to: None }),
    }
}

impl Dest {
    /// Who a direct destination is for, as `to` holds it: the session ids,
    /// or the person's member id.
    fn addressee(self) -> String {
        match self {
            Dest::Sessions { ids, .. } => ids.join(","),
            Dest::Person(member) => member,
            Dest::Open { to } | Dest::Channel { to, .. } => to.unwrap_or_default(),
        }
    }
}

/// Refuse a channel no live peer would read. A signal written to a group
/// nobody is *currently* listening on sits unread in `@<name>/` until
/// cleanup sweeps it, while the sender sees "signal written" and assumes
/// delivery. `_groups.yaml` membership is intersected with
/// `PeerSensor::live_session_ids` so a peer that joined and died does not
/// let the validation pass on a phantom member.
fn check_channel(me: &Session, name: &str) -> Result<(), SendError> {
    let r = groups_for(me);
    let self_id = me.session_resolved.then(|| me.session_id.clone());
    let members = r.members(name);
    let live_ids = live_session_ids();
    // A member is live if it's a running claude session OR its heartbeat is
    // fresh. The heartbeat arm covers human members (ADR-170): attend-chat
    // heartbeats the username while open, and a human never appears in the
    // claude-process scan — so without it, a human-only group would reject
    // agent sends with a phantom "no live peers".
    let live_peer_count = members.as_ref().map_or(0, |ids| {
        ids.iter()
            .filter(|sid| attend_presence::alive(sid, &live_ids) && self_id.as_ref().map(|s| s != *sid).unwrap_or(true))
            .count()
    });
    if live_peer_count > 0 {
        return Ok(());
    }
    let channels = r.all_groups().into_iter().map(|(name, members, pinned)| ChannelCount { name, members, pinned }).collect();
    Err(match members {
        None => SendError::NoChannel { name: name.to_string(), channels },
        Some(ids) => SendError::NoLivePeers {
            name: name.to_string(),
            only_self: self_id.as_ref().is_some_and(|sid| ids.iter().any(|m| m == sid)),
            channels,
        },
    })
}

/// A live session other than this one, as `to` can name it.
struct LiveSession {
    id: String,
    cwd: String,
    nickname: String,
    name: String,
}

fn live_sessions() -> Vec<LiveSession> {
    let registry = attend_instances::Registry::new();
    list_peers()
        .into_iter()
        .map(|(id, cwd, _, _, _)| {
            let (nickname, name) = crate::verbs::peers::names(&cwd, registry.lookup(&cwd, &id).as_deref());
            LiveSession { id, cwd, nickname, name }
        })
        .collect()
}

/// The people attend knows by member id (ADR-170): the person at this
/// machine, anyone with a person tray, and every person in a channel.
fn known_people() -> Vec<String> {
    let base = signals_base();
    let mut people = vec![agent_identity::sanitize_id_component(&agent_identity::signal::user_name())];
    let trays = std::fs::read_dir(base.join(attend_groups::PEOPLE_DIR)).into_iter().flatten().flatten();
    people.extend(trays.filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()));
    for entry in attend_groups::load_groups(&base).into_values() {
        people.extend(entry.members.into_iter().filter(|m| !attend_groups::is_agent_member(m)));
    }
    people.sort();
    people.dedup();
    people
}

/// Resolve `to`: a project path to every live session there, else a
/// session's name or id, or a person's member id. A name that matches more
/// than one, or none, is refused.
/// Whether recipient `to` is written as a project path, not a name.
fn is_project_path(to: &str) -> bool {
    to.contains(['/', '\\']) || to.starts_with(['.', '~'])
}

fn resolve_recipient(me: &Session, to: &str) -> Result<Dest, SendError> {
    let to = to.trim();
    if is_project_path(to) {
        return resolve_project(me, to);
    }
    let sessions = live_sessions();
    // A session id is canonical: it names that session, whatever else
    // answers to the same text.
    if let Some(s) = sessions.iter().find(|s| s.id == to) {
        return Ok(Dest::Sessions { cwd: s.cwd.clone(), ids: vec![s.id.clone()] });
    }
    let matched: Vec<&LiveSession> = sessions
        .iter()
        .filter(|s| s.id == to || s.name.eq_ignore_ascii_case(to) || s.nickname.eq_ignore_ascii_case(to))
        .collect();
    let person = known_people().into_iter().find(|p| p.eq_ignore_ascii_case(to));
    match (matched.as_slice(), person) {
        ([one], None) => Ok(Dest::Sessions { cwd: one.cwd.clone(), ids: vec![one.id.clone()] }),
        ([], Some(person)) => Ok(Dest::Person(person)),
        ([], None) => Err(SendError::NoRecipient { name: to.to_string(), sessions: sessions.iter().map(|s| s.name.clone()).collect() }),
        (many, person) => {
            let mut candidates: Vec<String> = many.iter().map(|s| format!("{} (session {})", s.name, s.id)).collect();
            candidates.extend(person.map(|p| format!("{p} (person)")));
            Err(SendError::AmbiguousRecipient { name: to.to_string(), candidates })
        }
    }
}

/// A project path: every live session working there. A path no live peer
/// works at is refused, with the live ones listed.
fn resolve_project(me: &Session, path: &str) -> Result<Dest, SendError> {
    let resolved = std::fs::canonicalize(path).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| path.to_string());
    let peers = list_peers();
    let ids: Vec<String> = peers.iter().filter(|(_, cwd, ..)| *cwd == resolved).map(|(id, ..)| id.clone()).collect();
    if !ids.is_empty() {
        return Ok(Dest::Sessions { cwd: resolved, ids });
    }
    // The peer list leaves this session out, so its own project reads as
    // empty when no other session works there. Say that, rather than "no
    // active peer" at a path the sender is using.
    let cwd = &me.origin_path;
    let own = std::fs::canonicalize(cwd).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| cwd.clone());
    if resolved == own || resolved == *cwd {
        return Err(SendError::OwnProject { path: resolved });
    }
    let peer_paths: Vec<&str> = peers.iter().map(|(_, cwd, ..)| cwd.as_str()).collect();
    Err(SendError::NoPeerAt {
        suggestion: find_closest_peer(&resolved, &peer_paths).map(str::to_string),
        peers: peers.iter().map(|(_, cwd, project, _, _)| PeerAt { cwd: cwd.clone(), project: project.clone() }).collect(),
        path: resolved,
    })
}

/// Session `me` as it sends, and as it adds a member (attend-membership).
/// The wire identity rides its origin path (issue #378), the session
/// record's, so a stray shell `cd` can't relabel what it sends. An
/// unresolved session sends as the person at the machine.
pub(crate) fn actor(me: &Session) -> attend_membership::Actor {
    let self_id = me.session_resolved.then(|| me.session_id.clone());
    let (sender_id, source_kind) = agent_identity::signal::identify_sender(self_id.clone());
    let member_id = self_id.unwrap_or_else(|| agent_identity::sanitize_id_component(&agent_identity::signal::user_name()));
    attend_membership::Actor {
        name: crate::util::you_are(me).unwrap_or_else(|| member_id.clone()),
        member_id,
        kind: if me.session_resolved { "agent" } else { "human" },
        from: format!("{source_kind}:{sender_id}"),
        sender_id,
        project: agent_identity::signal::project_label(&me.origin_path),
        cwd: me.origin_path.clone(),
    }
}

/// The one live session `to` names, by name or id, as its id, name and
/// project root. A person, a project path or a name that matches more than
/// one session is refused, as is one that matches none. A project path is
/// refused even where one session works there: it names whoever does.
pub(crate) fn resolve_session(me: &Session, to: &str) -> Result<(String, String, String), SendError> {
    let to = to.trim();
    if is_project_path(to) {
        return Err(SendError::ProjectPath { path: to.to_string() });
    }
    match resolve_recipient(me, to)? {
        Dest::Sessions { cwd, ids } if ids.len() == 1 => {
            let name = live_sessions().into_iter().find(|s| s.id == ids[0]).map_or_else(|| ids[0].clone(), |s| s.name);
            Ok((ids[0].clone(), name, cwd))
        }
        Dest::Sessions { ids, .. } => Err(SendError::AmbiguousRecipient { name: to.to_string(), candidates: ids }),
        _ => Err(SendError::NoRecipient {
            name: to.to_string(),
            sessions: live_sessions().into_iter().map(|s| s.name).collect(),
        }),
    }
}

/// Write `req.message` to `dest` as session `me`, threaded under `req.re`.
fn write(me: &Session, dest: &Dest, req: &SendRequest) -> Result<Sent, SendError> {
    let base = signals_base();
    let (dir, scope, to) = match dest {
        Dest::Open { to } => (base.join(attend_groups::BROADCAST_DIR), "#open", to.clone().unwrap_or_else(|| "*".to_string())),
        Dest::Channel { name, to } => (groups_for(me).group_dir(name), "channel", to.clone().unwrap_or_else(|| format!("#{name}"))),
        Dest::Sessions { cwd, ids } => (base.join(encode_project(cwd)), "directed", ids.join(",")),
        Dest::Person(member) => (attend_groups::person_dir(&base, member), "directed", member.clone()),
    };
    let actor = actor(me);
    // The envelope (ADR-401): the sender's kind and canonical id, resolved
    // here, never supplied by the caller, and who the message is for.
    let envelope = agent_identity::Envelope {
        from_kind: Some(actor.kind),
        from_id: Some(&actor.member_id),
        on_behalf_of: None,
        to: Some(&to),
    };

    // One message is one signal, whatever its length (ADR-412). The
    // filename stem doubles as the signal id `re:<id>` replies reference.
    // `signal_filename` normalizes the sender id (external senders are
    // `$USER@<terminal>`; the raw `@`/`.` would fail `is_valid_signal_id`
    // and break `attend message reply` auto-threading, issue #368) and makes the
    // name collision-proof. The `from` field above keeps the un-normalized
    // identity.
    let filename = agent_identity::signal_filename(&actor.sender_id);
    // Wire format (ADR-120, ADR-401): `from|project|cwd|<tags>|message`.
    // attend-chat writes through the same `agent_identity::signal` functions.
    let content =
        agent_identity::signal::format_tagged(&actor.from, &actor.project, &actor.cwd, &envelope, req.re.as_deref(), &req.message);
    agent_identity::signal::write_signal_file(&dir, &filename, &content)
        .map_err(|e| SendError::NotWritten { error: format!("{}: {e}", dir.display()) })?;
    let id = filename.trim_end_matches(".signal").to_string();
    Ok(Sent { id, scope, to, re: req.re.clone(), dirs: 1, failed: Vec::new() })
}

/// How a reply was threaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Threading {
    /// Under the parent message's id.
    Threaded,
    /// The parent's recorded id cannot be threaded, so the reply went out
    /// without one. It still landed: threading is cosmetic, `parse_signal`
    /// drops the id and no peer renders it.
    Unthreaded,
}

/// A reply written to the bus, and how it threads.
#[derive(Debug, Clone, Serialize)]
pub struct Replied {
    #[serde(flatten)]
    pub sent: Sent,
    pub threading: Threading,
}

/// How a reply threads, given this session's newest peer message.
#[cfg(feature = "sensor-peers")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReplyTarget {
    /// No peer message has reached this session: nothing to reply to.
    NoInbound,
    /// One has, but its id can't be threaded. A standing guard, not a
    /// transition shim: whatever the source of a bad id (a stale pre-#368
    /// record from an external `$USER@<terminal>` sender, a hand-edited state
    /// file, a writer that skips `signal_filename`), the caller did nothing
    /// wrong and can't fix it, so the reply degrades to unthreaded rather
    /// than failing on `send`'s `re` validation.
    Unthreaded,
    /// A threadable id.
    Threaded(String),
}

#[cfg(feature = "sensor-peers")]
fn classify_reply_target(last_inbound: Option<String>) -> ReplyTarget {
    match last_inbound {
        None => ReplyTarget::NoInbound,
        Some(id) if is_valid_signal_id(&id) => ReplyTarget::Threaded(id),
        Some(_) => ReplyTarget::Unthreaded,
    }
}

/// Send `req` as a reply. An explicit `re` threads there; otherwise the reply
/// threads under the newest peer message a delivering conduit (the peers
/// sensor or the drain) surfaced to this session, and fails with
/// [`SendError::NoInbound`] when none has.
///
/// A reply goes to its parent's sender as a direct message, a reply to an
/// `#open` post included (ADR-401, `to`; ADR-406 item 10). A parent said in
/// a named channel is answered in that channel, addressed to its sender, and
/// never outside it: when this session has left the channel, or the channel
/// is gone, the reply is refused. A sender session that has exited is
/// refused by name; a person is always written to. A parent whose room or
/// sender cannot be told is refused, never guessed to be `#open`. A `to` or
/// `channel` in `req` overrides all of this.
#[cfg(feature = "sensor-peers")]
pub fn reply(req: &SendRequest) -> Result<Replied, SendError> {
    // An empty message is refused before anything is looked up, as `send`
    // refuses it, whatever this session has received.
    if req.message.is_empty() {
        return Err(SendError::EmptyMessage);
    }
    let me = current_session();
    let last = sensor_peers::last_inbound::read(&me.session_id);
    let (re, threading, recorded) = match &req.re {
        Some(id) => (Some(resolve_re(&me, id)?), Threading::Threaded, None),
        None => match classify_reply_target(last.as_ref().map(|l| l.id.clone())) {
            ReplyTarget::NoInbound => return Err(SendError::NoInbound),
            ReplyTarget::Unthreaded => (None, Threading::Unthreaded, last.as_ref()),
            ReplyTarget::Threaded(id) => (Some(id), Threading::Threaded, last.as_ref()),
        },
    };
    let req = SendRequest { re, ..req.clone() };
    check(&req)?;
    let dest = if req.to.is_some() || req.channel.is_some() {
        resolve(&me, &req)?
    } else {
        parent_dest(&me, req.re.as_deref(), recorded)?
    };
    let sent = write(&me, &dest, &req)?;
    Ok(Replied { sent, threading })
}

/// The full id `re` names: a full signal id as given, or the short `~id`
/// handle a delivery line shows (ADR-406 item 5), with or without its `~`,
/// looked up in this session's receive set. The attend mod maps the handles
/// of what it delivered itself; a handle the Stop-hook drain printed reaches
/// attend as written and is resolved here.
#[cfg(feature = "sensor-peers")]
fn resolve_re(me: &Session, re: &str) -> Result<String, SendError> {
    let re = re.strip_prefix('~').unwrap_or(re);
    if !crate::verbs::inbox::is_short_id(re) {
        return Ok(re.to_string());
    }
    crate::verbs::inbox::resolve_short_id(me, re).ok_or_else(|| SendError::UnknownHandle { handle: re.to_string() })
}

/// Who sent a parent message: its canonical id, its kind, and where it
/// worked when it is known.
#[cfg(feature = "sensor-peers")]
struct Sender {
    id: String,
    kind: Option<agent_identity::signal::Kind>,
    cwd: Option<String>,
}

#[cfg(feature = "sensor-peers")]
impl Sender {
    fn of(sig: &agent_identity::ParsedSignal) -> Self {
        Sender { id: sig.sender_id(), kind: sig.sender_kind(), cwd: Some(sig.cwd.to_string()).filter(|c| !c.is_empty()) }
    }

    /// From a wire `from` alone, as the last-inbound record keeps it.
    fn of_from(from: &str) -> Self {
        let kind = if from.starts_with("claude:") {
            Some(agent_identity::signal::Kind::Agent)
        } else if from.starts_with("external:") {
            Some(agent_identity::signal::Kind::Human)
        } else {
            None
        };
        Sender { id: agent_identity::signal::derived_sender_id(from), kind, cwd: None }
    }
}

/// Where the reply to parent `id` goes. The parent's file in this session's
/// receive set answers first; a parent not there but in another channel's
/// directory was said in a channel this session left. A parent whose file
/// is gone, or whose id cannot be looked up, goes by `recorded`: the room and
/// sender the conduit noted when it surfaced the message.
#[cfg(feature = "sensor-peers")]
fn parent_dest(
    me: &Session,
    id: Option<&str>,
    recorded: Option<&sensor_peers::last_inbound::LastInbound>,
) -> Result<Dest, SendError> {
    use sensor_peers::last_inbound::Said;
    let r = groups_for(me);
    let channel_dest = |channel: &str, sender: Option<&Sender>| {
        if r.members(channel).is_none() {
            Err(SendError::ParentChannelGone { channel: channel.to_string() })
        } else if !r.joined_group_names().iter().any(|n| n == channel) {
            Err(SendError::ParentChannelLeft { channel: channel.to_string() })
        } else {
            check_channel(me, channel)?;
            Ok(Dest::Channel { name: channel.to_string(), to: sender.map(|s| s.id.clone()) })
        }
    };
    let file = id.map(|id| format!("{id}.signal"));
    let in_receive_set = file.as_ref().and_then(|file| {
        r.receive_dirs(&me.origin_path).into_iter().find(|d| d.path.join(file).is_file()).map(|d| (d, file))
    });
    // The room the parent was said in, and its sender when it can be told.
    let (room, sender): (Option<Said>, Option<Sender>) = match in_receive_set {
        Some((dir, file)) => {
            let raw = agent_identity::signal::read_signal_file(&dir.path.join(file)).ok();
            let sig = raw.as_deref().and_then(|c| agent_identity::parse_signal(agent_identity::wire_line(c)));
            let room = match &dir.room {
                attend_groups::Room::Channel(name) => Said::Channel(name.clone()),
                attend_groups::Room::Open => Said::Open,
                attend_groups::Room::Project => Said::Project(None),
            };
            (Some(room), sig.as_ref().map(Sender::of))
        }
        None => {
            if let Some(channel) = file.as_ref().and_then(|f| r.channel_holding(f)) {
                let sender = recorded.and_then(|l| l.from.as_deref()).map(Sender::of_from);
                return channel_dest(&channel, sender.as_ref());
            }
            let Some(rec) = recorded else { return Err(SendError::ParentRoomUnknown) };
            // A record written before the sender was kept names a Claude
            // sender only by its project: answer there, as attend did then.
            if let (None, Some(Said::Project(Some(cwd)))) = (&rec.from, &rec.said) {
                return resolve_project(me, cwd);
            }
            (rec.said.clone(), rec.from.as_deref().map(Sender::of_from))
        }
    };
    match (room, sender) {
        (Some(Said::Channel(channel)), sender) => channel_dest(&channel, sender.as_ref()),
        (Some(_), Some(sender)) => sender_dest(sender),
        _ => Err(SendError::ParentRoomUnknown),
    }
}

/// A direct message to `sender`: a person's tray, always; a session's
/// project tray while it is live, else refused by its name. A session id
/// that `/clear` replaced answers through the live session that took its
/// slot.
#[cfg(feature = "sensor-peers")]
fn sender_dest(sender: Sender) -> Result<Dest, SendError> {
    use agent_identity::signal::Kind;
    match sender.kind {
        Some(Kind::Human) if is_member_id(&sender.id) => Ok(Dest::Person(sender.id)),
        Some(Kind::Agent) => {
            let registry = attend_instances::Registry::new();
            let successor = sender.cwd.as_deref().and_then(|cwd| registry.successor(cwd, &sender.id));
            let sessions = live_sessions();
            let live = [Some(&sender.id), successor.as_ref()]
                .into_iter()
                .flatten()
                .find_map(|id| sessions.iter().find(|s| &s.id == id));
            if let Some(live) = live {
                return Ok(Dest::Sessions { cwd: live.cwd.clone(), ids: vec![live.id.clone()] });
            }
            let name = match &sender.cwd {
                Some(cwd) => crate::verbs::peers::names(cwd, registry.lookup(cwd, &sender.id).as_deref()).1,
                None => format!("session {}", sender.id.chars().take(8).collect::<String>()),
            };
            Err(SendError::SenderExited { session: sender.id, name })
        }
        _ => Err(SendError::ParentRoomUnknown),
    }
}

/// Whether `id` can name a person tray: a non-empty sanitized member id.
#[cfg(feature = "sensor-peers")]
fn is_member_id(id: &str) -> bool {
    !id.is_empty() && id.trim_matches('-').len() == id.len() && agent_identity::sanitize_id_component(id) == id
}

#[cfg(feature = "sensor-peers")]
fn list_peers() -> Vec<(String, String, String, String, f64)> {
    crate::sensors::PeerSensor::new().list_peers()
}

#[cfg(not(feature = "sensor-peers"))]
fn list_peers() -> Vec<(String, String, String, String, f64)> {
    Vec::new()
}

#[cfg(feature = "sensor-peers")]
fn live_session_ids() -> std::collections::HashSet<String> {
    crate::sensors::PeerSensor::new().live_session_ids()
}

#[cfg(not(feature = "sensor-peers"))]
fn live_session_ids() -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

/// Find the closest matching peer path by comparing path suffixes.
fn find_closest_peer<'a>(target: &str, peers: &[&'a str]) -> Option<&'a str> {
    // Try matching the last N segments of the target against peer paths
    let target_parts: Vec<&str> = target.rsplit('/').collect();
    let mut best: Option<(&str, usize)> = None;

    for peer in peers {
        let peer_parts: Vec<&str> = peer.rsplit('/').collect();
        let common = target_parts
            .iter()
            .zip(peer_parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if common > 0 && (best.is_none() || common > best.unwrap().1) {
            best = Some((peer, common));
        }
    }

    best.map(|(p, _)| p)
}

#[cfg(all(test, feature = "sensor-peers"))]
mod tests {
    use super::*;

    #[test]
    fn no_inbound_is_a_hard_error_case() {
        assert_eq!(classify_reply_target(None), ReplyTarget::NoInbound);
    }

    #[test]
    fn valid_id_threads() {
        assert_eq!(
            classify_reply_target(Some("claude-2f2632d7-1712345".to_string())),
            ReplyTarget::Threaded("claude-2f2632d7-1712345".to_string())
        );
    }

    #[test]
    fn external_at_bearing_id_degrades_to_unthreaded() {
        // The regression from issue #368: a last-inbound id from an
        // external `$USER@<terminal>` sender must NOT reach send's `re`
        // validation — it degrades to an unthreaded send instead.
        assert_eq!(classify_reply_target(Some("aaron@kitty-1712345".to_string())), ReplyTarget::Unthreaded);
        assert_eq!(classify_reply_target(Some("aaron@iterm.app-1712345".to_string())), ReplyTarget::Unthreaded);
    }

    #[test]
    fn closest_peer_matches_the_longest_shared_suffix() {
        let peers = ["/a/b/proj", "/x/proj", "/q/other"];
        assert_eq!(find_closest_peer("/z/b/proj", &peers), Some("/a/b/proj"));
        assert_eq!(find_closest_peer("/z/none", &peers), None);
    }
}
