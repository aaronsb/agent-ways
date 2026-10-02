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
    if injects && !scan::enabled_for(project) {
        return Ok(());
    }

    match input.request(event) {
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
        Request::Task { session, query, team, subagent_type } => {
            let defined = subagent_type.as_deref().is_some_and(|t| {
                subagent::is_defined_agent(t, std::path::Path::new(&project_dir), &crate::paths::projection_root())
            });
            if defined {
                return Ok(());
            }
            scan::task(&query, &session, project, team.as_deref())
        }
        Request::PostTool { session, hook_event } => {
            // As in the scan lanes: fired ways read the model and the refire
            // window from the invoking agent's transcript.
            crate::cmd::show::set_firing_transcript(transcript);
            emit(&hook_event, &post_tool::scan(&raw, &session, &project_dir));
            Ok(())
        }
        Request::Queued { session, transcript } => {
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

/// Print the hook envelope when there is visible context to inject.
fn emit(hook_event: &str, context: &str) {
    if !context.trim().is_empty() {
        scan::emit_hook_context(hook_event, context);
    }
}
