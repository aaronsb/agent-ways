//! `ways settings show` — a human-readable view of the fragment store (ADR-147).
//!
//! Read-only, writes nothing. Two modes:
//!
//! - **inventory** (`ways settings show`) — every fragment grouped by scope, one
//!   line per top-level key (key · value · one-line rationale), with a per-scope
//!   footer: where it projects, the lint verdict, and how many keys differ from
//!   the live `settings.json`.
//! - **detail** (`ways settings show <key>`) — a manpage-shaped page for one key:
//!   NAME (scope · type), VALUE (+ source fragment), SCHEMA (the official
//!   description), WHY (the fragment's rationale body), STATUS (live/pending).
//!
//! It *composes* the pieces the other verbs already own — the loader
//! ([`super::fragment`]), the schema ([`super::schema_doc`]), the compile merge +
//! provenance ([`super::compile`]), and the live `settings.json` — rather than
//! re-deriving any of them.

use super::compile::{self, Provenance};
use super::fragment::{load_dir, Fragment, Scope};
use super::project::reconciler_owned;
use super::schema_doc;
use anyhow::Result;
use serde_json::{Map, Value};
use std::path::Path;

pub fn run(store: &Path, key: Option<String>) -> Result<()> {
    if !store.is_dir() {
        println!("No settings store at {} yet.", store.display());
        println!("Author one with: ways settings new <key> --dir {}", store.display());
        return Ok(());
    }
    let frags = load_dir(store)?;
    match key {
        Some(k) => show_key(store, &frags, &k),
        None => show_inventory(store, &frags),
    }
}

// ── inventory ──────────────────────────────────────────────────

fn show_inventory(store: &Path, frags: &[Fragment]) -> Result<()> {
    println!("{}", store.display());
    if frags.is_empty() {
        println!("\n  (empty — no fragments yet; `ways settings new <key>` to start)");
        return Ok(());
    }

    let schema = schema_doc::active();
    let findings = super::lint::check(frags, schema);
    let has_errors = findings.iter().any(|f| f.severity == super::lint::Severity::Error);
    let lint_verdict = lint_summary(&findings);

    for scope in [Scope::User, Scope::Project, Scope::Managed] {
        let scoped: Vec<&Fragment> = frags.iter().filter(|f| f.scope == scope).collect();
        if scoped.is_empty() {
            continue;
        }
        println!("\n{} ({} fragment{})", scope.as_str().to_uppercase(), scoped.len(), plural(scoped.len()));

        // Widest top-level key, for column alignment.
        let width = scoped
            .iter()
            .flat_map(|f| top_keys(f))
            .map(|k| k.len())
            .max()
            .unwrap_or(0)
            .min(28);

        for frag in &scoped {
            let why = one_line_why(&frag.body);
            let keys = top_keys(frag);
            for (i, key) in keys.iter().enumerate() {
                let val = frag.settings.get(key).map(compact_value).unwrap_or_default();
                // Rationale prints once per fragment, on its first key line.
                let tail = if i == 0 { why.as_str() } else { "" };
                println!("    {:<width$}  {}  {}", key, truncate(&val, 32), tail, width = width);
            }
        }

        print_scope_footer(scope, &scoped, has_errors);
    }

    println!("\nlint: {lint_verdict}");
    println!("`ways settings show <key>` for a key's value, rationale, schema, and live status.");
    Ok(())
}

fn print_scope_footer(scope: Scope, scoped: &[&Fragment], gated: bool) {
    let target = match scope {
        Scope::User => "~/.claude/settings.json".to_string(),
        Scope::Project => "$CLAUDE_PROJECT_DIR/.claude/settings.json".to_string(),
        Scope::Managed => "(managed blob — pasted into the console, never auto-written)".to_string(),
    };
    let mut line = format!("  → {target}");
    if scope != Scope::Managed {
        // A store with lint errors won't compile/project, so a pending count would
        // be a lie — say the gate is closed instead. (Matches the `lint:` verdict.)
        if gated {
            line.push_str("  ·  project gated until lint errors are fixed");
        } else {
            let n = pending_count(scope, scoped);
            if n > 0 {
                let verb = if n == 1 { "key differs" } else { "keys differ" };
                line.push_str(&format!("  ·  {n} {verb} from live"));
            } else {
                line.push_str("  ·  in sync with live");
            }
        }
    }
    println!("{line}");
}

