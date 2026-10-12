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
            println!("keepwarm is not armed");
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
            // In a session the attend mod hosts, the mod's own switch decides
            // whether the check runs at all, whatever is armed here.
            if attend_presence::enrollment::host(sid) == Some(attend_presence::enrollment::Host::Mod) {
                println!("{}", mod_setting_line(mod_settings(&ident.origin_path).as_deref()));
            }
        }
    }
}

/// The effective `attend_mod` section for `project`, as the attend mod reads
/// it: `ways settings list attend_mod --json --effective`. `None` when ways
/// cannot answer within five seconds.
fn mod_settings(project: &str) -> Option<String> {
    use std::io::Read;
    let mut child = std::process::Command::new("ways")
        .args(["settings", "list", "attend_mod", "--json", "--effective", "--project", project])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(20)),
            _ => {
                child.kill().ok();
                child.wait().ok();
                return None;
            }
        }
    };
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    status.success().then_some(out)
}

/// The card's line for the attend mod's `attend_mod.keepwarm` setting, read
/// from `ways settings list attend_mod --json --effective`: its value and
/// the layer that set it. With `off` the mod never runs the check, so
/// nothing arms or wakes in this session whatever the line above says.
fn mod_setting_line(settings: Option<&str>) -> String {
    let key = settings
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|doc| doc.get("keys").and_then(|k| k.get("attend_mod.keepwarm")).cloned());
    let Some(key) = key else {
        return "mod setting attend_mod.keepwarm unknown (ways settings could not be read)".to_string();
    };
    // The mod reads anything but `off` as on (plugins/attend/hooks/lib.ts).
    let value = if key.get("value").and_then(|v| v.as_str()) == Some("off") { "off" } else { "on" };
    let from = match (key.get("layer").and_then(|v| v.as_str()), key.get("file").and_then(|v| v.as_str())) {
        (_, Some(file)) => format!(" (from {file})"),
        (Some(layer), None) => format!(" ({layer})"),
        (None, None) => String::new(),
    };
    let effect = if value == "off" { "; the mod never runs the keepwarm check in this session" } else { "" };
    format!("mod setting attend_mod.keepwarm {value}{from}{effect}")
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

#[cfg(test)]
mod tests {
    use super::mod_setting_line;

    fn settings(value: &str, layer: &str, file: Option<&str>) -> String {
        let file = file.map_or("null".to_string(), |f| format!("\"{f}\""));
        format!(r#"{{"view":"effective","keys":{{"attend_mod.keepwarm":{{"value":"{value}","default":"on","layer":"{layer}","file":{file}}}}}}}"#)
    }

    /// The clean-slate test's finding 6: the card names the mod's setting
    /// apart from whether keepwarm is armed.
    #[test]
    fn the_mod_setting_line_names_the_value_and_where_it_came_from() {
        assert_eq!(mod_setting_line(Some(&settings("on", "default", None))), "mod setting attend_mod.keepwarm on (default)");
        assert_eq!(
            mod_setting_line(Some(&settings("off", "project", Some("/p/.claude/ways.yaml")))),
            "mod setting attend_mod.keepwarm off (from /p/.claude/ways.yaml); the mod never runs the keepwarm check in this session"
        );
        assert_eq!(
            mod_setting_line(None),
            "mod setting attend_mod.keepwarm unknown (ways settings could not be read)"
        );
        assert_eq!(
            mod_setting_line(Some("not json")),
            "mod setting attend_mod.keepwarm unknown (ways settings could not be read)"
        );
    }
}
