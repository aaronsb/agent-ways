// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend sensor probe`: two calls of an mcp sensor's tool, compared, and
//! the operator's confirmation, written as the sensor's probe record.
//!
//! The confirmation is a y/N answer read from a terminal. With no terminal
//! on stdin nothing is recorded as confirmed, so a caller that pipes its
//! input cannot answer. A caller that gives the probe a terminal of its
//! own, as `script` does, can still answer; what stands between such a
//! caller and the answer is the permission prompt on that command.
//!
//! The record goes in the sensor's user file, created with the records
//! alone when the sensor has none. A sensor no project file defines keeps
//! its record under `probe`. A sensor a project file defines keeps one
//! record per project under `probes`, keyed by the project's canonical
//! path, and the probe writes only its own project's entry, so same-named
//! sensors of two projects keep a record each. The file is checked before
//! the calls, so a record that could not be written costs no call.
//!
//! References:
//! - ADR-403 §4: the probe, its record, and where the record lives.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::{context_of, find, tilde};
use crate::cli::ProbeArgs;
use crate::sensors::{self, State};
use sensor_spec::Block;

pub(crate) fn probe(a: &ProbeArgs) {
    let (focus, cfg) = context_of(None, "probe", a.json);
    let h = find(sensors::catalog_for(&cfg, &focus.working_dir, None), &a.name, a.json);
    let refuse = |why: String| -> ! {
        if a.json {
            crate::cmd::print_json(&json!({ "sensor": a.name, "error": why }));
        } else {
            eprintln!("attend sensor probe: {why}");
        }
        std::process::exit(agent_settings::exit::REJECTED);
    };
    let Some(spec) = h.loaded.spec.as_ref().filter(|_| !matches!(h.state, State::Refused(_) | State::Invalid(_))) else {
        refuse(format!("{} is {}{}", h.name(), h.state.label(), h.state.detail().map(|d| format!(": {d}")).unwrap_or_default()))
    };
    let Block::Mcp(m) = &spec.block else {
        let why = format!("{} is a {} sensor; only an mcp sensor is probed", h.name(), spec.kind.as_str());
        crate::cmd::refuse_usage(&format!("attend sensor probe: {why}"), &why, a.json)
    };
    let project = h.in_project().then(|| sensors::project_key(&focus.working_dir));
    let key: Vec<String> = match &project {
        Some(p) => vec!["probes".into(), p.clone()],
        None => vec!["probe".into()],
    };
    let record = |binding: &str, calls_match: bool, confirmed: bool, at: &str| {
        let mut r = serde_yaml::Mapping::new();
        r.insert("server".into(), m.server.label().into());
        r.insert("binding".into(), binding.into());
        r.insert("tool".into(), m.tool.clone().into());
        r.insert("args".into(), serde_yaml::Value::Mapping(m.args.clone()));
        r.insert("calls_match".into(), calls_match.into());
        r.insert("confirmed".into(), confirmed.into());
        r.insert("at".into(), at.into());
        r
    };
    probe_file(h.name(), &key, record("", false, false, "")).unwrap_or_else(|why| refuse(format!("no call was made: {why}")));
    let mut tool = sensors::mcp_source(&h, &cfg, &focus.working_dir).unwrap_or_else(|why| refuse(why));
    let calls = tool.probe_call().and_then(|first| Ok((first, tool.probe_call()?.0)));
    // The server ends with the last sensor holding it, before any exit.
    drop(tool);
    let ((first, binding), second) = calls.unwrap_or_else(|why| refuse(format!("the call failed, so no record was written: {why}")));
    let compared = sensors::hosted::kept(spec, &first, &second);
    let terminal = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let confirmed = confirm(compared.is_ok(), terminal, || ask(h.name(), &first, std::io::stdin().lock(), std::io::stderr()));
    let at = agent_fmt::when::now_utc_iso();
    let (path, text) = probe_file(h.name(), &key, record(&binding, compared.is_ok(), confirmed, &at)).unwrap_or_else(|why| refuse(why));
    agent_settings::writer::write_atomic(&path, text).unwrap_or_else(|e| refuse(format!("cannot write {}: {e}", path.display())));
    let holds = compared.is_ok() && confirmed;
    let then = match (compared.is_ok(), confirmed, terminal, spec.enabled) {
        (false, _, _, _) => "the sensor stays disabled: reading the tool may change it".to_string(),
        (true, false, false, _) => format!(
            "the sensor stays disabled: the confirmation is answered at a terminal; run `attend sensor probe {0}` there, such as `! attend sensor probe {0}` in Claude Code",
            h.name()
        ),
        (true, false, true, _) => "the sensor stays disabled: the items were not confirmed untouched".to_string(),
        (true, true, _, true) => "the sensor runs from the host's next start".to_string(),
        (true, true, _, false) => "the record holds; set enabled: true to run it".to_string(),
    };
    if a.json {
        crate::cmd::print_json(&json!({
            "sensor": h.name(),
            "server": m.server.label(),
            "binding": binding,
            "project": project,
            "tool": m.tool,
            "calls_match": compared.is_ok(),
            "difference": compared.as_ref().err(),
            "confirmed": confirmed,
            "at": at,
            "holds": holds,
            "file": path.display().to_string(),
            "then": then,
        }));
    } else {
        match &compared {
            Ok(()) => println!("{}: the second call returned every item of the first", h.name()),
            Err(why) => println!("{}: the calls differ: {why}", h.name()),
        }
        println!("{}: probe recorded in {}; {then}", h.name(), tilde(&path.display().to_string()));
    }
    if !holds {
        std::process::exit(agent_settings::exit::REJECTED);
    }
}

