//! `send` and `reply`: write one signal to the bus.

use crate::util::{encode_project, get_groups, own_session_id, signals_base};
use agent_identity::is_valid_signal_id;
use serde::Serialize;
use std::path::PathBuf;

/// A message to send. With no routing field it reaches every peer.
#[derive(Debug, Clone, Default)]
pub struct SendRequest {
    pub message: String,
    /// A project path: only the live peers working there receive it.
    pub to: Option<String>,
    /// A channel name: only its members receive it.
    pub channel: Option<String>,
    /// A signal id to thread under. `reply` fills it from the newest peer
    /// message when it is empty.
    pub re: Option<String>,
}

/// A signal written to the bus.
#[derive(Debug, Clone, Serialize)]
pub struct Sent {
    /// The signal's id, which a reply to it threads under.
    pub id: String,
    /// `#open` for a broadcast, `channel` or `directed`.
    pub scope: &'static str,
    /// The id this one threads under, if any.
    pub re: Option<String>,
    /// How many trays it was addressed to.
    pub dirs: usize,
    /// Trays the write failed in. The send still reports success, as the
    /// other trays may have it.
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
    /// No live peer works at the `to` path.
    NoPeerAt { path: String, peers: Vec<PeerAt>, suggestion: Option<String> },
    /// No channel has the `channel` name.
    NoChannel { name: String, channels: Vec<ChannelCount> },
    /// The channel exists but no live peer is in it; `only_self` when the
    /// sender is its one listener.
    NoLivePeers { name: String, only_self: bool, channels: Vec<ChannelCount> },
    /// `reply` found no peer message to thread under.
    NoInbound,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidReId => write!(f, "re must be a signal id matching [A-Za-z0-9_-]+"),
            Self::EmptyMessage => write!(f, "the message is empty"),
            Self::NoPeerAt { path, .. } => write!(f, "no active peer at {path}"),
            Self::NoChannel { name, .. } => write!(f, "no channel named '{name}'"),
            Self::NoLivePeers { name, only_self: true, .. } => {
                write!(f, "no live peers in channel '{name}' (you are the only listener)")
            }
            Self::NoLivePeers { name, .. } => write!(f, "no live peers in channel '{name}'"),
            Self::NoInbound => write!(f, "no prior inbound signal to thread against; to start a new topic, send instead"),
        }
    }
}

impl std::error::Error for SendError {}

