//! `attend keepwarm` — arm, disarm, and read the cache-warming floor
//! (ADR-182), and the keepalive policy the attend mod runs (ADR-414). The
//! sensor and the check do the judging; these verbs own the arm file, the
//! policy's marks in the ledger, and render the card.

use crate::cli::{KeepwarmCheckArgs, KeepwarmCmd};
use crate::util::keepwarm_dir;
use sensor_keepwarm::policy::{self, Config, Gate, Policy, Site, WrapOutcome};
use sensor_keepwarm::{
    card, card_state, check_once, fmt_duration, lock_session, parse_duration, read_arm, read_context, read_ledger, settle, write_arm,
    write_ledger, ArmFile, State, DEFAULT_WINDOW_SECS, PING_AFTER_SECS,
};
use std::path::Path;

/// When a policy check that could not read the transcript asks to be run
/// again.
const RETRY_SECS: u64 = 300;
/// How long the policy check reuses its read of the attend_mod settings,
/// unless a settings file it reads from changed since.
const SETTINGS_KEPT_SECS: u64 = 300;

pub(crate) fn cmd_keepwarm(sub: KeepwarmCmd) {
    let ident = attend_presence::session::identity();
    if let KeepwarmCmd::Check(args) = &sub {
        return check(&ident, args);
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
            let arm = ArmFile { deadline: now + secs, armed_at: now, window_secs: secs, auto: false };
            if let Err(e) = write_arm(&dir, sid, &arm) {
                eprintln!("keepwarm: could not write the arm file: {e}");
                std::process::exit(1);
            }
            // The operator's word clears a stop of the keepalive policy.
            update_policy(&dir, sid, policy::clear_stop);
            println!(
                "keepwarm on for {}: one wake {} into each idle stretch keeps the cache read, not re-written. Answer it with one word.",
                fmt_duration(secs),
                fmt_duration(PING_AFTER_SECS),
            );
        }
        KeepwarmCmd::Off => {
            let arm = ArmFile { deadline: 0, armed_at: now, window_secs: 0, auto: false };
            if let Err(e) = write_arm(&dir, sid, &arm) {
                eprintln!("keepwarm: could not write the arm file: {e}");
                std::process::exit(1);
            }
            // And the keepalive policy stops until the operator's next prompt.
            update_policy(&dir, sid, policy::stop_by_operator);
            println!("keepwarm is not armed");
        }
        KeepwarmCmd::Check(_) => unreachable!("handled above"),
        KeepwarmCmd::Reset { at } => {
            let at = at.filter(|t| *t <= now).unwrap_or(now);
            if !update_policy(&dir, sid, |p| policy::reset(p, at)) {
                std::process::exit(1);
            }
            println!("keepalive count reset");
        }
        KeepwarmCmd::Wrapped { outcome, detail } => {
            let Some(o) = WrapOutcome::parse(&outcome) else {
                eprintln!("keepwarm wrapped takes transcript, file, sent, refused or unfinished");
                std::process::exit(2);
            };
            if !update_policy(&dir, sid, |p| p.wrap_outcome = Some(o.describe(&detail))) {
                std::process::exit(1);
            }
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
            // In a session the attend mod hosts, the mod's own switch decides
            // whether the check runs at all, whatever is armed here. The
            // keepalive lines show wherever the mod has run the policy's
            // check, a dormant session's included.
            let is_mod_host = attend_presence::enrollment::host(sid) == Some(attend_presence::enrollment::Host::Mod);
            let ran_policy = ledger.as_ref().is_some_and(|l| l.policy.gate.is_some());
            if is_mod_host || ran_policy {
                let settings = mod_settings(&ident.origin_path);
                if is_mod_host {
                    println!("{}", mod_setting_line(settings.as_deref()));
                }
                let cfg = Config::from_settings(settings.as_deref());
                let state = card_state(&reading, arm.as_ref(), ledger.as_ref());
                for line in policy::card(&state, &reading, &cfg, &site(&ident), now) {
                    println!("{line}");
                }
            }
        }
    }
}

