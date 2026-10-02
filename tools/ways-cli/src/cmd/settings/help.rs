//! The schema's long text (ADR-503 §10).

use super::*;

// ── help ───────────────────────────────────────────────────────

/// The long help for a key or section: the text the settings TUI's detail
/// pane shows (ADR-503 §10).
pub fn help(topic: Option<&str>) -> Out {
    let reg = registry();
    let Some(topic) = topic else {
        println!("ways settings: the settings of ways and its agent, read and written through their files.\n");
        println!("  get <key>            the value in effect (--json: with its layer, default and file)");
        println!("  set <key> <value>    write it (--project <dir> for a project's ways.yaml)");
        println!("  unset <key>          remove it, so the layer below applies");
        println!("  list [prefix]        key=value lines (--json: stored, or --effective)");
        println!("  emit [prefix]        the canonical fragment (--effective: the values in effect)");
        println!("  apply                write a settings object from stdin or --file; answers in JSON");
        println!("  lint                 check the files; exit 3 with findings");
        println!("  fix <section>        write a section's canonical fragment");
        println!("  help <key|section>   what a key or section does\n");
        println!("exit codes: 0 done, 2 usage or unknown key, 3 rejected, 4 overridden by a higher layer, 5 write failed\n");
        println!("sections:");
        let width = reg.sections().map(|(_, s)| s.name.len()).max().unwrap_or(0);
        for (_, s) in reg.sections() {
            println!("  {:<width$}  {}", s.name, s.doc);
        }
        return Ok(());
    };
    if let Some(b) = reg.lookup(topic).or_else(|| {
        // A pattern key by its own name, `ways.project.*`.
        reg.keys().find(|(_, k)| k.name == topic).map(|(schema, spec)| Bound { schema, spec, bound: vec![] })
    }) {
        let s = b.spec;
        println!("{}", if b.bound.is_empty() { s.name.to_string() } else { b.name() });
        println!("  {}", s.doc);
        println!("  type:    {}", s.kind.describe());
        if let Some(d) = s.default_for(&b.bound) {
            println!("  default: {}", plain(Some(&d)));
        }
        println!("  scope:   {}", s.scope.as_str());
        if s.computed.is_none() {
            println!("  file:    {} key {}", s.file, s.path.join("."));
        }
        if !s.long.is_empty() {
            println!();
            println!("{}", wrap(s.long, 76));
        }
        return Ok(());
    }
    // A section, with the sections under it: `gate` covers `gate.mode`.
    let sections: Vec<_> =
        reg.sections().filter(|(_, s)| s.name == topic || agent_settings::registry::under(s.name, topic)).collect();
    if !sections.is_empty() {
        for (schema, sec) in sections {
            println!("{}: {}", sec.name, sec.doc);
            for k in schema.keys_of_section(sec.name) {
                println!("  {:<40} {}", k.name, k.doc);
            }
        }
        return Ok(());
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

