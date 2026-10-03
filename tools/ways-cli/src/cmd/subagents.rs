//! `ways session subagents`: switch ways off or on for one session's
//! subagents and teammates, or report which switch is in effect (#768).
//!
//! The project's or the user's `subagents:` setting is the standing switch;
//! this one is for a single session, set by the operator or by an agent before
//! it launches a workflow. It holds until switched back or the session's state
//! is cleared.

use anyhow::{bail, Result};

use super::list::{detect_session, NoSession};
use crate::session;

pub fn run(state: Option<&str>, session: Option<&str>, json: bool) -> Result<()> {
    let session_id = match session {
        Some(s) => s.to_string(),
        None => match detect_session() {
            Ok(s) => s,
            Err(NoSession::NoMarkers | NoSession::AllOrphaned) => {
                bail!("no current session found; pass --session <id> (`ways session list` lists them)")
            }
        },
    };
    if !session::is_plain_session_id(&session_id) {
        bail!("not a session id: {session_id}");
    }
    match state {
        Some("on") => session::set_subagents(&session_id, true)?,
        Some("off") => session::set_subagents(&session_id, false)?,
        Some(other) => bail!("expected on or off, got {other}"),
        None => {}
    }

    let project = crate::util::project_dir();
    let project_on = crate::config::Config::load(&project).subagents;
    let session_off = session::subagents_off(&session_id);
    let (on, switch) = match (session_off, project_on) {
        (true, _) => (false, "session"),
        (false, false) => (false, "project"),
        (false, true) => (true, "default"),
    };
    if json {
        let report = serde_json::json!({
            "session": session_id,
            "subagents": if on { "on" } else { "off" },
            "switch": switch,
            "session_switch": if session_off { "off" } else { "on" },
            "project_setting": project_on,
            "project": project,
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let state = if on { "on" } else { "off" };
    let why = match switch {
        "session" => "switched off for this session; `ways session subagents on` restores it".to_string(),
        "project" => format!("`subagents: false` in {project}/.claude/ways.yaml or the user config"),
        _ => "the default".to_string(),
    };
    println!("ways for subagents of {session_id}: {state} ({why})");
    Ok(())
}
