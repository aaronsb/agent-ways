//! The schema's long text (ADR-503 §10). `help` prints it; the settings
//! screens show the same text in their help overlay and detail pane.

use super::*;
use std::fmt::Write as _;

// ── help ───────────────────────────────────────────────────────

/// The long help for a key or section: the text the settings TUI's detail
/// pane and help overlay show (ADR-503 §10).
pub fn help(topic: Option<&str>) -> Out {
    print!("{}", help_text(topic)?);
    Ok(())
}

/// What `ways settings help [topic]` prints.
pub fn help_text(topic: Option<&str>) -> Result<String, Failure> {
    let reg = registry();
    let mut out = String::new();
    let Some(topic) = topic else {
        let _ = writeln!(out, "ways settings: the settings of ways and its agent, read and written through their files.\n");
        let _ = writeln!(out, "  (no verb)            the settings screens on a terminal; `list` in a pipe");
        let _ = writeln!(out, "  <tab>                the screens, opened on ways, matching, gate, install, attend, sensors or theme");
        let _ = writeln!(out, "  get <key>            the value in effect (--json: with its layer, default and file)");
        let _ = writeln!(out, "  set <key> <value>    write it (--project <dir> for a project's ways.yaml)");
        let _ = writeln!(out, "  unset <key>          remove it, so the layer below applies");
        let _ = writeln!(out, "  list [prefix]        key=value lines (--json: stored, or --effective)");
        let _ = writeln!(out, "  emit [prefix]        the canonical fragment (--effective: the values in effect)");
        let _ = writeln!(out, "  apply                write a settings object from stdin or --file; answers in JSON");
        let _ = writeln!(out, "  lint                 check the files; exit 3 with findings");
        let _ = writeln!(out, "  fix <section>        repair what the section's findings point at; a switch stays off");
        let _ = writeln!(out, "  help <key|section>   what a key or section does\n");
        let _ = writeln!(out, "exit codes: 0 done, 2 usage or unknown key, 3 rejected, 4 overridden by a higher layer, 5 write failed\n");
        let _ = writeln!(out, "sections:");
        let width = reg.sections().map(|(_, s)| s.name.len()).max().unwrap_or(0);
        for (_, s) in reg.sections() {
            let _ = writeln!(out, "  {:<width$}  {}", s.name, s.doc);
        }
        return Ok(out);
    };
    if let Some(b) = reg.lookup(topic).or_else(|| {
        // A pattern key by its own name, `ways.project.*`.
        reg.keys().find(|(_, k)| k.name == topic).map(|(schema, spec)| Bound { schema, spec, bound: vec![] })
    }) {
        let s = b.spec;
        let _ = writeln!(out, "{}", if b.bound.is_empty() { s.name.to_string() } else { b.name() });
        let _ = writeln!(out, "  {}", s.doc);
        // The choices of a computed choice come from the files.
        let layers = match s.kind {
            Kind::ChoiceOf { .. } => live_layers(&project_dir(None)),
            _ => Vec::new(),
        };
        let _ = writeln!(out, "  type:    {}", s.kind.describe(&layers));
        if let Some(d) = s.default_for(&b.bound) {
            let _ = writeln!(out, "  default: {}", plain(Some(&d)));
        }
        let _ = writeln!(out, "  scope:   {}", s.scope.as_str());
        if s.computed.is_none() {
            let _ = writeln!(out, "  file:    {} key {}", s.file, s.path.join("."));
        }
        if !s.long.is_empty() {
            let _ = writeln!(out);
            let _ = writeln!(out, "{}", wrap(s.long, 76));
        }
        return Ok(out);
    }
    // A tab whose keys sit under a longer prefix: `sensors` is attend's.
    let topic = super::tui::build::tab_named(topic).map_or(topic, |t| t.prefix);
    // A section, with the sections under it: `gate` covers `gate.mode`.
    let sections: Vec<_> =
        reg.sections().filter(|(_, s)| s.name == topic || agent_settings::registry::under(s.name, topic)).collect();
    if !sections.is_empty() {
        for (schema, sec) in sections {
            let _ = writeln!(out, "{}: {}", sec.name, sec.doc);
            for k in schema.keys_of_section(sec.name) {
                let _ = writeln!(out, "  {:<40} {}", k.name, k.doc);
            }
        }
        return Ok(out);
    }
    Err(fail(exit::USAGE, format!("no key or section {topic}; `ways settings help` lists the sections")))
}

pub(super) fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push_str("  ");
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push_str("  ");
        out.push_str(&line);
    }
    out
}
