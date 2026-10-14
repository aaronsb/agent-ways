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
    let Some(session) = a.session else {
        let why = "name a session id, or pass --list";
        crate::cmd::refuse_usage(&format!("attend message tombstone: {why}"), why, a.json);
    };
    let result = if a.undo { tombstone::undo(&session) } else { tombstone::tombstone(&session) };
    match result {
        Ok(t) if a.json => crate::cmd::print_json(&t),
        Ok(t) if t.tombstoned => println!("tombstoned {}: every reader skips its messages", t.session),
        Ok(t) => println!("removed the tombstone on {}", t.session),
        Err(e) => refuse(&e, a.json),
    }
}

fn refuse(e: &TombstoneError, json: bool) -> ! {
    if json {
        crate::cmd::print_json_error(e, &e.to_string());
    }
    eprintln!("attend message tombstone: {e}");
    std::process::exit(1);
}