/// How many owned keys `project` would change for this scope, read against the
/// live `settings.json` and this scope's last-applied base.
fn pending_count(scope: Scope, scoped: &[&Fragment]) -> usize {
    let (baked, _) = compile::merge_scope(scoped);
    let baked_obj = baked.as_object().cloned().unwrap_or_default();
    let live_obj = live_settings(scope).as_object().cloned().unwrap_or_default();
    let base_obj = read_base(scope).as_object().cloned().unwrap_or_default();
    pending_of(&baked_obj, &live_obj, &base_obj)
}

/// Pure pending count, mirroring `project`'s three-way notion: an owned key
/// differs if its compiled value isn't already the live value; a base-tracked
/// owned key with no current fragment is a pending *removal* iff the live value
/// still equals what we last wrote (so `project` would delete it — if the user
/// has since changed it, `project` leaves it, so it isn't pending).
fn pending_of(baked: &Map<String, Value>, live: &Map<String, Value>, base: &Map<String, Value>) -> usize {
    let mut n = 0;
    for (k, v) in baked {
        if reconciler_owned(k) {
            continue;
        }
        if live.get(k) != Some(v) {
            n += 1;
        }
    }
    for (k, v) in base {
        if reconciler_owned(k) || baked.contains_key(k) {
            continue;
        }
        if live.get(k) == Some(v) {
            n += 1; // project would remove it
        }
    }
    n
}

/// This scope's projector base (`$XDG_STATE/agent-ways/settings-fragments-<scope>.json`).
/// Absent/unreadable → empty object. Mirrors `project::base_path`.
fn read_base(scope: Scope) -> Value {
    let path = crate::paths::state_root().join(format!("settings-fragments-{}.json", scope.as_str()));
    match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|_| Value::Object(Map::new())),
        Err(_) => Value::Object(Map::new()),
    }
}

// ── per-key detail ─────────────────────────────────────────────

