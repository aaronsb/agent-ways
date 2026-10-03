//! `ways session subagents`: switch ways off or on for one session's
//! subagents and teammates, or report which switch is in effect (#768).
//!
//! The project's or the user's `subagents:` setting is the standing switch;
//! this one is for a single session, set by the operator or by an agent before
//! it launches a workflow. It holds until switched back, through compaction
//! and `ways session reset`.

use anyhow::{bail, Result};

use super::list::{detect_session, NoSession};
use crate::session;

pub fn run(state: Option<&str>, session: Option<&str>, json: bool) -> Result<()> {
    // The session this process runs in, when Claude Code says so: an agent
    // switching its own session never lands on a sibling's.
    let own = std::env::var("CLAUDE_CODE_SESSION_ID").ok().filter(|s| !s.is_empty());
    let session_id = match (session, own) {
        (Some(s), _) => s.to_string(),
        (None, Some(s)) => s,
        // Guessing is fine for a report, never for a switch: with two live
        // sessions in a project the guess can be the other one.
        (None, None) if state.is_some() => {
            bail!("name the session to switch: --session <id> (`ways session list` lists them)")
        }
        (None, None) => match detect_session() {
            Ok(s) => s,
            Err(NoSession::NoMarkers | NoSession::AllOrphaned) => {
                bail!("no current session found; pass --session <id> (`ways session list` lists them)")
            }
        },
    };
    if !session::is_plain_session_id(&session_id) {
        bail!("not a session id: {session_id}");
    }
    if state.is_some() && !session::session_dir(&session_id).exists() {
        eprintln!("note: no ways state for session {session_id} yet; the switch applies once its hooks run");
    }
    if let Some(state) = state {
        session::set_subagents(&session_id, state == "on")?;
    }

    let project = crate::util::project_dir();
    let configured = crate::config::Config::load(&project).subagents;
    let session_off = session::subagents_off(&session_id);
    let (on, switch) = match (session_off, configured) {
        (true, _) => (false, "session"),
        (false, false) => (false, "config"),
        (false, true) => (true, "default"),
    };
    let word = |b: bool| if b { "on" } else { "off" };
    if json {
        let report = serde_json::json!({
            "session": session_id,
            "subagents": word(on),
            "switch": switch,
            "session_switch": word(!session_off),
            "configured": word(configured),
            "project": project,
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let why = match switch {
        "session" => "switched off for this session; `ways session subagents on` restores it".to_string(),
        "config" => format!("`subagents: false` in {project}/.claude/ways.yaml or the user config"),
        _ => "the default".to_string(),
    };
    println!("subagents of {session_id} get ways: {} ({why})", word(on));
    Ok(())
}
