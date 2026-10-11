//! `attend keepwarm` — arm, disarm, and read the cache-warming floor
//! (ADR-182). The sensor does the waking; these verbs own the arm file
//! and render the card from the ledger the sensor writes.

use crate::cli::KeepwarmCmd;
use crate::util::keepwarm_dir;
use sensor_keepwarm::{
    card, check_once, fmt_duration, parse_duration, read_arm, read_context, read_ledger, write_arm, ArmFile,
    DEFAULT_WINDOW_SECS, PING_AFTER_SECS,
};

pub(crate) fn cmd_keepwarm(sub: KeepwarmCmd) {
    let ident = attend_presence::session::identity();
    if let KeepwarmCmd::Check = sub {
        return check(&ident);
    }
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
        KeepwarmCmd::Check => unreachable!("handled above"),
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

/// `attend keepwarm check`: the sensor's judgment, once. Prints the
/// `keepwarm:` line when a wake is due and nothing otherwise, and exits 0
/// either way, so a polling host reads stdout alone. Why nothing printed
/// goes to stderr.
fn check(ident: &attend_presence::session::SessionIdentity) {
    if !ident.session_resolved {
        eprintln!("keepwarm: no Claude session owns this process, nothing to warm");
        return;
    }
    let sid = ident.session_id.as_str();
    // Under `attend run` the sensor holds the state and wakes the session;
    // a second judge would wake it twice.
    if attend_presence::heartbeat::run_is_live(sid) {
        eprintln!("keepwarm: an attend run holds this session; its sensor keeps the cache warm");
        return;
    }
    let cfg = crate::config::Config::load(&ident.origin_path);
    if !cfg.sensors.get("keepwarm").map(|s| s.enabled).unwrap_or(true) {
        eprintln!("keepwarm: the keepwarm sensor is off in attend's config");
        return;
    }
    if let Some(line) = check_once(sid, &keepwarm_dir(), &ident.origin_path, sensor_trait::epoch_secs()) {
        println!("{line}");
    }
}
