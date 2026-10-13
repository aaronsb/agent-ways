//! `attend listen` and `attend message unread`: the verbs are `verbs::listen` and
//! `verbs::unread`; this renders them.

use crate::cli::{ListenCmd, ListenHost};
use attend_presence::enrollment::Host;
use crate::verbs::listen::{self, Listening};
use crate::verbs::unread;

/// `attend listen on|off|status`. Prints the state that results, `listening`
/// or `not listening`; `--json` prints [`Listening`].
pub(crate) fn cmd_listen(sub: ListenCmd, json: bool) {
    let result = match sub {
        ListenCmd::On { host } => listen::on(host.map(|ListenHost::Mod| Host::Mod)),
        ListenCmd::Off => listen::off(),
        ListenCmd::Status => Ok(listen::status()),
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
    if matches!(sub, ListenCmd::Off) && state.listening && state.run_live {
        eprintln!("[attend] an attend run holds this session; it listens until that run exits");
    }
    if !state.resolved {
        eprintln!("[attend] no Claude Code session owns this process; nothing listens for it");
    }
    if json {
        crate::cmd::print_json(&state);
    } else {
        println!("{}", word(&state));
    }
}

fn word(state: &Listening) -> &'static str {
    if state.listening {
        "listening"
    } else {
        "not listening"
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