/// Whether the record says confirmed: only when the calls matched and a
/// terminal is there to ask, and then only on a yes from `ask`, which is
/// not called otherwise.
fn confirm(calls_match: bool, terminal: bool, ask: impl FnOnce() -> bool) -> bool {
    calls_match && terminal && ask()
}

/// Ask on `out` whether the items the first call returned are still unread
/// or otherwise untouched, and read the answer from `input`: y or yes, in
/// any case, is a yes; anything else, an empty line or the end of the
/// input is a no.
fn ask(name: &str, first: &Value, mut input: impl std::io::BufRead, mut out: impl std::io::Write) -> bool {
    let shown: String = first.to_string().chars().take(400).collect();
    writeln!(out, "{name}: the first call returned: {shown}").ok();
    write!(out, "{name}: in another client, are those items still unread or otherwise untouched? [y/N] ").ok();
    out.flush().ok();
    let mut line = String::new();
    input.read_line(&mut line).ok();
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// The user file of the sensor `name` and its text with `record` at `key`,
/// `probe` or a project's entry under `probes`: the rest of the file as
/// written, or the records alone for a sensor with no user file. Refused
/// when the edit would make the sensor invalid.
fn probe_file(name: &str, key: &[String], record: serde_yaml::Mapping) -> Result<(PathBuf, String), String> {
    let path = attend_config::sensors::user_dir().join(format!("{name}.yaml"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let mut doc = agent_settings::yaml_edit::Doc::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    doc.set(key, &serde_yaml::Value::Mapping(record)).map_err(|e| format!("{}: {e}", path.display()))?;
    doc.verify().map_err(|e| format!("{}: {e}", path.display()))?;
    let out = doc.text();
    attend_config::sensors::check_edit(&path, &out)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    Ok((path, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(typed: &str) -> bool {
        let mut shown = Vec::new();
        let yes = ask("chat", &json!({"items": []}), typed.as_bytes(), &mut shown);
        assert!(String::from_utf8(shown).unwrap().ends_with("[y/N] "), "the question is asked");
        yes
    }

    #[test]
    fn only_y_or_yes_is_a_yes() {
        for typed in ["y\n", "yes\n", "Y\n", " YES \n", "y"] {
            assert!(answer(typed), "{typed:?}");
        }
        for typed in ["n\n", "no\n", "\n", "", "yep\n", "sure\n"] {
            assert!(!answer(typed), "{typed:?}");
        }
    }

    /// A failed comparison, or no terminal, records no confirmation and
    /// never asks.
    #[test]
    fn a_failed_comparison_never_asks() {
        assert!(!confirm(false, true, || panic!("asked after the calls differed")));
        assert!(!confirm(true, false, || panic!("asked with no terminal")));
        assert!(confirm(true, true, || true));
        assert!(!confirm(true, true, || false));
    }
}
