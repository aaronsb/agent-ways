// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

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
    /// The persona name with its instance suffix, as peers see it: the
    /// bare nickname for the first live session in its directory, and the
    /// nickname with a short session id for a session with no row.
    ///
    /// References:
    /// - ADR-129, amendment of 2026-11-24: the first live session is bare.
    pub name: String,
    pub nickname: String,
    /// The suffix; `None` for a session that holds the bare name or has no
    /// registry row, which both show as the bare nickname.
    pub instance: Option<String>,
    /// What the peers sensor reads it doing; `working` for this session.
    /// A peer marked finished reads `finished`, with its age
    /// (`finished 12m ago`).
    pub status: String,
    /// When a peer was marked finished (`attend listen finish`), in seconds
    /// since the epoch. `None` for this session and for a peer that has not
    /// finished.
    pub finished: Option<u64>,
    /// Context window used, in percent, from the newest turn's API usage in
    /// the session's transcript ([`own_context_pct`] for this session, the
    /// peers sensor for a peer). `None`, `null` in JSON, when the transcript
    /// holds no such figure yet: unknown, not empty.
    pub context_pct: Option<f64>,
    /// The zone keepwarm reads the session's operator's night in, from the
    /// settings its last policy check read: `{"zone", "source"}`, or `None`
    /// where no check has run.
    ///
    /// References:
    /// - ADR-419: a supervisor tells when a worker's operator is likely back.
    pub timezone: Option<sensor_keepwarm::windows::ZoneView>,
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
    // the session's sensor host writes at startup.
    let session = current_session();
    let cwd = session.origin_path.clone();
    let self_sid = session.session_resolved.then(|| session.session_id.clone());
    let self_instance = self_sid.as_deref().and_then(|sid| registry.lookup(&cwd, sid));
    let listed: Vec<_> = list_peers()
        .into_iter()
        .map(|(sid, peer_cwd, _project, status, ctx)| {
            let instance = registry.lookup(&peer_cwd, &sid);
            (sid, peer_cwd, instance, status, ctx)
        })
        .collect();
    // The sessions with no row, by directory, so two of them in one
    // directory show short ids that differ.
    let rowless = |dir: &str| -> Vec<String> {
        let me = self_sid.iter().filter(|_| self_instance.is_none()).map(|sid| (sid.as_str(), cwd.as_str()));
        let others = listed.iter().filter(|(_, _, inst, ..)| inst.is_none()).map(|(sid, c, ..)| (sid.as_str(), c.as_str()));
        me.chain(others).filter(|(_, c)| attend_presence::session::same_dir(c, dir)).map(|(sid, _)| sid.to_string()).collect()
    };
    let me = self_agent(&ways_core::paths::claude_dir(), self_sid.clone(), &cwd, self_instance.clone(), &rowless(&cwd));
    let now = std::time::SystemTime::now();
    let peers: Vec<Agent> = listed
        .iter()
        .map(|(sid, peer_cwd, instance, status, ctx)| {
            let here = attend_presence::session::same_dir(peer_cwd, &cwd);
            let mut peer = agent(Some(sid.clone()), peer_cwd, instance.clone(), status.clone(), *ctx, here, &rowless(peer_cwd));
            if let Some(at) = attend_presence::enrollment::finished_since(sid) {
                let age = now.duration_since(at).map_or(0, |d| d.as_secs());
                peer.status = format!("finished {}", agent_fmt::when::ago(age));
                peer.finished = at.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
            }
            peer
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

/// This session's row: working, in its own directory, with the context fill
/// its transcript under `claude` records.
fn self_agent(
    claude: &claude_sessions::ClaudeDir,
    session_id: Option<String>,
    cwd: &str,
    instance: Option<String>,
    rowless: &[String],
) -> Agent {
    let context_pct = own_context_pct(claude, cwd, session_id.as_deref());
    agent(session_id, cwd, instance, "working".to_string(), context_pct, true, rowless)
}

/// This session's context fill, in percent of its own window, read from its
/// transcript under `claude` the way the context sensor reads it. The window
/// is this session's own, so `CLAUDE_CONTEXT_WINDOW` applies
/// ([`ways_core::context_fill::Owner::Own`]). `None` without a resolved
/// session, without a transcript, or before a turn reports API usage: the
/// byte estimate is not shown as a figure, as for a peer.
fn own_context_pct(claude: &claude_sessions::ClaudeDir, cwd: &str, session_id: Option<&str>) -> Option<f64> {
    use ways_core::context_fill::{measure_session, Method};
    let fill = measure_session(claude, Some(cwd), session_id?)?;
    (fill.model.is_some() && fill.method == Method::Api).then(|| fill.percent())
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

fn agent(
    session_id: Option<String>,
    cwd: &str,
    instance: Option<String>,
    status: String,
    context_pct: Option<f64>,
    here: bool,
    rowless: &[String],
) -> Agent {
    let (nickname, name) = match &session_id {
        Some(sid) => names(cwd, sid, instance.as_deref(), &rowless.iter().map(String::as_str).collect::<Vec<_>>()),
        // An unresolved process has no session id to show.
        None => {
            let nickname = agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string();
            (nickname.clone(), nickname)
        }
    };
    let instance = instance.filter(|i| i != attend_instances::BARE);
    #[cfg(feature = "sensor-keepwarm")]
    let timezone = session_id.as_deref().and_then(crate::cmd::keepwarm::kept_zone);
    #[cfg(not(feature = "sensor-keepwarm"))]
    let timezone = None;
    Agent {
        session_id,
        cwd: cwd.to_string(),
        name,
        nickname,
        instance,
        status,
        finished: None,
        context_pct,
        timezone,
        here,
    }
}

/// Session `session_id`'s `(nickname, name)` at `cwd`: the persona, and the
/// name peers see it by ([`attend_instances::session_name`]). `rowless`
/// holds the ids of the sessions with no row in that directory.
///
/// References:
/// - ADR-129, amendment of 2026-11-24: the first live session is bare, and
///   a session with no row shows a short session id.
pub(crate) fn names(cwd: &str, session_id: &str, instance: Option<&str>, rowless: &[&str]) -> (String, String) {
    let nickname = agent_identity::Identity::for_cwd(cwd, agent_theme::ColorDepth::NoColor).nickname.to_string();
    let name = attend_instances::session_name(&nickname, session_id, instance, rowless);
    (nickname, name)
}

#[cfg(feature = "sensor-peers")]
fn list_peers() -> Vec<(String, String, String, String, Option<f64>)> {
    crate::sensors::PeerSensor::new().list_peers()
}

#[cfg(not(feature = "sensor-peers"))]
fn list_peers() -> Vec<(String, String, String, String, Option<f64>)> {
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
            finished: None,
            context_pct: None,
            timezone: None,
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

    /// A Claude config dir holding one transcript, `lines`, for session
    /// `sid` of the project at `cwd`.
    fn claude_with_transcript(tag: &str, cwd: &str, sid: &str, lines: &str) -> (std::path::PathBuf, claude_sessions::ClaudeDir) {
        let root = std::env::temp_dir().join(format!("attend-peers-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let claude = claude_sessions::ClaudeDir::at(&root);
        let dir = claude.project_dir(cwd);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{sid}.jsonl")), lines).unwrap();
        (root, claude)
    }

    #[test]
    fn self_reads_its_own_fill_from_an_api_usage_line() {
        let sid = "11111111-1111-1111-1111-111111111111";
        let (root, claude) = claude_with_transcript(
            "self-api",
            "/srv/proj",
            sid,
            "{\"type\":\"user\"}\n{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":1000,\"cache_read_input_tokens\":99000,\"cache_creation_input_tokens\":0}}}\n",
        );
        let me = self_agent(&claude, Some(sid.to_string()), "/srv/proj", None, &[]);
        std::fs::remove_dir_all(&root).ok();
        assert!(me.context_pct.is_some_and(|p| p > 0.0), "self's fill from its API usage line, got {:?}", me.context_pct);
    }

    #[test]
    fn self_without_an_api_usage_line_has_no_figure() {
        // Only user turns so far: the byte estimate is not shown as a figure.
        let sid = "22222222-2222-2222-2222-222222222222";
        let (root, claude) = claude_with_transcript("self-bytes", "/srv/proj", sid, "{\"type\":\"user\",\"message\":\"hello\"}\n");
        let me = self_agent(&claude, Some(sid.to_string()), "/srv/proj", None, &[]);
        let unresolved = self_agent(&claude, None, "/srv/proj", None, &[]);
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(me.context_pct, None);
        assert_eq!(unresolved.context_pct, None);
    }

    #[test]
    fn an_unknown_figure_serializes_as_null() {
        let a = named("33333333-3333-3333-3333-333333333333", "/srv/proj", "McKay");
        let json = serde_json::to_value(&a).unwrap();
        assert!(json["context_pct"].is_null());
    }

    #[test]
    fn members_carry_the_disambiguated_scope() {
        let me = named("11111111-1111-1111-1111-111111111111", "/a/praecipio/app", "Clelia");
        let peers = [named("22222222-2222-2222-2222-222222222222", "/a/personal/app", "Clelia")];
        let members = ["22222222-2222-2222-2222-222222222222".to_string()];
        assert_eq!(member_names(&members, &me, &peers), ["Clelia (personal/app)"]);
    }
}
