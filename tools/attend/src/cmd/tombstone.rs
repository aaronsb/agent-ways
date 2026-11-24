// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend message tombstone`: renders `verbs::tombstone`.

use crate::cli::TombstoneArgs;
use crate::verbs::tombstone::{self, TombstoneError};

pub(crate) fn cmd_tombstone(a: TombstoneArgs) {
    if a.list {
        let t = tombstone::list();
        if a.json {
            crate::cmd::print_json(&t);
        } else if t.sessions.is_empty() {
            println!("no tombstones");
        } else {
            for s in &t.sessions {
                println!("{s}");
            }
        }
        return;
    }
    if a.ended {
        return ended(a.dry_run, a.json);
    }
    // Checked here: a clap `requires = "ended"` let `--dry-run <id>`
    // through as a tombstone of the id.
    if a.dry_run {
        let why = "--dry-run takes --ended";
        crate::cmd::refuse_usage(&format!("attend message tombstone: {why}"), why, a.json);
    }
    // `--undo` names a session or a batch, as its value or, for a session,
    // as the session id beside it; with neither it takes the newest batch.
    let session = match (a.session, a.undo) {
        (Some(_), Some(v)) if !v.is_empty() => {
            let why = "--undo takes a session id or a batch, not both";
            crate::cmd::refuse_usage(&format!("attend message tombstone: {why}"), why, a.json);
        }
        (Some(s), Some(_)) => return single(tombstone::undo(&s), a.json),
        (None, Some(v)) if v.is_empty() => return undo_batch(None, a.json),
        (None, Some(v)) if attend_groups::is_agent_member(&v) => return single(tombstone::undo(&v), a.json),
        (None, Some(v)) => return undo_batch(Some(&v), a.json),
        (Some(s), None) => s,
        (None, None) => {
            let why = "name a session id, or pass --ended or --list";
            crate::cmd::refuse_usage(&format!("attend message tombstone: {why}"), why, a.json);
        }
    };
    single(tombstone::tombstone(&session), a.json);
}

/// One session tombstoned, or its tombstone removed.
fn single(result: Result<tombstone::Tombstoned, TombstoneError>, json: bool) {
    match result {
        Ok(t) if json => crate::cmd::print_json(&t),
        Ok(t) if t.moved => println!(
            "tombstone on {} moved to {}: every reader skips what it sent up to then",
            t.session,
            t.at.as_deref().unwrap_or_default()
        ),
        Ok(t) if t.tombstoned => println!(
            "tombstoned {} at {}: every reader skips what it sent up to then",
            t.session,
            t.at.as_deref().unwrap_or_default()
        ),
        Ok(t) => println!("removed the tombstone on {}", t.session),
        Err(e) => refuse(&e, json),
    }
}

/// `--ended`, or its dry run.
fn ended(dry_run: bool, json: bool) {
    match tombstone::ended(dry_run) {
        Ok(e) if json => crate::cmd::print_json(&e),
        Ok(e) if e.count == 0 => println!("no ended session to tombstone"),
        Ok(e) => {
            match &e.batch {
                Some(batch) => println!(
                    "tombstoned {} ended session(s) as batch {batch}: every reader skips what they sent up to now",
                    e.count
                ),
                None => println!("would tombstone {} ended session(s):", e.count),
            }
            for s in &e.sessions {
                println!("  {s}");
            }
            if e.batch.is_some() {
                println!("undo with: attend message tombstone --undo");
            }
        }
        Err(e) => refuse(&e, json),
    }
}

/// `--undo` of a batch, the newest when `batch` is `None`.
fn undo_batch(batch: Option<&str>, json: bool) {
    match tombstone::undo_ended(batch) {
        Ok(u) if json => crate::cmd::print_json(&u),
        Ok(u) => {
            let kept = match u.kept.len() {
                0 => String::new(),
                n => format!("; {n} tombstoned again since, or no longer tombstoned, stay as they are"),
            };
            println!(
                "removed the tombstones of {} session(s) from batch {}, made {}{kept}",
                u.restored,
                u.batch,
                agent_fmt::when::ago(u.age_secs)
            );
        }
        Err(e) => refuse(&e, json),
    }
}

fn refuse(e: &TombstoneError, json: bool) -> ! {
    if json {
        crate::cmd::print_json_error(e, &e.to_string());
    }
    eprintln!("attend message tombstone: {e}");
    std::process::exit(1);
}
