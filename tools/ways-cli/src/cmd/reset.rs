// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Reset session state — clear markers, epochs, check fire counts, and the
//! refire back-off counts.
//!
//! Unjams stale session state without restarting Claude Code. A session's
//! clocks and markers live under `sessions_root()/{session_id}/`, and its
//! per-thread fire counts and compaction epoch under the state root's
//! `refire-counts/{session_id}/`. A confirmed reset removes both, so every
//! way's next match fires as the session's first. The SessionStart hook
//! (`ways hook session-start`) clears through [`clear_session`], which
//! removes the session directory only: the counts survive a compaction or
//! a resume.
//!
//! References:
//! - ADR-711 §1: the fire count lives outside the session directory so the
//!   compaction reset does not erase it.

use anyhow::Result;
use std::path::Path;

use crate::session;
use agent_fmt::say;
use agent_theme::{paint, Role, Style};

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
            agent_fmt::print_block("No session state found.\n");
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
        agent_fmt::print_block("No session state found.\n");
        return Ok(());
    }

    let mut total = 0;
    let mut out = String::new();

    for sid in &sessions {
        let short_id = &sid[..sid.len().min(12)];
        if dry_run {
            let Some(dir) = session_state(sid) else { continue };
            say!(out, "Session {short_id}... ({} state files)", count_files(&dir));
            let ways = session::list_fired_ways(sid);
            if !ways.is_empty() {
                say!(out, "  ways: {}", ways.len());
            }
            let epoch = session::get_epoch(sid);
            if epoch > 0 {
                say!(out, "  epoch: {epoch}");
            }
            let compactions = session::refire_count::compaction_epoch(sid);
            if compactions > 0 {
                say!(out, "  compactions: {compactions}");
            }
        } else {
            let count = clear_session(sid).unwrap_or(0) + session::refire_count::clear(sid).unwrap_or(0);
            if count > 0 {
                say!(out, "Session {short_id}...: cleared ({count} state files)");
                total += count;
            }
        }
    }

    if dry_run {
        say!(out);
        say!(
            out,
            "{} — no files removed. Add {} to execute.",
            paint(Style::new().role(Role::Warn).bold(), "Dry run"),
            paint(Style::new().bold(), "--confirm")
        );
        say!(out);
        for line in [
            "Note: resetting mid-session causes all ways to re-fire on the next",
            "hook invocation. Core guidance, checks, progressive disclosure state",
            "and each way's refire back-off count restart from scratch. This is",
            "safe but noisy — best used when the session feels jammed or after",
            "significant context shifts.",
        ] {
            say!(out, "{}", paint(Role::Muted, line));
        }
    } else if total > 0 {
        say!(out, "\nReset complete. Ways will re-disclose on next hook invocation.");
    } else {
        say!(out, "Nothing to clear.");
    }
    agent_fmt::print_block(&out);

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
