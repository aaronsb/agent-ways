// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend keepwarm` — arm, disarm, and read the cache-warming floor
//! (ADR-182), and the keepalive policy the attend mod runs (ADR-414). The
//! sensor and the check do the judging; these verbs own the arm file, the
//! policy's marks in the ledger, and render the card.

use crate::cli::{KeepwarmCheckArgs, KeepwarmCmd, KeepwarmStatusArgs};
use crate::util::keepwarm_dir;
use sensor_keepwarm::policy::armed::{self, Judgement, Sufficiency, Verdict};
use sensor_keepwarm::policy::{self, Action, Config, Fill, Gate, Handoff, Policy, SessionSkills, Site, WrapOutcome};
use sensor_keepwarm::{
    card, card_state, check_once, fmt_duration, lock_session, parse_duration, read_arm, read_context, read_ledger, settle, write_arm,
    write_ledger, ArmFile, State, DEFAULT_WINDOW_SECS, PING_AFTER_SECS,
};
use std::path::{Path, PathBuf};

/// When a policy check that could not read the transcript asks to be run
/// again.
const RETRY_SECS: u64 = 300;
/// The largest wrap reply `wrapped --save` writes.
const MAX_REPLY_BYTES: u64 = 1024 * 1024;
/// How long the policy check reuses its read of the mods.attend settings,
/// unless a settings file it reads from changed since.
const SETTINGS_KEPT_SECS: u64 = 300;
/// How long these verbs give `ways settings` to answer.
const SETTINGS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) fn cmd_keepwarm(sub: KeepwarmCmd) {
    let ident = attend_presence::session::identity();
    if let KeepwarmCmd::Check(args) = &sub {
        return check(&ident, args);
    }
    if let KeepwarmCmd::Brief(args) = &sub {
        return super::brief::cmd_brief(&ident, args);
    }
    if let KeepwarmCmd::Status(KeepwarmStatusArgs { json, session: Some(id) }) = &sub {
        return status(&named(id, *json), *json);
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
        KeepwarmCmd::Check(_) | KeepwarmCmd::Brief(_) => unreachable!("handled above"),
        KeepwarmCmd::Reset { at } => {
            let at = at.filter(|t| *t <= now).unwrap_or(now);
            if !update_policy(&dir, sid, |p| policy::reset(p, at)) {
                std::process::exit(1);
            }
            println!("keepalive count reset");
            // The operator is back: an armed exit is disarmed with the
            // ledger, and the finished mark the check set on a listening
            // session clears as `attend listen on` clears it. A session
            // that is not listening stays so.
            //
            // References:
            // - ADR-418 item 13: an operator prompt disarms the exit.
            if attend_presence::enrollment::is_finished(sid) && crate::verbs::listen::status().listening {
                match crate::verbs::listen::on(None) {
                    Ok(_) => println!("the session is no longer finished"),
                    Err(e) => {
                        eprintln!("keepwarm: could not clear the finished mark: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
        KeepwarmCmd::Judged { wrap, verdict, handoff, own, moved, reason, operator_turn, json } => {
            let parsed = (
                Verdict::parse(&verdict),
                Sufficiency::parse(&handoff),
                Verdict::parse(&own),
                moved.as_deref().map_or(Some(None), |m| Verdict::parse(m).map(Some)),
            );
            let (Some(verdict), Some(handoff), Some(own), Some(moved)) = parsed else {
                eprintln!("keepwarm judged takes --verdict stop|compact|continue|doubt, --handoff sufficient|holds-detail, --own stop|compact|continue and --move stop|compact|continue|doubt");
                std::process::exit(2);
            };
            // The move the mod carries out; a hand-run call from before
            // --move carries out the verdict.
            let moved = moved.unwrap_or(verdict);
            let j = Judgement { wrap_id: wrap.trim().to_string(), verdict, handoff, own, moved, reason: reason.clone() };
            let file = read_handoff(&ident.origin_path);
            // attend decides whether a compact has room, from the relay
            // settings and the ledger.
            let settings = policy_settings(&dir, sid, &ident.origin_path, now, SETTINGS_TIMEOUT);
            let cfg = Config::from_settings(settings.as_deref());
            let _lock = lock_session(&dir, sid).ok();
            let mut ledger = read_ledger(&dir, sid).unwrap_or_default();
            let rec = match armed::record(&mut ledger.policy, j, file.as_ref(), &cfg, operator_turn, now) {
                Ok(rec) => rec,
                Err(why) => {
                    eprintln!("keepwarm judged refused: {why}; nothing recorded");
                    std::process::exit(1);
                }
            };
            ledger.updated_at = now;
            if let Err(e) = write_ledger(&dir, sid, &ledger) {
                eprintln!("keepwarm: could not write the ledger: {e}");
                std::process::exit(1);
            }
            let id = wrap.trim();
            let moved = rec.moved;
            let arms = matches!(moved, Verdict::Stop | Verdict::Doubt);
            // With keepwarm off the check judges nothing, so no tick runs
            // the exit.
            let keepwarm_off = keepwarm_setting_off(settings.as_deref());
            // A doubt sends the handoff to the operator over attend, on the
            // route a relay stop's handoff takes; attend owns that route.
            // A compact, and a doubt with relay on and the operator away,
            // compact with the instructions the relay builds; a compact
            // then resumes with the relay's resume and its escape.
            //
            // References:
            // - ADR-417 item 4: a relay compaction takes its instructions
            //   from the handoff.
            // - ADR-418 item 11: in relay mode, `doubt` becomes `relay: stop
            //   doubt`, and the handoff goes to the operator over attend.
            if json {
                let route = (moved == Verdict::Doubt).then(|| policy::doubt_route(&cfg, &site(&ident), reason.as_deref(), operator_turn, now));
                let compacts = moved == Verdict::Compact || (moved == Verdict::Doubt && cfg.relay && !operator_turn);
                let instructions = file.as_ref().filter(|_| compacts).map(|h| h.instructions.clone());
                let resume = file
                    .as_ref()
                    .filter(|_| moved == Verdict::Compact)
                    .map(|h| policy::boundary_resume_text(&h.path, ledger.policy.relays + 1, cfg.max_relays));
                crate::cmd::print_json(&serde_json::json!({
                    "recorded": true,
                    "armed": arms,
                    "acted_on": arms && !keepwarm_off,
                    "move": moved.as_str(),
                    "compact_refused": rec.compact_refused,
                    "wrap": route,
                    "instructions": instructions,
                    "resume": resume,
                }));
                return;
            }
            if let Some(why) = &rec.compact_refused {
                println!("compact refused for wrap {id}: {why}; recorded as stop");
            }
            if arms && keepwarm_off {
                println!("exit armed for wrap {id}, but mods.attend.keepwarm is off: no tick runs, so the exit does nothing while it is off");
            } else if arms {
                let stay = match handoff {
                    Sufficiency::Sufficient => "while something is pending, up to the price of a fresh start",
                    Sufficiency::HoldsDetail => "up to the price of a cold reload, since the handoff holds detail",
                };
                println!("exit armed for wrap {id}: each idle tick pings {stay}, and otherwise lets the cache lapse and marks the session finished");
            } else if moved == verdict {
                println!("judged wrap {id}: {}, the handoff {}; no exit is armed", verdict.as_str(), handoff.as_str());
            } else {
                println!("judged wrap {id}: {}, carried out {}, the handoff {}; no exit is armed", verdict.as_str(), moved.as_str(), handoff.as_str());
            }
        }
        KeepwarmCmd::Wrapped { outcome, detail, path, save, focus, marker } => {
            let Some(o) = WrapOutcome::parse(&outcome) else {
                eprintln!("keepwarm wrapped takes transcript, file, sent, refused or unfinished");
                std::process::exit(2);
            };
            if save && !matches!(o, WrapOutcome::File | WrapOutcome::Refused) {
                eprintln!("keepwarm wrapped --save writes the reply for file or refused only");
                std::process::exit(2);
            }
            let (line, wrote) = match (save, path) {
                (true, Some(path)) => saved_outcome(o, &detail, &path, sid),
                _ => (o.describe(&detail), Ok(())),
            };
            let focus = focus.map(|f| f.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|f| !f.is_empty());
            let marker = marker.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
            let recorded = update_policy(&dir, sid, |p| policy::wrapped(p, o, line, focus, marker, now));
            if !recorded {
                std::process::exit(1);
            }
            if let Err(e) = wrote {
                eprintln!("keepwarm: {e}");
                std::process::exit(1);
            }
        }
        KeepwarmCmd::Compacted { failed, trigger } => {
            let failed = failed.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
            let took = take_policy(&dir, sid, |p| match trigger {
                Some(_) => match policy::relay_wrap_on_record(p) {
                    Some(why) => Err(why),
                    None => {
                        policy::boundary_compacted(p, failed, now);
                        Ok(())
                    }
                },
                None => {
                    policy::compacted(p, failed, now);
                    Ok(())
                }
            });
            match took {
                Some(Ok(())) => {}
                Some(Err(why)) => {
                    eprintln!("keepwarm: {why}, so its compaction is the relay's; the boundary compaction is not recorded");
                    std::process::exit(1);
                }
                None => std::process::exit(1),
            }
        }
        KeepwarmCmd::Resumed { failed, trigger } => {
            let failed = failed.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
            let boundary = trigger.is_some();
            let recorded = update_policy(&dir, sid, |p| match &failed {
                // A boundary resume that did not go in counts no relay, and
                // there is no relay wrap to stop.
                Some(_) if boundary => {}
                Some(why) => policy::resume_failed(p, why),
                None if boundary => {
                    policy::boundary_resumed(p, now);
                }
                // Only a relay wrap that compacted resumes: a report that
                // lands after the operator's prompt changes nothing.
                None if p.relay && p.compacted_at.is_some() && p.stopped.is_none() => policy::resumed(p, now),
                None => {}
            });
            if !recorded {
                std::process::exit(1);
            }
        }
        KeepwarmCmd::Drained => {
            if !update_policy(&dir, sid, |p| policy::drained(p, now)) {
                std::process::exit(1);
            }
        }
        KeepwarmCmd::Goal { state } => {
            if !update_policy(&dir, sid, |p| policy::goal(p, state == "active")) {
                std::process::exit(1);
            }
        }
        KeepwarmCmd::Stopped { reason, detail } => {
            let mut took = false;
            if !update_policy(&dir, sid, |p| took = policy::stopped_by_agent(p, &reason, &detail)) {
                std::process::exit(1);
            }
            if !took {
                eprintln!("keepwarm: no relay has run since the operator's last prompt; nothing to stop");
            }
        }
        KeepwarmCmd::WrapId { record: Some(id) } => {
            let mut took = false;
            if !update_policy(&dir, sid, |p| took = policy::record_wrap_id(p, id.trim())) {
                std::process::exit(1);
            }
            if !took {
                eprintln!("keepwarm: {id:?} is no wrap id (8 to 64 letters, digits and -); nothing recorded");
                std::process::exit(2);
            }
        }
        KeepwarmCmd::WrapId { record: None } => match read_ledger(&dir, sid).and_then(|l| l.policy.last_wrap_id) {
            Some(id) => println!("{id}"),
            None => std::process::exit(1),
        },
        KeepwarmCmd::Status(args) => status(&ident, args.json),
    }
}

/// `attend keepwarm status` for the session `ident` names. With `json`, one
/// object: the ledger's exit and boundary, for the attend mod, so a process
/// that resumes a session learns of an exit an earlier one recorded and
/// resets it at the operator's prompt; and the card's cache verdict, relay
/// count and the operator's last prompt, for a supervisor.
///
/// References:
/// - ADR-418 item 9: an operator prompt disarms the exit.
/// - ADR-418 item 14, item 4: the ledger records each boundary's calls.
/// - ADR-419 item 2: the cache with its next tick, the relay count, and the
///   operator's last prompt.
fn status(ident: &attend_presence::session::SessionIdentity, json: bool) {
    let sid = ident.session_id.as_str();
    let dir = keepwarm_dir();
    let now = sensor_trait::epoch_secs();
    if json {
        let ledger = read_ledger(&dir, sid);
        let state = State::resume(ledger.clone());
        let mut doc = serde_json::json!({ "exit": armed::view(&state), "boundary": armed::boundary(&state.policy) });
        if let (Some(doc), serde_json::Value::Object(more)) = (doc.as_object_mut(), status_fields(ident, ledger.as_ref(), &dir, now)) {
            doc.extend(more);
        }
        println!("{doc}");
        return;
    }
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
        let settings = mod_settings(&ident.origin_path, SETTINGS_TIMEOUT);
        if is_mod_host {
            println!("{}", mod_setting_line(settings.as_deref()));
        }
        let cfg = Config::from_settings(settings.as_deref());
        let state = card_state(&reading, arm.as_ref(), ledger.as_ref());
        for line in policy::card(&state, &reading, &cfg, &site(ident), now) {
            println!("{line}");
        }
    }
}

/// The `cache`, `relays` and `operator_prompt_at` keys of `status --json`,
/// from what the card reads: the transcript through `ways context`, the arm
/// file, the ledger, and the session's project settings. A value that
/// cannot be read is null: the cache's without a transcript reading, the
/// count, stop and prompt without a ledger, and the cap and relay mode
/// without the settings.
fn status_fields(ident: &attend_presence::session::SessionIdentity, ledger: Option<&sensor_keepwarm::Ledger>, dir: &Path, now: u64) -> serde_json::Value {
    let sid = ident.session_id.as_str();
    let arm = read_arm(dir, sid);
    let settings = policy_settings(dir, sid, &ident.origin_path, now, SETTINGS_TIMEOUT);
    let cfg = Config::from_settings(settings.as_deref());
    let cache = read_context(sid, &ident.origin_path).map(|reading| {
        let s = card_state(&reading, arm.as_ref(), ledger);
        let mut site = site(ident);
        site.handoff = s.policy.exit.as_ref().filter(|e| e.armed()).and_then(|e| read_handoff_at(e.path.clone()));
        serde_json::json!({
            "state": sensor_keepwarm::cache_state(&s, now),
            "last_request_at": (s.last_seen_at > 0).then_some(s.last_seen_at),
            "lapses_at": sensor_keepwarm::lapses_at(&s),
            "next_tick_at": policy::next_tick_at(&s, &reading, arm.as_ref(), &cfg, &site, now),
        })
    });
    let p = ledger.map(|l| &l.policy);
    serde_json::json!({
        "cache": cache.unwrap_or_else(|| serde_json::json!({ "state": "unknown", "last_request_at": null, "lapses_at": null, "next_tick_at": null })),
        "relays": {
            "count": p.map(|p| p.relays),
            "max": cfg.settings_ok.then_some(cfg.max_relays),
            "relay": cfg.settings_ok.then_some(cfg.relay),
            "stopped": p.and_then(policy::relay_stopped),
        },
        "operator_prompt_at": p.and_then(|p| p.operator_at),
    })
}

/// The session `id` names, for `status --session` from any shell, resolved
/// as `attend sensor poll --session` resolves it: exit 2 for an id no
/// session could have, and exit 3 when no session record names it, since
/// its project, where its transcript is read, is unknown.
fn named(id: &str, json: bool) -> attend_presence::session::SessionIdentity {
    if !agent_identity::signal::is_tag_value(id) {
        let why = format!("{id:?} is not a session id");
        crate::cmd::refuse_usage(&format!("keepwarm status: {why}"), &why, json)
    }
    let Some(origin_path) = attend_presence::session::origin_path(id) else {
        let why = format!("no record of session {id}; its project is unknown");
        if json {
            crate::cmd::print_json(&serde_json::json!({ "error": why }));
        }
        eprintln!("keepwarm status: {why}");
        std::process::exit(agent_settings::exit::REJECTED);
    };
    attend_presence::session::SessionIdentity { session_id: id.to_string(), origin_path, session_resolved: true, origin_resolved: true, claude_pid: None }
}

/// Writes the wrap's reply, read on stdin, to `path` (ADR-414 item 6): the
/// mod hands it over rather than writing it, so the path is checked again
/// now, after a turn in which the model knew it, and written through a
/// temp file renamed over it. The card's line for the outcome, and whether
/// the write held.
fn saved_outcome(o: WrapOutcome, detail: &str, path: &str, sid: &str) -> (String, Result<(), String>) {
    use std::io::Read;
    let mut reply = String::new();
    let wrote = std::io::stdin()
        .take(MAX_REPLY_BYTES + 1)
        .read_to_string(&mut reply)
        .map_err(|e| e.to_string())
        .and_then(|n| if n as u64 > MAX_REPLY_BYTES { Err(format!("the reply is over {} bytes", MAX_REPLY_BYTES)) } else { Ok(n) })
        .and_then(|_| policy::write_wrap(&ways_core::paths::state_root(), sid, Path::new(path), &reply))
        .map_err(|e| format!("could not write {path}: {e}"));
    let line = match (o, &wrote) {
        (WrapOutcome::File, Ok(())) => o.describe(path),
        (WrapOutcome::File, Err(e)) => WrapOutcome::Unfinished.describe(e),
        (_, Ok(())) => o.describe(&format!("{detail}; saved to {path}")),
        (_, Err(e)) => o.describe(&format!("{detail}; the file could not be written either: {e}")),
    };
    (line, wrote)
}

/// Apply `f` to the policy's marks in `sid`'s ledger, a fresh ledger where
/// there is none, under the session's lock. False, with the reason on
/// stderr, when it cannot be written.
fn update_policy(dir: &Path, sid: &str, f: impl FnOnce(&mut Policy)) -> bool {
    take_policy(dir, sid, |p| {
        f(p);
        Ok(())
    })
    .is_some()
}

/// [`update_policy`] for a mark that can be refused: the ledger is written
/// only when `f` takes the mark. Whether it took it or why it refused, or
/// `None`, with the reason on stderr, when the ledger cannot be written.
fn take_policy(dir: &Path, sid: &str, f: impl FnOnce(&mut Policy) -> Result<(), String>) -> Option<Result<(), String>> {
    let _lock = lock_session(dir, sid).ok();
    let mut ledger = read_ledger(dir, sid).unwrap_or_default();
    if let Err(why) = f(&mut ledger.policy) {
        return Some(Err(why));
    }
    ledger.updated_at = sensor_trait::epoch_secs();
    match write_ledger(dir, sid, &ledger) {
        Ok(()) => Some(Ok(())),
        Err(e) => {
            eprintln!("keepwarm: could not write the ledger: {e}");
            None
        }
    }
}

/// The effective `mods.attend` entry for `project`, as the attend mod reads
/// it: `ways settings list mods.attend --json --effective`. `None` when ways
/// cannot answer within `timeout`, in which case it is killed.
fn mod_settings(project: &str, timeout: std::time::Duration) -> Option<String> {
    use std::io::Read;
    // Through the brief's spawn lock: the brief runs this beside its other
    // probes, and on macOS a pipe made during a sibling's spawn leaks into it.
    let mut child = sensor_keepwarm::spawn_probe(
        std::process::Command::new("ways")
            .args(["settings", "list", "mods.attend", "--json", "--effective", "--project", project])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null()),
    )
    .ok()?;
    let deadline = std::time::Instant::now() + timeout;
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

/// The card's line for the attend mod's `mods.attend.keepwarm` setting, read
/// from `ways settings list mods.attend --json --effective`: its value and
/// the layer that set it. With `off` the mod never runs the check, so
/// nothing arms or wakes in this session whatever the line above says.
fn mod_setting_line(settings: Option<&str>) -> String {
    let key = settings
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|doc| doc.get("keys").and_then(|k| k.get("mods.attend.keepwarm")).cloned());
    let Some(key) = key else {
        return "mod setting mods.attend.keepwarm unknown (ways settings could not be read)".to_string();
    };
    // The mod reads anything but `off` as on (plugins/attend/hooks/lib.ts).
    let value = if key.get("value").and_then(|v| v.as_str()) == Some("off") { "off" } else { "on" };
    let from = match (key.get("layer").and_then(|v| v.as_str()), key.get("file").and_then(|v| v.as_str())) {
        (_, Some(file)) => format!(" (from {file})"),
        (Some(layer), None) => format!(" ({layer})"),
        (None, None) => String::new(),
    };
    let effect = if value == "off" { "; the mod never runs the keepwarm check in this session" } else { "" };
    format!("mod setting mods.attend.keepwarm {value}{from}{effect}")
}

/// Whether the effective `mods.attend` settings, as
/// `ways settings list mods.attend --json --effective` prints them, set
/// `mods.attend.keepwarm` to `off`. Settings ways could not give read as on,
/// as the attend mod reads them (plugins/attend/hooks/lib.ts).
fn keepwarm_setting_off(settings: Option<&str>) -> bool {
    settings
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|doc| doc.get("keys")?.get("mods.attend.keepwarm")?.get("value")?.as_str().map(|v| v == "off"))
        .unwrap_or(false)
}

/// What `check --json` prints when nothing was judged: why, and when to ask
/// again.
fn nothing(reason: &str, next_at: Option<u64>) -> serde_json::Value {
    serde_json::json!({
        "action": "none", "text": null, "next_at": next_at, "count": 0, "max": 0,
        "wraps_at": null, "reason": reason, "policy": false, "state": "off",
        "wraps_at_hm": null, "wrapped_at_hm": null, "wrap": null, "retry": false, "idle": null,
        "after_count": 0, "after_max": 0, "compacts_at": null, "compacts_at_hm": null, "compacted_at_hm": null,
        "relay": false, "relay_count": 0, "max_relays": 0, "relay_at_tokens": null, "resume": null, "relay_stop": null,
        "send_handoff": false, "wrap_id": null, "exit": null,
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
        handoff: None,
    }
}

/// The repository's handoff file under `project`, as the relay judges it:
/// its path, the wrap id on its first line, the compaction instructions
/// built from it, and its text. `None` outside a repository or where the
/// file cannot be read.
///
/// References:
/// - ADR-417 item 4: the relay compacts with the handoff only when it is fresh.
fn read_handoff(project: &str) -> Option<Handoff> {
    let root = ways_core::handoff::repo_root(Path::new(project))?;
    read_handoff_at(ways_core::handoff::path(&root))
}

/// The handoff file at `path`, read with no git: an armed exit's tick reads
/// the file its judgment named.
fn read_handoff_at(path: PathBuf) -> Option<Handoff> {
    use ways_core::handoff;
    let text = std::fs::read_to_string(&path).ok()?;
    Some(Handoff {
        wrap_id: handoff::wrap_id_of(&text).map(str::to_string),
        instructions: handoff::instructions(&text, &path),
        text: text.trim_end().to_string(),
        path,
    })
}

/// The mods.attend settings for the policy check: a read kept beside the
/// ledger for a few minutes, so a listening session that asks every minute
/// does not run ways every minute, unless the user's or the project's
/// settings file changed since. `None` when ways cannot answer within
/// `timeout`.
pub(super) fn policy_settings(dir: &Path, sid: &str, project: &str, now: u64, timeout: std::time::Duration) -> Option<String> {
    let path = dir.join(format!("{sid}.settings.json"));
    let stamps = settings_stamps(project);
    if let Some(kept) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        let at = kept.get("at").and_then(|v| v.as_u64()).unwrap_or(0);
        let same = kept.get("project").and_then(|v| v.as_str()) == Some(project) && kept.get("files") == Some(&stamps);
        if same && now.saturating_sub(at) < SETTINGS_KEPT_SECS {
            if let Some(doc) = kept.get("doc").and_then(|v| v.as_str()) {
                return Some(doc.to_string());
            }
        }
    }
    let doc = mod_settings(project, timeout)?;
    let kept = serde_json::json!({ "at": now, "project": project, "files": stamps, "doc": doc });
    std::fs::create_dir_all(dir).ok();
    std::fs::write(&path, kept.to_string()).ok();
    Some(doc)
}

/// Each settings file the mods.attend entry resolves through (the user's,
/// the active target's, the project's) with its modification time in
/// milliseconds, or null where it is absent: a kept read is reused only
/// while these are unchanged, so a file written, created or deleted since
/// is seen at once. Taken before ways reads them.
fn settings_stamps(project: &str) -> serde_json::Value {
    let current = ways_core::paths::current_config_dir();
    let target = ways_core::paths::target_config_root(&current).join("config.yaml");
    let root = ways_core::settings::project_root(Path::new(project)).unwrap_or_else(|| PathBuf::from(project));
    let mut files = vec![ways_core::paths::user_config(), target, ways_core::settings::project_file(&root)];
    files.extend(ways_core::settings::layers(&root).into_iter().filter_map(|l| l.path));
    files.sort();
    files.dedup();
    let stamp = |f: &PathBuf| {
        std::fs::metadata(f)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
    };
    serde_json::Value::Array(files.iter().map(|f| serde_json::json!([f.to_string_lossy(), stamp(f)])).collect())
}

/// How long a session's kept settings read outlives its ledger's last
/// write before a check removes it.
const STALE_SECS: u64 = 7 * 24 * 3600;

/// Removes other sessions' kept settings reads and Stop records of their
/// background work whose ledger is gone or has not been written for
/// [`STALE_SECS`]: nothing else removes them.
fn prune_stale(dir: &Path, sid: &str, now: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // Lock files stay: one may be held, and a writer that then locked a
        // new file would hold a second lock beside the first. They are empty.
        let Some(other) = name.strip_suffix(".settings.json").or_else(|| name.strip_suffix(".background.json")) else { continue };
        if other == sid {
            continue;
        }
        let written = std::fs::metadata(sensor_keepwarm::ledger_path(dir, other))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        if written.is_none_or(|t| now.saturating_sub(t) > STALE_SECS) {
            std::fs::remove_file(e.path()).ok();
        }
    }
}

/// `attend keepwarm check`: the sensor's judgment, once. Prints the
/// `keepwarm:` line when a wake is due and nothing otherwise, and exits 0
/// either way, so a polling host reads stdout alone. Why nothing printed
/// goes to stderr. With `--policy` the keepalive policy judges (ADR-414);
/// with `--json` the decision prints as one object, whatever it is.
///
/// It judges nothing where `mods.attend.keepwarm` is `off`, the switch the
/// attend mod reads too. A `keepwarm.yaml` that sets `enabled: false`, the
/// opt-out from when a shipped sensor file switched keepwarm on, is
/// honoured as `off` for one release ([`attend_config::sensors::legacy_keepwarm`]).
///
/// References:
/// - ADR-182, amendment of 2026-11-23: the config key is the switch.
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
    let now = sensor_trait::epoch_secs();
    let dir = keepwarm_dir();
    let settings = policy_settings(&dir, sid, &ident.origin_path, now, SETTINGS_TIMEOUT);
    if keepwarm_setting_off(settings.as_deref()) {
        return quiet("mods.attend.keepwarm is off", None);
    }
    // #1316 grace: remove after the next release.
    if let Some((paths, true)) = attend_config::sensors::legacy_keepwarm(std::path::Path::new(&ident.origin_path)) {
        return quiet(&attend_config::sensors::legacy_keepwarm_note(&paths, true), None);
    }
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

    let gate = Gate::parse(&args.autocompact);
    let mut pcfg = Config::from_settings(settings.as_deref());
    let names = |list: &Vec<String>| list.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    pcfg.session_skills = match (&args.skills, args.skills_unread) {
        (_, true) => SessionSkills::Unread,
        (Some(list), false) if args.skills_partial => SessionSkills::Partial(names(list)),
        (Some(list), false) => SessionSkills::Listed(names(list)),
        (None, false) => SessionSkills::NotChecked,
    };
    // A host that passes no listing is a mod from before the flag, which
    // compacts as soon as the wrap answers and knows no compact action: no
    // stretch after the wrap for it.
    if pcfg.session_skills == SessionSkills::NotChecked {
        pcfg.after_max = 0;
    }
    if let (Some(tokens), Some(window)) = (args.fill, args.window) {
        pcfg.fill = Some(Fill { tokens, window, native_at: args.native_at });
    }
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
    let mut site = site(ident);
    prune_stale(&dir, sid, now);
    // One read-step-write at a time per session: two processes on one
    // session id never both send a keepalive or a wrap.
    let _lock = lock_session(&dir, sid).ok();
    let ledger = read_ledger(&dir, sid);
    // The handoff is read only while a relay wrap's marker waits to be
    // judged or compacted on, so a check every minute runs no git.
    if ledger.as_ref().is_some_and(|l| l.policy.wants_handoff()) {
        site.handoff = read_handoff(&ident.origin_path);
    }
    // An armed exit re-reads its handoff's first line at each tick, and
    // counts as pending the host's agents, the shells and watchers the last
    // Stop hook listed where the host named none, and the threads awaiting
    // a reply.
    //
    // References:
    // - ADR-418 item 9: each tick checks the recorded wrap id against the
    //   handoff.
    if let Some(e) = ledger.as_ref().and_then(|l| l.policy.exit.as_ref()).filter(|e| e.armed()) {
        if site.handoff.is_none() {
            site.handoff = read_handoff_at(e.path.clone());
        }
        pcfg.pending = args.pending.unwrap_or_default().with(sensor_keepwarm::background::read(&dir, sid).as_ref());
        pcfg.pending.awaiting = crate::verbs::awaiting::count().count as u32;
    }
    let mut state = State::resume(ledger);
    let arm = read_arm(&dir, sid);
    let (mut out, decision) = policy::step(&mut state, &reading, arm.as_ref(), &pcfg, gate, &site, now);
    state.policy.old_host = pcfg.session_skills == SessionSkills::NotChecked;
    // The tick that finishes an armed exit marks a listening session
    // finished in the same process, so peers see it before the cache has
    // lapsed. A session that is not listening gets no mark, so the
    // operator's prompt has none to clear by listening.
    //
    // References:
    // - ADR-418 item 12: a stopped session is marked finished.
    if decision.action == Action::Finish && crate::verbs::listen::status().listening {
        if let Err(e) = crate::verbs::listen::finish() {
            eprintln!("keepwarm: could not mark the session finished: {e}");
        }
    }
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
        format!(r#"{{"view":"effective","keys":{{"mods.attend.keepwarm":{{"value":"{value}","default":"on","layer":"{layer}","file":{file}}}}}}}"#)
    }

    /// The clean-slate test's finding 6: the card names the mod's setting
    /// apart from whether keepwarm is armed.
    #[test]
    fn the_mod_setting_line_names_the_value_and_where_it_came_from() {
        assert_eq!(mod_setting_line(Some(&settings("on", "default", None))), "mod setting mods.attend.keepwarm on (default)");
        assert_eq!(
            mod_setting_line(Some(&settings("off", "project", Some("/p/.claude/ways.yaml")))),
            "mod setting mods.attend.keepwarm off (from /p/.claude/ways.yaml); the mod never runs the keepwarm check in this session"
        );
        assert_eq!(
            mod_setting_line(None),
            "mod setting mods.attend.keepwarm unknown (ways settings could not be read)"
        );
        assert_eq!(
            mod_setting_line(Some("not json")),
            "mod setting mods.attend.keepwarm unknown (ways settings could not be read)"
        );
    }
}
