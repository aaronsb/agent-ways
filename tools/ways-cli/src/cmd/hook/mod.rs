//! `ways hook <event>`: the one interface the hook scripts call (ADR-504 §11).
//!
//! A hook script is a transport adapter: it hands Claude Code's payload on
//! stdin to this command and returns what it prints. Every decision the hook
//! needs (which ways fire, what to inject, what state to keep) is made here,
//! so a Claude Code mod can call the same logic later without a shell hook.

mod input;
mod post_tool;
mod response;
mod subagent;

pub use input::HookEvent;
use input::{HookInput, Request};

use anyhow::Result;
use std::io::Read;

use crate::cmd::scan;
use crate::session;

pub fn run(event: HookEvent) -> Result<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let input = HookInput::parse(&raw);
    let common = input.common(crate::util::env_project_dir());

    // The binary's readers take the agent and the project from the
    // environment, as do the macros and postchecks it runs.
    if let Some(agent) = &common.agent_id {
        std::env::set_var("CLAUDE_AGENT_ID", agent);
    }
    if let Some(project) = &common.project {
        std::env::set_var("CLAUDE_PROJECT_DIR", project);
    }
    let project_dir = common.project.clone().unwrap_or_else(crate::util::project_dir);
    let project = Some(project_dir.as_str());
    let transcript = common.transcript.as_deref();

    // ADR-184 item 6: a project (or user config) with `enabled: false`
    // injects nothing. Session upkeep (clearing, the response record, the
    // tasks marker) still runs.
    let injects = !matches!(
        event,
        HookEvent::Stop | HookEvent::SessionStart | HookEvent::TasksActive
    );
    let config = crate::config::Config::load(&project_dir);
    if injects && !config.enabled {
        return Ok(());
    }

    let request = input.request(event);
    // A Task naming a defined agent injects nothing, whatever the switches say.
    if let Request::Task { subagent_type: Some(t), .. } = &request {
        if subagent::is_defined_agent(t, std::path::Path::new(&project_dir), &crate::paths::projection_root()) {
            return Ok(());
        }
    }
    if let Some(lane) = subagent_lane(&request, common.agent_id.is_some()) {
        if let Some(switch) = subagent_switch(config.subagents, request_session(&request)) {
            suppress(&request, lane, switch, common.agent_id.as_deref(), &project_dir);
            return Ok(());
        }
    }

    match request {
        Request::Skip => Ok(()),
        Request::Prompt { session, query } => {
            let response = response::read(&session);
            scan::prompt(&query, &session, project, response.as_deref(), transcript)
        }
        Request::State { session, hook_event, query } => {
            scan::state(&session, project, transcript, &hook_event, query.as_deref())
        }
        Request::Command { session, command, description } => {
            scan::command(&command, description.as_deref(), &session, project, transcript)
        }
        Request::File { session, path } => scan::file(&path, &session, project, transcript),
        Request::Task { session, query, team, .. } => scan::task(&query, &session, project, team.as_deref()),
        Request::PostTool { session, hook_event } => {
            // As in the scan lanes: fired ways read the model and the refire
            // window from the invoking agent's transcript.
            crate::cmd::show::set_firing_transcript(transcript);
            emit(&hook_event, &post_tool::scan(&raw, &session, &project_dir));
            Ok(())
        }
        Request::Queued { session, transcript } => {
            // Operator messages are queued to the main agent, and a subagent's
            // hook names main's transcript and shares the session's scan mark,
            // so only the main agent's tool calls scan them.
            if common.agent_id.is_some() {
                return Ok(());
            }
            // A transcript not yet readable, or a failed scan, injects nothing:
            // this lane rides every tool call and must stay quiet.
            let _ = scan::messages(&session, project, Some(&transcript));
            Ok(())
        }
        Request::Stop { session, transcript } => {
            let path = std::path::Path::new(&transcript);
            if path.is_file() {
                response::record(&session, path)?;
            }
            Ok(())
        }
        Request::SubagentStart { session } => {
            emit("SubagentStart", &subagent::inject(&session, &project_dir)?);
            Ok(())
        }
        Request::SessionStart { session } => {
            if let Some(sid) = &session {
                crate::cmd::reset::clear_session(sid);
            }
            session::prune_subagent_switches(SUBAGENT_SWITCH_MAX_AGE);
            session::log_event(&[
                ("event", "session_start"),
                ("project", common.project.as_deref().unwrap_or("unknown")),
                ("session", session.as_deref().unwrap_or("unknown")),
            ]);
            Ok(())
        }
        Request::TasksActive { session } => {
            if session::is_plain_session_id(&session) {
                let dir = session::session_dir(&session);
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join("tasks-active"), "")?;
            }
            Ok(())
        }
    }
}

