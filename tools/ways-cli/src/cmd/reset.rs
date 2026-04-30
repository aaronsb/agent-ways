//! Reset session state — clear markers, epochs, and check fire counts.
//!
//! Unjams stale session state without restarting Claude Code.
//!
//! Clears both the per-session directory under `sessions_root()` and any
//! known side files written by hooks to fixed `/tmp` paths (e.g. the
//! response-topics file written by the Stop hook and consumed by
//! `check-prompt.sh` to enrich prompt matching).

use anyhow::Result;
use std::path::PathBuf;

use crate::session;

/// Return paths to off-root, per-session state files that hooks write
/// outside `sessions_root()`. Reset must clear these alongside the main
/// session directory or matching can be biased by stale topic context
/// after a reset. Each path resolves through its canonical helper so
/// reset and the writing/reading hooks can never drift.
fn session_side_files(sid: &str) -> Vec<PathBuf> {
    vec![session::response_topics_path(sid)]
}


pub fn run(session: Option<&str>, all: bool, confirm: bool) -> Result<()> {
    let dry_run = !confirm;

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
        let dir = format!("{}/{sid}", session::sessions_root());
        let path = std::path::Path::new(&dir);
        let side_files: Vec<PathBuf> = session_side_files(sid)
            .into_iter()
            .filter(|p| p.exists())
            .collect();

        let has_main = path.is_dir();
        if !has_main && side_files.is_empty() {
            continue;
        }

        let main_count = if has_main { count_files(path) } else { 0 };
        let count = main_count + side_files.len();
        let short_id = &sid[..sid.len().min(12)];

        if dry_run {
            println!("Session {short_id}... ({count} state files)");
            if has_main {
                let ways = session::list_fired_ways(sid);
                if !ways.is_empty() {
                    println!("  ways: {}", ways.len());
                }
                let epoch = session::get_epoch(sid);
                if epoch > 0 {
                    println!("  epoch: {epoch}");
                }
            }
            if !side_files.is_empty() {
                println!("  side files: {}", side_files.len());
            }
        } else {
            if has_main {
                let _ = std::fs::remove_dir_all(path);
            }
            for f in &side_files {
                let _ = std::fs::remove_file(f);
            }
            println!("Session {short_id}...: cleared ({count} state files)");
            total += count;
        }
    }

    if dry_run {
        println!();
        println!("\x1b[1;33mDry run\x1b[0m — no files removed. Add \x1b[1m--confirm\x1b[0m to execute.");
        println!();
        println!("\x1b[2mNote: resetting mid-session causes all ways to re-fire on the next");
        println!("hook invocation. Core guidance, checks, and progressive disclosure");
        println!("state will restart from scratch. This is safe but noisy — best used");
        println!("when the session feels jammed or after significant context shifts.\x1b[0m");
    } else if total > 0 {
        println!("\nReset complete. Ways will re-disclose on next hook invocation.");
    } else {
        println!("Nothing to clear.");
    }

    Ok(())
}

fn count_files(dir: &std::path::Path) -> usize {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count()
}

fn find_newest_session(sessions: &[String]) -> String {
    let mut newest = (std::time::UNIX_EPOCH, sessions[0].clone());

    for sid in sessions {
        let dir = format!("{}/{sid}", session::sessions_root());
        let path = std::path::Path::new(&dir);
        if let Ok(meta) = std::fs::metadata(path) {
            if let Ok(mtime) = meta.modified() {
                if mtime > newest.0 {
                    newest = (mtime, sid.clone());
                }
            }
        }
    }

    newest.1
}