fn show_key(store: &Path, frags: &[Fragment], key: &str) -> Result<()> {
    // Which scopes define this top-level key?
    let scopes: Vec<Scope> = [Scope::User, Scope::Project, Scope::Managed]
        .into_iter()
        .filter(|s| frags.iter().any(|f| f.scope == *s && top_keys(f).iter().any(|k| k == key)))
        .collect();

    if scopes.is_empty() {
        println!("No fragment sets `{key}` in {}.", store.display());
        if let Some((top, _)) = key.split_once('.') {
            println!("`show` works on top-level keys — try `{top}`; nested values appear in its detail.");
            return Ok(());
        }
        let known = schema_doc::active().and_then(|s| s.get(key)).is_some();
        if known {
            println!("It's a valid key — scaffold it: ways settings new {key}");
        } else {
            println!("It isn't in the current schema either (try `ways settings schema --refresh` if it's new).");
        }
        return Ok(());
    }

    let schema = schema_doc::active();
    let info = schema.and_then(|s| s.get(key));

    for scope in scopes {
        let scoped: Vec<&Fragment> = frags.iter().filter(|f| f.scope == scope).collect();
        let (baked, prov) = compile::merge_scope(&scoped);
        let value = baked.as_object().and_then(|o| o.get(key));

        println!("NAME");
        let ty = info.map(|i| i.ty.name()).unwrap_or("unknown to schema");
        println!("    {key} — {} scope · {ty}", scope.as_str());

        println!("\nVALUE");
        match value {
            Some(v) => {
                for line in pretty_value(v).lines() {
                    println!("    {line}");
                }
            }
            None => println!("    (none)"),
        }
        let sources = key_sources(&prov, key);
        if !sources.is_empty() {
            println!("    from: {}", sources.join(", "));
        }
        // Overridden? More than one fragment set a scalar for this key.
        let setters: Vec<String> = scoped
            .iter()
            .filter(|f| top_keys(f).iter().any(|k| k == key))
            .filter_map(|f| f.path.file_name().and_then(|s| s.to_str()).map(String::from))
            .collect();
        if setters.len() > 1 {
            println!(
                "    note: {} fragments contribute — merged per ADR-147 (objects deep-merge, \
                 permission lists union, scalars: last in filename order wins)",
                setters.len()
            );
        }

        println!("\nSCHEMA");
        match info {
            Some(i) if !i.description.trim().is_empty() => {
                for line in wrap(i.description.trim(), 72) {
                    println!("    {line}");
                }
            }
            Some(_) => println!("    (no description in the schema)"),
            None => println!("    key not found in the vendored schema (may be newer than it)"),
        }

        println!("\nWHY");
        let body = strip_leading_heading(&defining_body(&scoped, key));
        if body.trim().is_empty() {
            println!("    (no rationale — the fragment body is empty)");
        } else {
            for line in body.trim().lines() {
                println!("    {line}");
            }
        }

        println!("\nSTATUS");
        let live_obj = live_settings(scope).as_object().cloned().unwrap_or_default();
        println!("    {}", live_status(scope, key, value, &live_obj));
        println!();
    }
    Ok(())
}

/// The live status of a key: does the store's value match the live settings, or
/// is a projection pending? Pure — the caller supplies the live scope object, so
/// every branch is unit-testable (the filesystem read lives in the caller).
fn live_status(scope: Scope, key: &str, baked: Option<&Value>, live: &Map<String, Value>) -> String {
    if scope == Scope::Managed {
        return "managed scope — never auto-written; emitted as a console blob by `project`".into();
    }
    if reconciler_owned(key) {
        return "managed by the reconciler (`project` skips it; set via `ways reconcile`)".into();
    }
    let live_val = live.get(key);
    match (baked, live_val) {
        (Some(b), Some(l)) if b == l => "live in settings.json (matches the store)".into(),
        (Some(_), Some(_)) => "pending — the store differs from the live value; `project` would change it".into(),
        (Some(_), None) => "pending — not yet in settings.json; `project` would add it".into(),
        (None, _) => "no compiled value".into(),
    }
}

// ── shared helpers ─────────────────────────────────────────────

/// The live `settings.json` object for a scope (user → ~/.claude; project →
/// $CLAUDE_PROJECT_DIR/.claude). Absent/unreadable → empty object.
fn live_settings(scope: Scope) -> Value {
    let path = match scope {
        Scope::User => crate::paths::settings_json(),
        Scope::Project => {
            let root = std::env::var("CLAUDE_PROJECT_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            root.join(".claude").join("settings.json")
        }
        Scope::Managed => return Value::Object(Map::new()),
    };
    match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|_| Value::Object(Map::new())),
        Err(_) => Value::Object(Map::new()),
    }
}

