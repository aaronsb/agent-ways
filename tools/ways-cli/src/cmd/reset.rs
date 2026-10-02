//! Reset session state — clear markers, epochs, and check fire counts.
//!
//! Unjams stale session state without restarting Claude Code. Every piece of
//! a session's state lives under `sessions_root()/{session_id}/`, so clearing
//! a session is removing that one directory. The SessionStart hook
//! (`ways hook session-start`) clears through [`clear_session`] as well.

use anyhow::Result;
use std::path::Path;

use crate::session;
use agent_theme::{pair, paint, Role, Style};

/// Remove one session's state directory. Returns the number of files it held,
/// `None` when the id is not a plain session id ([`session::is_plain_session_id`])
/// or the session has no state. An id that would climb out of the sessions
/// root, or name the root itself, removes nothing.
pub fn clear_session(session_id: &str) -> Option<usize> {
    let dir = session_state(session_id)?;
    let count = count_files(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    Some(count)
}

/// The session's state directory when the id is plain and the directory exists.
fn session_state(session_id: &str) -> Option<std::path::PathBuf> {
    if !session::is_plain_session_id(session_id) {
        return None;
    }
    let dir = session::session_dir(session_id);
    dir.is_dir().then_some(dir)
}

pub fn run(session: Option<&str>, all: bool, confirm: bool) -> Result<()> {
    let dry_run = !confirm;

    if let Some(sid) = session.filter(|s| !session::is_plain_session_id(s)) {
        anyhow::bail!("not a session id: {sid:?}");
    }

    let sessions = if all {
        session::list_sessions()
    } else if let Some(sid) = session {
        vec![sid.to_string()]
    } else {
        let all_sessions = session::list_sessions();
        if all_sessions.is_empty() {
            println!("No session state found.");
            return Ok(());
        }
        if all_sessions.len() == 1 {
            all_sessions
        } else {
            let newest = find_newest_session(&all_sessions);
            eprintln!(
                "Found {} sessions, resetting newest: {}",
                all_sessions.len(),
                &newest[..newest.len().min(12)]
            );
            eprintln!("  (use --all to reset all, or --session <id> to target one)");
            vec![newest]
        }
    };

    if sessions.is_empty() {
        println!("No session state found.");
        return Ok(());
    }

    let mut total = 0;

    for sid in &sessions {
        let short_id = &sid[..sid.len().min(12)];
        if dry_run {
            let Some(dir) = session_state(sid) else { continue };
            println!("Session {short_id}... ({} state files)", count_files(&dir));
            let ways = session::list_fired_ways(sid);
            if !ways.is_empty() {
                println!("  ways: {}", ways.len());
            }
            let epoch = session::get_epoch(sid);
            if epoch > 0 {
                println!("  epoch: {epoch}");
            }
        } else if let Some(count) = clear_session(sid) {
            println!("Session {short_id}...: cleared ({count} state files)");
            total += count;
        }
    }

    if dry_run {
        println!();
        println!(
            "{} — no files removed. Add {} to execute.",
            paint(Style::new().role(Role::Warn).bold(), "Dry run"),
            paint(Style::new().bold(), "--confirm")
        );
        println!();
        let (dim, off) = pair(Role::Muted);
        println!("{dim}Note: resetting mid-session causes all ways to re-fire on the next");
        println!("hook invocation. Core guidance, checks, and progressive disclosure");
        println!("state will restart from scratch. This is safe but noisy — best used");
        println!("when the session feels jammed or after significant context shifts.{off}");
    } else if total > 0 {
        println!("\nReset complete. Ways will re-disclose on next hook invocation.");
    } else {
        println!("Nothing to clear.");
    }

    Ok(())
}

fn count_files(dir: &Path) -> usize {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count()
}

fn find_newest_session(sessions: &[String]) -> String {
    let mut newest = (std::time::UNIX_EPOCH, sessions[0].clone());

    for sid in sessions {
        if let Ok(mtime) = std::fs::metadata(session::session_dir(sid)).and_then(|m| m.modified()) {
            if mtime > newest.0 {
                newest = (mtime, sid.clone());
            }
        }
    }

    newest.1
}