/// Apply `f` to the policy's marks in `sid`'s ledger, a fresh ledger where
/// there is none, under the session's lock. False, with the reason on
/// stderr, when it cannot be written.
fn update_policy(dir: &Path, sid: &str, f: impl FnOnce(&mut Policy)) -> bool {
    let _lock = lock_session(dir, sid).ok();
    let mut ledger = read_ledger(dir, sid).unwrap_or_default();
    f(&mut ledger.policy);
    ledger.updated_at = sensor_trait::epoch_secs();
    match write_ledger(dir, sid, &ledger) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("keepwarm: could not write the ledger: {e}");
            false
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

/// What `check --json` prints when nothing was judged: why, and when to ask
/// again.
fn nothing(reason: &str, next_at: Option<u64>) -> serde_json::Value {
    serde_json::json!({
        "action": "none", "text": null, "next_at": next_at, "count": 0, "max": 0,
        "wraps_at": null, "reason": reason, "policy": false, "state": "off",
        "wraps_at_hm": null, "wrapped_at_hm": null, "wrap": null, "retry": false,
    })
}

/// This session as the wrap's file, recipient and handoff line name it.
fn site(ident: &attend_presence::session::SessionIdentity) -> Site {
    Site {
        session_id: ident.session_id.clone(),
        project: ident.origin_path.clone(),
        name: crate::util::display_name(ident),
        operator: agent_identity::sanitize_id_component(&agent_identity::signal::user_name()),
        state_root: ways_core::paths::state_root(),
        home: claude_sessions::home_dir(),
    }
}

/// The attend_mod settings for the policy check: a read kept beside the
/// ledger for a few minutes, so a listening session that asks every minute
/// does not run ways every minute, unless the user's or the project's
/// settings file changed since. `None` when ways cannot answer.
fn policy_settings(dir: &Path, sid: &str, project: &str, now: u64) -> Option<String> {
    let path = dir.join(format!("{sid}.settings.json"));
    let changed_after = |t: u64| {
        let files = [ways_core::paths::user_config(), Path::new(project).join(".claude").join("ways.yaml")];
        files.iter().any(|f| {
            std::fs::metadata(f)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .is_some_and(|m| m.as_secs() >= t)
        })
    };
    if let Some(kept) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        let at = kept.get("at").and_then(|v| v.as_u64()).unwrap_or(0);
        let same = kept.get("project").and_then(|v| v.as_str()) == Some(project);
        if same && now.saturating_sub(at) < SETTINGS_KEPT_SECS && !changed_after(at) {
            if let Some(doc) = kept.get("doc").and_then(|v| v.as_str()) {
                return Some(doc.to_string());
            }
        }
    }
    let doc = mod_settings(project)?;
    let kept = serde_json::json!({ "at": now, "project": project, "doc": doc });
    std::fs::write(&path, kept.to_string()).ok();
    Some(doc)
}

/// `attend keepwarm check`: the sensor's judgment, once. Prints the
/// `keepwarm:` line when a wake is due and nothing otherwise, and exits 0
/// either way, so a polling host reads stdout alone. Why nothing printed
/// goes to stderr. With `--policy` the keepalive policy judges (ADR-414);
/// with `--json` the decision prints as one object, whatever it is.
fn check(ident: &attend_presence::session::SessionIdentity, args: &KeepwarmCheckArgs) {
    let quiet = |reason: &str, next_at: Option<u64>| {
        if args.json {
            println!("{}", nothing(reason, next_at));
        } else {
            eprintln!("keepwarm: {reason}");
        }
    };
    if !ident.session_resolved {
        return quiet("no Claude session owns this process, nothing to warm", None);
    }
    let sid = ident.session_id.as_str();
    // Under `attend run` the sensor holds the state and wakes the session;
    // a second judge would wake it twice.
    if attend_presence::heartbeat::run_is_live(sid) {
        return quiet("an attend run holds this session; its sensor keeps the cache warm", None);
    }
    let cfg = crate::config::Config::load(&ident.origin_path);
    if !cfg.sensors.get("keepwarm").map(|s| s.enabled).unwrap_or(true) {
        return quiet("the keepwarm sensor is off in attend's config", None);
    }
    let now = sensor_trait::epoch_secs();
    if !args.policy {
        let line = check_once(sid, &keepwarm_dir(), &ident.origin_path, now);
        if args.json {
            let doc = match line {
                Some(text) => serde_json::json!({ "action": "keepalive", "text": text, "next_at": now + 60, "policy": false, "state": "off" }),
                None => nothing("no wake is due", Some(now + 60)),
            };
            println!("{doc}");
        } else if let Some(line) = line {
            println!("{line}");
        }
        return;
    }

    let dir = keepwarm_dir();
    let gate = Gate::parse(&args.autocompact);
    let pcfg = Config::from_settings(policy_settings(&dir, sid, &ident.origin_path, now).as_deref());
    let Some(reading) = read_context(sid, &ident.origin_path) else {
        let why = "could not read the session transcript through `ways context`";
        if args.json {
            // Where the policy runs, it still runs: the host asks again.
            let mut doc = nothing(why, Some(now + RETRY_SECS));
            if pcfg.why_off(gate).is_none() {
                doc["policy"] = true.into();
                doc["state"] = "running".into();
                doc["retry"] = true.into();
            }
            println!("{doc}");
        } else {
            eprintln!("keepwarm: {why}");
        }
        return;
    };
    let site = site(ident);
    // One read-step-write at a time per session: two processes on one
    // session id never both send a keepalive or a wrap.
    let _lock = lock_session(&dir, sid).ok();
    let mut state = State::resume(read_ledger(&dir, sid));
    let arm = read_arm(&dir, sid);
    let (mut out, decision) = policy::step(&mut state, &reading, arm.as_ref(), &pcfg, gate, &site, now);
    // The cursor and the gate move with every step.
    out.changed = true;
    settle(&dir, sid, &state, arm.as_ref(), &out, now);
    if args.json {
        println!("{}", serde_json::to_string(&decision).unwrap_or_else(|_| nothing("the decision did not serialize", None).to_string()));
    } else if let Some(text) = &decision.text {
        println!("{text}");
    } else {
        eprintln!("keepwarm: {}", decision.reason);
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
