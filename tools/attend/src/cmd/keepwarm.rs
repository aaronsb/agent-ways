//! `attend keepwarm` — arm, disarm, and read the cache-warming floor
//! (ADR-182). The sensor does the waking; these verbs own the arm file
//! and render the card from the ledger the sensor writes.

use crate::cli::KeepwarmCmd;
use crate::util::keepwarm_dir;
use sensor_keepwarm::{
    card, fmt_duration, parse_duration, read_arm, read_context, read_ledger, write_arm, ArmFile,
    DEFAULT_WINDOW_SECS, PING_AFTER_SECS,
};

pub(crate) fn cmd_keepwarm(sub: KeepwarmCmd) {
    let ident = attend_session::identity();
    if !ident.session_resolved {
        eprintln!("keepwarm: no Claude session owns this process, nothing to warm");
        std::process::exit(1);
    }
    let sid = ident.session_id.as_str();
    let dir = keepwarm_dir();
    let now = sensor_trait::epoch_secs();

    match sub {
        KeepwarmCmd::On { window } => {
            let secs = match window.as_deref() {
                None => DEFAULT_WINDOW_SECS,
                Some(w) => match parse_duration(w) {
                    Some(s) if s > 0 => s,
                    _ => {
                        eprintln!("keepwarm on takes a window such as 6h, 90m, or 2h30m");
                        std::process::exit(2);
                    }
                },
            };
            let arm = ArmFile { deadline: now + secs, armed_at: now, window_secs: secs };
            if let Err(e) = write_arm(&dir, sid, &arm) {
                eprintln!("keepwarm: could not write the arm file: {e}");
                std::process::exit(1);
            }
            println!(
                "keepwarm on for {}: one wake {} into each idle stretch keeps the cache read, not re-written. Answer it with one word.",
                fmt_duration(secs),
                fmt_duration(PING_AFTER_SECS),
            );
        }
        KeepwarmCmd::Off => {
            let arm = ArmFile { deadline: 0, armed_at: now, window_secs: 0 };
            if let Err(e) = write_arm(&dir, sid, &arm) {
                eprintln!("keepwarm: could not write the arm file: {e}");
                std::process::exit(1);
            }
            println!("keepwarm is off");
        }
        KeepwarmCmd::Status => {
            let Some(reading) = read_context(sid, &ident.origin_path) else {
                eprintln!("keepwarm: could not read the session transcript through `ways context`");
                std::process::exit(1);
            };
            let arm = read_arm(&dir, sid);
            let ledger = read_ledger(&dir, sid);
            for line in card(&reading, arm.as_ref(), ledger.as_ref(), now) {
                println!("{line}");
            }
        }
    }
}

/// The one-line form `attend status` shows.
pub(crate) fn status_line() -> String {
    let ident = attend_session::identity();
    if !ident.session_resolved {
        return "no session".to_string();
    }
    let now = sensor_trait::epoch_secs();
    let dir = keepwarm_dir();
    let arm = read_arm(&dir, &ident.session_id);
    let ledger = read_ledger(&dir, &ident.session_id);
    match arm {
        Some(a) if a.deadline > now => format!("on, {} left", fmt_duration(a.deadline - now)),
        _ => match ledger.and_then(|l| l.stopped) {
            Some(why) => format!("stopped, {why}"),
            None => "off".to_string(),
        },
    }
}
