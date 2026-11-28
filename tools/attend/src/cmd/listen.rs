// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend listen` and `attend message unread`: the verbs are `verbs::listen` and
//! `verbs::unread`; this renders them.

use crate::cli::{ListenCmd, ListenHost};
use attend_presence::enrollment::Host;
use crate::verbs::listen::{self, Listening};
use crate::verbs::unread;

/// `attend listen on|off|status|beat|finish`. Prints the state that results,
/// `listening` or `not listening`, with `, finished` while the session is
/// marked finished; `--json` prints [`Listening`].
pub(crate) fn cmd_listen(sub: ListenCmd, json: bool) {
    let result = match sub {
        ListenCmd::On { host } => listen::on(host.map(|ListenHost::Mod| Host::Mod)),
        ListenCmd::Off => listen::off(),
        ListenCmd::Status => Ok(listen::status()),
        ListenCmd::Beat { ref mod_version } => listen::beat(mod_version.as_deref()),
        ListenCmd::Finish => listen::finish(),
    };
    let state = match result {
        Ok(state) => state,
        Err(e) => {
            if json {
                crate::cmd::print_json_error(&e, &e.to_string());
            }
            eprintln!("attend listen: {e}");
            std::process::exit(1);
        }
    };
    if !state.resolved {
        eprintln!("[attend] no Claude Code session owns this process; nothing listens for it");
    }
    if json {
        crate::cmd::print_json(&state);
    } else {
        println!("{}", words(&state));
    }
}

fn words(state: &Listening) -> &'static str {
    match (state.listening, state.finished.is_some()) {
        (true, false) => "listening",
        (true, true) => "listening, finished",
        (false, false) => "not listening",
        (false, true) => "not listening, finished",
    }
}

/// `attend message unread`: `N unread`, or `N waiting · cold` when the prompt cache
/// has lapsed under an addressed message, then one line per room with
/// unread messages; `--json` prints [`unread::Unread`].
pub(crate) fn cmd_unread(json: bool) {
    let counts = unread::unread();
    if json {
        crate::cmd::print_json(&counts);
        return;
    }
    println!("{}", unread::headline(&counts));
    for room in counts.rooms.iter().filter(|r| r.count > 0) {
        println!("  {} {}", room.label, room.count);
    }
    if counts.held_back > 0 {
        println!("  ({} older held back)", counts.held_back);
    }
}