/// How long a session's subagent switch outlives its last change. A session
/// that runs longer than this with ways off for its subagents switches again.
const SUBAGENT_SWITCH_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// The lane a request injects into a subagent or teammate through, or `None`
/// for the main agent's own lanes and for session upkeep. The Task dispatch
/// and SubagentStart serve the dispatched agent; any other injecting request
/// that carries an `agent_id` comes from inside one.
fn subagent_lane(request: &Request, from_subagent: bool) -> Option<&'static str> {
    match request {
        Request::Task { .. } => Some("task"),
        Request::SubagentStart { .. } => Some("subagent_start"),
        _ if !from_subagent => None,
        Request::Prompt { .. } => Some("prompt"),
        Request::State { .. } => Some("state"),
        Request::Command { .. } => Some("command"),
        Request::File { .. } => Some("file"),
        Request::PostTool { .. } => Some("post_tool"),
        Request::Queued { .. } => Some("queued"),
        Request::Stop { .. } | Request::SessionStart { .. } | Request::TasksActive { .. } | Request::Skip => None,
    }
}

fn request_session(request: &Request) -> Option<&str> {
    match request {
        Request::Prompt { session, .. }
        | Request::State { session, .. }
        | Request::Command { session, .. }
        | Request::File { session, .. }
        | Request::Task { session, .. }
        | Request::PostTool { session, .. }
        | Request::Queued { session, .. }
        | Request::Stop { session, .. }
        | Request::SubagentStart { session }
        | Request::TasksActive { session } => Some(session),
        Request::SessionStart { session } => session.as_deref(),
        Request::Skip => None,
    }
}

/// Which switch, if any, keeps ways out of subagents here: the session's own
/// (`ways session subagents off`), then the project's or the user's
/// `subagents: false` (`configured`).
fn subagent_switch(configured: bool, session: Option<&str>) -> Option<&'static str> {
    if session.is_some_and(session::subagents_off) {
        return Some("session");
    }
    (!configured).then_some("config")
}

/// Drop what would have reached the subagent and log the suppression: the
/// dispatch ran with injection switched off, whether or not a way would have
/// matched. A Task dispatch logs once per dispatch; hooks from inside a
/// subagent log once per agent.
fn suppress(request: &Request, lane: &str, switch: &str, agent: Option<&str>, project_dir: &str) {
    let Some(session_id) = request_session(request) else { return };
    match request {
        // A stash written before the switch went off is claimed and dropped,
        // so a later SubagentStart cannot inject it.
        Request::SubagentStart { session } => {
            subagent::discard(session);
            return;
        }
        Request::Task { .. } => {}
        _ => {
            if !session::first_suppression_for(session_id, agent.unwrap_or("unknown")) {
                return;
            }
        }
    }
    let mut fields = vec![
        ("event", "injection_suppressed"),
        ("reason", "subagents_off"),
        ("switch", switch),
        ("lane", lane),
        ("scope", "subagent"),
        ("project", project_dir),
        ("session", session_id),
    ];
    if let Some(agent) = agent {
        fields.push(("agent", agent));
    }
    session::log_event(&fields);
}

/// Print the hook envelope when there is visible context to inject.
fn emit(hook_event: &str, context: &str) {
    if !context.trim().is_empty() {
        scan::emit_hook_context(hook_event, context);
    }
}
