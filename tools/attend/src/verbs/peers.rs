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

#[derive(Debug, Clone, Serialize)]
pub struct Peers {
    #[serde(rename = "self")]
    pub me: Agent,
    pub channels: Vec<JoinedChannel>,
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

    let channels = groups_for(&session)
        .my_groups()
        .into_iter()
        .map(|(name, pinned)| JoinedChannel { name, pinned })
        .collect();

    let peers = list_peers()
        .into_iter()
        .map(|(sid, peer_cwd, _project, status, ctx)| {
            let instance = registry.lookup(&peer_cwd, &sid);
            let here = peer_cwd == cwd;
            agent(Some(sid), &peer_cwd, instance, status, Some(ctx), here)
        })
        .collect();

    Peers { me, channels, peers }
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
