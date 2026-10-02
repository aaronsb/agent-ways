//! `attend config show` and `lint`, over attend's schema (ADR-503 §13).
//! Named `config_cmd` to avoid shadowing the `config` alias.
//!
//! `show` prints each key in effect as `key=value`, the shape `ways settings
//! list attend` prints; `lint` prints each finding with its file, line and
//! section, and exits 3 when there is one. Editing is `ways settings`'s.

use std::path::Path;

use agent_settings::load::resolve;
use agent_settings::Registry;
use serde_yaml::Value;

use crate::config;

/// A value as `key=value` shows it: text bare, anything else as YAML flow.
fn plain(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Sequence(items)) => format!("[{}]", items.iter().map(|i| plain(Some(i))).collect::<Vec<_>>().join(", ")),
        Some(other) => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
    }
}

pub(crate) fn show(working_dir: &str) {
    let layers = config::layers(Path::new(working_dir));
    for l in layers.iter().filter(|l| l.present) {
        for f in &l.findings {
            eprintln!("{}", f.diagnostic("attend"));
        }
    }
    let reg = Registry::new(vec![&config::SCHEMA]);
    for b in reg.concrete("", &layers) {
        let r = resolve(b.spec, &b.bound, &layers);
        if r.value.is_some() {
            println!("{}={}", r.name, plain(r.value.as_ref()));
        }
    }
}

/// Print every finding in the user and project files; 3 when there is one.
pub(crate) fn lint(working_dir: &str) -> i32 {
    let layers = config::layers(Path::new(working_dir));
    let mut n = 0;
    for l in layers.iter().filter(|l| l.present) {
        for f in &l.findings {
            println!("{f}");
            n += 1;
        }
    }
    if n == 0 {
        return 0;
    }
    eprintln!(
        "attend config lint: {n} finding{}; `ways settings fix <section>` repairs what a section's findings point at (add --project <dir> for a project's file)",
        if n == 1 { "" } else { "s" }
    );
    agent_settings::exit::REJECTED
}
