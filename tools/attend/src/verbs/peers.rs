//! `peers`: this session and the live Claude Code sessions around it.

use crate::util::{current_session, groups_for};
use serde::Serialize;

/// A session on the bus.
#[derive(Debug, Clone, Serialize)]
pub struct Agent {
    /// The Claude Code session id; `None` for this process when no session
    /// owns it.
    pub session_id: Option<String>,
    /// The working directory its identity derives from.
    pub cwd: String,
    /// The persona name with its instance suffix (ADR-129), as peers see it.
    pub name: String,
    pub nickname: String,
    pub instance: Option<String>,
    /// What the peers sensor reads it doing; `working` for this session.
    pub status: String,
    /// Context window used, in percent. `None` for this session.
    pub context_pct: Option<f64>,
    /// Works in the same directory as this session.
    pub here: bool,
}

/// A channel this session is in.
#[derive(Debug, Clone, Serialize)]
pub struct JoinedChannel {
    pub name: String,
    pub pinned: bool,
}

/// A channel this session is in, with who else is in it.
#[derive(Debug, Clone, Serialize)]
pub struct PeerChannel {
    pub name: String,
    pub pinned: bool,
    /// Every member by name ([`member_names`]), this session included.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Peers {
    #[serde(rename = "self")]
    pub me: Agent,
    pub channels: Vec<PeerChannel>,
    pub peers: Vec<Agent>,
}

/// This session, the channels it is in, and every live peer session.
pub fn peers() -> Peers {
    let registry = attend_instances::Registry::new();
    // Self: identity from our origin path (issue #378) so the name matches
    // what peers see when they render a signal from us, including when the
    // shell's cwd has drifted. The instance suffix comes from the registry
    // `attend run` writes at startup.
    let session = current_session();
    let cwd = session.origin_path.clone();
    let self_sid = session.session_resolved.then(|| session.session_id.clone());
    let self_instance = self_sid.as_deref().and_then(|sid| registry.lookup(&cwd, sid));
    let me = agent(self_sid, &cwd, self_instance, "working".to_string(), None, true);

    let peers: Vec<Agent> = list_peers()
        .into_iter()
        .map(|(sid, peer_cwd, _project, status, ctx)| {
            let instance = registry.lookup(&peer_cwd, &sid);
            let here = peer_cwd == cwd;
            agent(Some(sid), &peer_cwd, instance, status, Some(ctx), here)
        })
        .collect();

    let r = groups_for(&session);
    let channels = r
        .my_groups()
        .into_iter()
        .map(|(name, pinned)| {
            let members = member_names(&r.members(&name).unwrap_or_default(), &me, &peers);
            PeerChannel { name, pinned, members }
        })
        .collect();

    Peers { me, channels, peers }
}

/// A channel's members by name, so a member can see who else has joined
/// without waiting for them to post (ADR-404 D7, the text view of
/// membership): this session as `Name (you)`, a live agent session as
/// `Name (project)`, the way `attend peers` lists it, a person in
/// attend-chat by their username, and an agent session that is not live
/// by the first eight characters of its id.
pub fn member_names(members: &[String], me: &Agent, peers: &[Agent]) -> Vec<String> {
    members
        .iter()
        .map(|id| {
            if me.session_id.as_deref() == Some(id.as_str()) {
                return format!("{} (you)", me.name);
            }
            if let Some(p) = peers.iter().find(|p| p.session_id.as_deref() == Some(id.as_str())) {
                return format!("{} ({})", p.name, basename(&p.cwd));
            }
            if attend_groups::is_agent_member(id) {
                return format!("session {}", id.chars().take(8).collect::<String>());
            }
            id.clone()
        })
        .collect()
}

fn basename(cwd: &str) -> &str {
    cwd.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(cwd)
}

fn agent(session_id: Option<String>, cwd: &str, instance: Option<String>, status: String, context_pct: Option<f64>, here: bool) -> Agent {
    let nickname = agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string();
    let name = match &instance {
        Some(inst) => format!("{nickname}-{inst}"),
        None => nickname.clone(),
    };
    Agent { session_id, cwd: cwd.to_string(), name, nickname, instance, status, context_pct, here }
}

#[cfg(feature = "sensor-peers")]
fn list_peers() -> Vec<(String, String, String, String, f64)> {
    crate::sensors::PeerSensor::new().list_peers()
}

#[cfg(not(feature = "sensor-peers"))]
fn list_peers() -> Vec<(String, String, String, String, f64)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(sid: &str, cwd: &str, name: &str) -> Agent {
        Agent {
            session_id: Some(sid.to_string()),
            cwd: cwd.to_string(),
            name: name.to_string(),
            nickname: name.to_string(),
            instance: None,
            status: "waiting".to_string(),
            context_pct: None,
            here: false,
        }
    }

    #[test]
    fn members_are_named_as_peers_lists_them() {
        let me = named("11111111-1111-1111-1111-111111111111", "/src/agent-ways", "Chaucer-beta");
        let peers = [named("22222222-2222-2222-2222-222222222222", "/src/attend-collaborator", "McKay")];
        let members = [
            "11111111-1111-1111-1111-111111111111".to_string(),
            "22222222-2222-2222-2222-222222222222".to_string(),
            "aaron".to_string(),
            "33333333-3333-3333-3333-333333333333".to_string(),
        ];
        assert_eq!(
            member_names(&members, &me, &peers),
            ["Chaucer-beta (you)", "McKay (attend-collaborator)", "aaron", "session 33333333"]
        );
    }
}