/// Write `req.message` to the bus as this session. The sender is the Claude
/// Code session found by process ancestry, else the external `$USER@<terminal>`
/// identity a plain terminal has.
pub fn send(req: &SendRequest) -> Result<Sent, SendError> {
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

    let base = signals_base();
    // Wire identity rides this cwd (issue #378): the session record's
    // origin path, so a stray shell `cd` can't relabel our sends.
    let cwd = crate::util::own_origin_cwd();

    // Validate `to` against active peers.
    let resolved_to = req.to.as_ref().map(|path| {
        std::fs::canonicalize(path).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| path.clone())
    });
    if let Some(resolved) = &resolved_to {
        let peers = list_peers();
        let peer_paths: Vec<&str> = peers.iter().map(|(_, cwd, _, _, _)| cwd.as_str()).collect();
        if !peer_paths.contains(&resolved.as_str()) {
            return Err(SendError::NoPeerAt {
                path: resolved.clone(),
                suggestion: find_closest_peer(resolved, &peer_paths).map(str::to_string),
                peers: peers.iter().map(|(_, cwd, project, _, _)| PeerAt { cwd: cwd.clone(), project: project.clone() }).collect(),
            });
        }
    }

    let r = get_groups();

    // Validate `channel` against live `_groups.yaml` membership. A signal
    // written to a group nobody is *currently* listening on sits unread in
    // `@<name>/` until cleanup sweeps it; the sender only sees "signal
    // written" and assumes delivery. Mirror `to`'s liveness discipline:
    // `_groups.yaml` membership is intersected with
    // `PeerSensor::live_session_ids` so a peer that joined-and-died does not
    // let the validation pass on a phantom member.
    if let Some(name) = &req.channel {
        let members = r.members(name);
        let self_id = own_session_id();
        let live_ids = live_session_ids();
        // A member is live if it's a running claude session OR its
        // heartbeat is fresh. The heartbeat arm covers human members
        // (ADR-170): attend-chat heartbeats the username while open,
        // and a human never appears in the claude-process scan — so
        // without it, a human-only group would reject agent sends
        // with a phantom "no live peers".
        let live_peer_count = members.as_ref().map_or(0, |ids| {
            ids.iter()
                .filter(|sid| attend_presence::alive(sid, &live_ids) && self_id.as_ref().map(|s| s != *sid).unwrap_or(true))
                .count()
        });
        if live_peer_count == 0 {
            let channels = r
                .all_groups()
                .into_iter()
                .map(|(name, members, pinned)| ChannelCount { name, members, pinned })
                .collect();
            return Err(match members {
                None => SendError::NoChannel { name: name.clone(), channels },
                Some(ids) => SendError::NoLivePeers {
                    name: name.clone(),
                    only_self: self_id.as_ref().is_some_and(|sid| ids.iter().any(|m| m == sid)),
                    channels,
                },
            });
        }
    }

    // Default is broadcast — simplest possible routing: every send reaches
    // every peer. A channel or a project path narrows it.
    let (dest_dirs, scope) = match (&req.channel, &resolved_to) {
        (Some(channel), _) => (vec![r.group_dir(channel)], "channel"),
        (None, Some(resolved)) => (vec![base.join(encode_project(resolved))], "directed"),
        (None, None) => (vec![base.join(attend_groups::BROADCAST_DIR)], "#open"),
    };

    let (sender_id, source_kind) = agent_identity::signal::identify_sender(own_session_id());
    let from = format!("{}:{}", source_kind, sender_id);
    // Build the filename stem, which doubles as the signal id `re:<id>`
    // replies reference. `signal_filename` normalizes the sender id
    // (external senders are `$USER@<terminal>`; the raw `@`/`.` would fail
    // `is_valid_signal_id` and break `attend reply` auto-threading —
    // issue #368) and makes the name collision-proof. The `from` field
    // above keeps the un-normalized identity.
    let filename = agent_identity::signal_filename(&sender_id);
    // Wire format (ADR-120): `from|project|cwd|message`, or with `re:<id>`
    // before the message when threading. attend-chat writes through the
    // same `agent_identity::signal` functions.
    let project = agent_identity::signal::project_label(&cwd);
    let content = agent_identity::signal::format_signal(&from, &project, &cwd, req.re.as_deref(), &req.message);

    let failed = dest_dirs
        .iter()
        .filter_map(|dir| {
            agent_identity::signal::write_signal_file(dir, &filename, &content)
                .err()
                .map(|e| WriteFailure { dir: dir.clone(), error: e.to_string() })
        })
        .collect();

    Ok(Sent {
        id: filename.trim_end_matches(".signal").to_string(),
        scope,
        re: req.re.clone(),
        dirs: dest_dirs.len(),
        failed,
    })
}

/// How a reply threads, given this session's newest peer message.
#[cfg(feature = "sensor-peers")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyTarget {
    /// No peer message has reached this session: nothing to reply to.
    NoInbound,
    /// One has, but its id can't be threaded; the reply goes out unthreaded
    /// rather than failing on an id the caller can't fix.
    Unthreaded,
    /// A threadable id.
    Threaded(String),
}

/// The peer message a reply from this session threads under: the newest one
/// a delivering conduit (the peers sensor or the drain) surfaced to it.
#[cfg(feature = "sensor-peers")]
pub fn reply_target() -> ReplyTarget {
    let session_id = own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));
    classify_reply_target(sensor_peers::last_inbound::read(&session_id))
}

#[cfg(feature = "sensor-peers")]
fn classify_reply_target(last_inbound: Option<String>) -> ReplyTarget {
    match last_inbound {
        None => ReplyTarget::NoInbound,
        Some(id) if is_valid_signal_id(&id) => ReplyTarget::Threaded(id),
        Some(_) => ReplyTarget::Unthreaded,
    }
}

/// Send `req` as a reply. An explicit `re` threads there; otherwise the
/// reply threads under [`reply_target`], and fails with
/// [`SendError::NoInbound`] when no peer message has arrived.
#[cfg(feature = "sensor-peers")]
pub fn reply(req: &SendRequest) -> Result<Sent, SendError> {
    if req.re.is_some() {
        return send(req);
    }
    let re = match reply_target() {
        ReplyTarget::NoInbound => return Err(SendError::NoInbound),
        ReplyTarget::Unthreaded => None,
        ReplyTarget::Threaded(id) => Some(id),
    };
    send(&SendRequest { re, ..req.clone() })
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