/// Top-level keys a fragment sets (its `settings:` object keys).
fn top_keys(frag: &Fragment) -> Vec<String> {
    frag.settings
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// The fragment bodies that set `key`, joined — the rationale for the value.
fn defining_body(scoped: &[&Fragment], key: &str) -> String {
    scoped
        .iter()
        .filter(|f| top_keys(f).iter().any(|k| k == key))
        .map(|f| f.body.trim())
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The fragment file(s) that contributed to `key` per the provenance map — the
/// key itself for a scalar, or any nested `key.*` leaf for an object.
fn key_sources(prov: &Provenance, key: &str) -> Vec<String> {
    let nested = format!("{key}.");
    let mut out: Vec<String> = Vec::new();
    for (path, files) in prov {
        if path == key || path.starts_with(&nested) {
            for f in files {
                if !out.contains(f) {
                    out.push(f.clone());
                }
            }
        }
    }
    out
}

/// First meaningful line of a rationale body: skip a leading `#` heading and
/// blank lines, so the inventory shows the "why", not the title.
fn one_line_why(body: &str) -> String {
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("<!--") {
            continue;
        }
        return truncate(t, 60);
    }
    // Nothing but a heading — fall back to the heading text itself.
    body.lines()
        .map(str::trim)
        .find(|l| l.starts_with('#'))
        .map(|h| truncate(h.trim_start_matches('#').trim(), 60))
        .unwrap_or_default()
}

/// Drop a leading `# heading` line (and the blank lines after it) from a body —
/// the detail view already shows the key as NAME, so the heading is redundant in
/// WHY. Only strips a heading that's the very first content line.
fn strip_leading_heading(body: &str) -> String {
    let mut lines = body.lines().peekable();
    // Skip leading blank lines.
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    if lines.peek().is_some_and(|l| l.trim_start().starts_with('#')) {
        lines.next(); // drop the heading
        while lines.peek().is_some_and(|l| l.trim().is_empty()) {
            lines.next(); // and the blank line(s) after it
        }
    }
    lines.collect::<Vec<_>>().join("\n")
}

/// Compact single-line rendering of a value, for the inventory column.
fn compact_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => format!("[{} item{}]", a.len(), plural(a.len())),
        Value::Object(o) => format!("{{{} key{}}}", o.len(), plural(o.len())),
        other => other.to_string(),
    }
}

/// Multi-line pretty rendering for the detail VALUE (objects/arrays expand).
fn pretty_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()),
    }
}

