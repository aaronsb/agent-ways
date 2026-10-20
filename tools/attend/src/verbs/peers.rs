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
    /// Every live member by name ([`member_names`]), this session included.
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
            // Read-only, so no cleanup_stale: a member whose session is gone
            // is left out here and pruned by the next `attend channel`.
            let live: Vec<String> =
                r.members(&name).unwrap_or_default().into_iter().filter(|m| attend_groups::member_alive(m)).collect();
            let members = member_names(&live, &me, &peers);
            PeerChannel { name, pinned, members }
        })
        .collect();

    Peers { me, channels, peers }
}

/// A channel's members by name, so a member can see who else has joined
/// without waiting for them to post (ADR-404 D7, the text view of
/// membership): this session as `Name (you)`, a live agent session as
/// `Name (project)`, the way `attend peer list` lists it, a person in
/// attend-chat as `username (attend-chat)`, and an agent session that is
/// not live by the first eight characters of its id.
pub fn member_names(members: &[String], me: &Agent, peers: &[Agent]) -> Vec<String> {
    let scopes = scopes(&roster(me, peers));
    members
        .iter()
        .map(|id| {
            if me.session_id.as_deref() == Some(id.as_str()) {
                return format!("{} (you)", me.name);
            }
            if let Some(i) = peers.iter().position(|p| p.session_id.as_deref() == Some(id.as_str())) {
                // The roster puts this session first, so peer i is scope i + 1.
                return format!("{} ({})", peers[i].name, scopes[i + 1]);
            }
            if attend_groups::is_agent_member(id) {
                return format!("session {}", id.chars().take(8).collect::<String>());
            }
            format!("{id} (attend-chat)")
        })
        .collect()
}

/// This session followed by its peers, in listing order: the set
/// [`scopes`] disambiguates over.
pub fn roster<'a>(me: &'a Agent, peers: &'a [Agent]) -> Vec<&'a Agent> {
    std::iter::once(me).chain(peers).collect()
}

/// The scope each agent's label shows in parentheses after its name, in the
/// order given. It is the cwd basename. Nicknames are a hash of the cwd over
/// a fixed pool, so two live sessions can share a name and a basename (two
/// checkouts both called `app`). For those rows only, the scope grows to the
/// parent segment (`work/app`), and where that still collides, to a
/// session-id prefix as well (`work/app 1a2b3c4d`). Display only: the name,
/// the registry and the wire id are unchanged.
pub fn scopes(agents: &[&Agent]) -> Vec<String> {
    let mut out: Vec<String> = agents.iter().map(|a| agent_identity::cwd_basename(&a.cwd)).collect();
    for i in clashes(agents, &out) {
        out[i] = parent_scope(&agents[i].cwd);
    }
    for i in clashes(agents, &out) {
        if let Some(sid) = agents[i].session_id.as_deref() {
            out[i] = format!("{} {}", out[i], sid.chars().take(8).collect::<String>());
        }
    }
    out
}

/// The indices whose name and scope another agent in the set shares.
fn clashes(agents: &[&Agent], scopes: &[String]) -> Vec<usize> {
    (0..agents.len())
        .filter(|&i| (0..agents.len()).any(|j| j != i && agents[j].name == agents[i].name && scopes[j] == scopes[i]))
        .collect()
}

/// The last two segments of `cwd` (`work/app`), or the basename alone when
/// there is no parent segment.
fn parent_scope(cwd: &str) -> String {
    let mut segs = cwd.split('/').filter(|s| !s.is_empty()).rev();
    match (segs.next(), segs.next()) {
        (Some(base), Some(parent)) => format!("{parent}/{base}"),
        _ => agent_identity::cwd_basename(cwd),
    }
}

fn agent(session_id: Option<String>, cwd: &str, instance: Option<String>, status: String, context_pct: Option<f64>, here: bool) -> Agent {
    let (nickname, name) = names(cwd, instance.as_deref());
    Agent { session_id, cwd: cwd.to_string(), name, nickname, instance, status, context_pct, here }
}

/// A session's `(nickname, name)` at `cwd`: the persona, and the persona
/// with its instance suffix (ADR-129), as peers see it.
pub(crate) fn names(cwd: &str, instance: Option<&str>) -> (String, String) {
    let nickname = agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string();
    let name = match instance {
        Some(inst) => format!("{nickname}-{inst}"),
        None => nickname.clone(),
    };
    (nickname, name)
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
            ["Chaucer-beta (you)", "McKay (attend-collaborator)", "aaron (attend-chat)", "session 33333333"]
        );
    }

    fn scopes_of(agents: &[Agent]) -> Vec<String> {
        scopes(&agents.iter().collect::<Vec<_>>())
    }

    #[test]
    fn distinct_names_keep_the_basename() {
        let agents = [named("aaaaaaaa-1", "/src/one/app", "Clelia"), named("bbbbbbbb-2", "/src/two/app", "McKay")];
        assert_eq!(scopes_of(&agents), ["app", "app"]);
    }

    #[test]
    fn same_name_different_basenames_keep_the_basename() {
        let agents = [
            named("aaaaaaaa-1", "/home/a/praecipio/ai-workstation-ops", "Clelia"),
            named("bbbbbbbb-2", "/home/a/over-the-hill", "Clelia"),
        ];
        assert_eq!(scopes_of(&agents), ["ai-workstation-ops", "over-the-hill"]);
    }

    #[test]
    fn same_name_and_basename_add_the_parent_segment() {
        let agents = [
            named("aaaaaaaa-1", "/home/a/praecipio/app", "Clelia"),
            named("bbbbbbbb-2", "/home/a/personal/app", "Clelia"),
            named("cccccccc-3", "/home/a/other", "McKay"),
        ];
        assert_eq!(scopes_of(&agents), ["praecipio/app", "personal/app", "other"]);
    }

    #[test]
    fn three_way_collision_falls_back_to_a_session_prefix_where_needed() {
        let agents = [
            named("aaaaaaaa-1111", "/x/work/app", "Clelia"),
            named("bbbbbbbb-2222", "/y/work/app", "Clelia"),
            named("cccccccc-3333", "/z/home/app", "Clelia"),
        ];
        assert_eq!(scopes_of(&agents), ["work/app aaaaaaaa", "work/app bbbbbbbb", "home/app"]);
    }

    #[test]
    fn members_carry_the_disambiguated_scope() {
        let me = named("11111111-1111-1111-1111-111111111111", "/a/praecipio/app", "Clelia");
        let peers = [named("22222222-2222-2222-2222-222222222222", "/a/personal/app", "Clelia")];
        let members = ["22222222-2222-2222-2222-222222222222".to_string()];
        assert_eq!(member_names(&members, &me, &peers), ["Clelia (personal/app)"]);
    }
}
