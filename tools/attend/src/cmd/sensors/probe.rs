// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend sensor probe`: two calls of an mcp sensor's tool, compared, and
//! the operator's confirmation, written as the sensor's probe record.

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
    let mut tool = sensors::mcp_source(&h, &cfg, &focus.working_dir).unwrap_or_else(|why| refuse(why));
    let calls = tool.call().and_then(|first| Ok((first, tool.call()?)));
    // The server ends with the last sensor holding it, before any exit.
    drop(tool);
    let (first, second) = calls.unwrap_or_else(|why| refuse(format!("the call failed, so no record was written: {why}")));
    let compared = sensors::hosted::kept(spec, &first, &second);
    let confirmed = compared.is_ok() && (a.confirm || (!a.json && ask(h.name(), &first)));
    let at = agent_fmt::when::now_utc_iso();
    let mut record = serde_yaml::Mapping::new();
    record.insert("server".into(), m.server.label().into());
    record.insert("tool".into(), m.tool.clone().into());
    record.insert("args".into(), serde_yaml::Value::Mapping(m.args.clone()));
    record.insert("calls_match".into(), compared.is_ok().into());
    record.insert("confirmed".into(), confirmed.into());
    record.insert("at".into(), at.clone().into());
    let file = write_probe(h.name(), record).unwrap_or_else(|why| refuse(why));
    let holds = compared.is_ok() && confirmed;
    if a.json {
        crate::cmd::print_json(&json!({
            "sensor": h.name(),
            "server": m.server.label(),
            "tool": m.tool,
            "calls_match": compared.is_ok(),
            "difference": compared.as_ref().err(),
            "confirmed": confirmed,
            "at": at,
            "holds": holds,
            "file": file.display().to_string(),
        }));
    } else {
        match &compared {
            Ok(()) => println!("{}: the second call returned every item of the first", h.name()),
            Err(why) => println!("{}: the calls differ: {why}", h.name()),
        }
        let then = match (compared.is_ok(), confirmed, spec.enabled) {
            (false, _, _) => "the sensor stays disabled: reading the tool may change it".to_string(),
            (true, false, _) => format!("the sensor stays disabled until you confirm; check in another client that the items are untouched, then run `attend sensor probe {} --confirm`", h.name()),
            (true, true, true) => "the sensor runs from the host's next start".to_string(),
            (true, true, false) => "the record holds; set enabled: true to run it".to_string(),
        };
        println!("{}: probe recorded in {}; {then}", h.name(), tilde(&file.display().to_string()));
    }
    if !holds {
        std::process::exit(agent_settings::exit::REJECTED);
    }
}

/// Ask on the terminal whether the items the first call returned are still
/// unread or otherwise untouched; no terminal is no.
fn ask(name: &str, first: &Value) -> bool {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    let shown: String = first.to_string().chars().take(400).collect();
    eprintln!("{name}: the first call returned: {shown}");
    eprint!("{name}: in another client, are those items still unread or otherwise untouched? [y/N] ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok();
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Write `record` as `probe` in the user file of the sensor `name`, which
/// is created with that field alone when there is none. The rest of the
/// file is left as written, and the edit is refused when it would make the
/// sensor invalid.
fn write_probe(name: &str, record: serde_yaml::Mapping) -> Result<std::path::PathBuf, String> {
    let path = attend_config::sensors::user_dir().join(format!("{name}.yaml"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let mut doc = agent_settings::yaml_edit::Doc::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    doc.set(&["probe".to_string()], &serde_yaml::Value::Mapping(record)).map_err(|e| format!("{}: {e}", path.display()))?;
    doc.verify().map_err(|e| format!("{}: {e}", path.display()))?;
    let out = doc.text();
    attend_config::sensors::check_edit(&path, &out)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    agent_settings::writer::write_atomic(&path, out).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(path)
}