fn lint_summary(findings: &[super::lint::Finding]) -> String {
    let errors = findings.iter().filter(|f| f.severity == super::lint::Severity::Error).count();
    let warns = findings.len() - errors;
    match (errors, warns) {
        (0, 0) => "clean".into(),
        (0, w) => format!("{w} warning{}", plural(w)),
        (e, 0) => format!("{e} error{} — compile/project are gated", plural(e)),
        (e, w) => format!("{e} error{}, {w} warning{}", plural(e), plural(w)),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= max {
        s
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Wrap text to `width` on word boundaries (for the schema description).
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        // char-based width (consistent with `truncate`) so a multibyte description
        // wraps at the intended visual column, not a byte count.
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn one_line_why_skips_heading_and_blanks() {
        let body = "# cleanupPeriodDays\n\nKeep a month of history. Long enough to reconstruct.";
        assert_eq!(one_line_why(body), "Keep a month of history. Long enough to reconstruct.");
    }

    #[test]
    fn one_line_why_falls_back_to_heading_when_only_a_heading() {
        assert_eq!(one_line_why("# Just a title"), "Just a title");
        assert_eq!(one_line_why(""), "");
    }

    #[test]
    fn strip_leading_heading_drops_title_not_body() {
        assert_eq!(
            strip_leading_heading("# cleanupPeriodDays\n\nKeep a month.\nSecond line."),
            "Keep a month.\nSecond line."
        );
        // No heading → unchanged.
        assert_eq!(strip_leading_heading("Just prose.\nMore."), "Just prose.\nMore.");
        // A later heading is left alone.
        assert_eq!(strip_leading_heading("Intro.\n# Section"), "Intro.\n# Section");
    }

    #[test]
    fn compact_value_summarizes_containers() {
        assert_eq!(compact_value(&json!("opus")), "opus");
        assert_eq!(compact_value(&json!(30)), "30");
        assert_eq!(compact_value(&json!(["a", "b", "c"])), "[3 items]");
        assert_eq!(compact_value(&json!({ "allow": [] })), "{1 key}");
        assert_eq!(compact_value(&json!([1])), "[1 item]");
    }

    #[test]
    fn truncate_respects_max_and_flattens_newlines() {
        assert_eq!(truncate("short", 32), "short");
        assert_eq!(truncate("a\nb", 32), "a b");
        let long = "x".repeat(50);
        let t = truncate(&long, 10);
        assert_eq!(t.chars().count(), 10);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn live_status_covers_every_branch() {
        let live = |pairs: &[(&str, Value)]| -> Map<String, Value> {
            pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
        };
        // Managed + reconciler-owned short-circuits (live map irrelevant).
        assert!(live_status(Scope::Managed, "model", Some(&json!("opus")), &Map::new()).contains("managed scope"));
        assert!(live_status(Scope::User, "hooks", Some(&json!({})), &Map::new()).contains("reconciler"));
        assert!(live_status(Scope::User, "permissions", Some(&json!({})), &Map::new()).contains("reconciler"));
        // Matches / pending-change / pending-add — now reachable without the FS.
        let store_val = json!(30);
        assert!(live_status(Scope::User, "cleanupPeriodDays", Some(&store_val), &live(&[("cleanupPeriodDays", json!(30))])).contains("matches"));
        assert!(live_status(Scope::User, "cleanupPeriodDays", Some(&store_val), &live(&[("cleanupPeriodDays", json!(14))])).contains("would change"));
        assert!(live_status(Scope::User, "cleanupPeriodDays", Some(&store_val), &Map::new()).contains("would add"));
    }

    #[test]
    fn pending_of_counts_changes_and_removals() {
        let m = |pairs: &[(&str, Value)]| -> Map<String, Value> {
            pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
        };
        // In sync → 0.
        assert_eq!(pending_of(&m(&[("model", json!("opus"))]), &m(&[("model", json!("opus"))]), &Map::new()), 0);
        // Value differs → 1.
        assert_eq!(pending_of(&m(&[("model", json!("opus"))]), &m(&[("model", json!("sonnet"))]), &Map::new()), 1);
        // Not yet live → 1.
        assert_eq!(pending_of(&m(&[("model", json!("opus"))]), &Map::new(), &Map::new()), 1);
        // Reconciler-owned keys never count.
        assert_eq!(pending_of(&m(&[("permissions", json!({"allow": []}))]), &Map::new(), &Map::new()), 0);
        // Pending removal: base has a key, no fragment sets it, live still matches base → 1.
        assert_eq!(pending_of(&Map::new(), &m(&[("cleanupPeriodDays", json!(30))]), &m(&[("cleanupPeriodDays", json!(30))])), 1);
        // But if the user changed it since we wrote it, project leaves it → 0.
        assert_eq!(pending_of(&Map::new(), &m(&[("cleanupPeriodDays", json!(99))]), &m(&[("cleanupPeriodDays", json!(30))])), 0);
    }

    #[test]
    fn key_sources_gathers_scalar_and_nested_contributors() {
        let mut prov = Provenance::new();
        prov.insert("model".into(), vec!["10-model.md".into()]);
        prov.insert("permissions.allow".into(), vec!["10-a.md".into(), "20-b.md".into()]);
        assert_eq!(key_sources(&prov, "model"), vec!["10-model.md"]);
        assert_eq!(key_sources(&prov, "permissions"), vec!["10-a.md", "20-b.md"]);
        assert!(key_sources(&prov, "env").is_empty());
    }

    #[test]
    fn wrap_breaks_on_word_boundaries() {
        let w = wrap("the quick brown fox jumps", 10);
        assert!(w.iter().all(|l| l.len() <= 10 || !l.contains(' ')));
        assert_eq!(w.join(" "), "the quick brown fox jumps");
    }

    #[test]
    fn lint_summary_phrasing() {
        let clean: Vec<super::super::lint::Finding> = vec![];
        assert_eq!(lint_summary(&clean), "clean");
    }
}
