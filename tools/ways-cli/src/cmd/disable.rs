//! ADR-131: project-scope per-way toggles.
//!
//! `ways disable <name>` and `ways enable <name>` edit
//! `{project}/.claude/ways.yaml` through the settings writer (ADR-503 §6):
//! they set or unset the key `ways.project.<name>` that `ways settings`
//! writes, under its lock, keeping comments and unrelated keys.
//!
//! Project scope only — there is no `--global` flag. Default state is
//! enabled (absence of an entry).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const HEADER: &str = "# Project-scope ways overlay — see ADR-115, ADR-131\n";

// ── Public entry points ────────────────────────────────────────

pub fn disable(name: &str) -> Result<()> {
    validate_way_name(name)?;
    warn_if_unknown(name);

    let path = project_overlay_path()?;
    write_toggle(&path, name, true)?;
    println!("disabled {name} (project: {})", path.display());
    Ok(())
}

pub fn enable(name: &str) -> Result<()> {
    validate_way_name(name)?;

    let path = project_overlay_path()?;
    if !path.exists() {
        println!("{name} is already enabled (no project overlay at {})", path.display());
        return Ok(());
    }
    let content = read_or_empty(&path)?;
    if !is_disabled(&content, name) {
        println!("{name} is already enabled");
        return Ok(());
    }
    write_toggle(&path, name, false)?;
    println!("enabled {name} (project: {})", path.display());
    Ok(())
}

pub fn list(names_only: bool) -> Result<()> {
    let cfg = crate::config::Config::load(&project_dir());

    // Machine-readable mode: bare names, one per line, no decoration, no
    // stderr commentary. Used by the bash subagent injector so it sees the
    // same disabled set the Rust parser does (single source of truth).
    if names_only {
        for w in cfg.disabled_ways() {
            println!("{w}");
        }
        return Ok(());
    }

    if cfg.disabled_ways().is_empty() {
        println!("no ways are disabled for this project");
        return Ok(());
    }
    for w in cfg.disabled_ways() {
        let marker = if way_exists(w) { " " } else { "?" };
        println!("{marker} {w}");
    }
    if cfg.disabled_ways().iter().any(|w| !way_exists(w)) {
        eprintln!("\nentries marked `?` do not match any way currently on disk \
                   (may have been renamed or removed upstream)");
    }
    Ok(())
}

// ── Path resolution ─────────────────────────────────────────────

fn project_dir() -> String {
    std::env::var("CLAUDE_PROJECT_DIR")
        .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()))
}

fn project_overlay_path() -> Result<PathBuf> {
    let dir = PathBuf::from(project_dir());
    Ok(dir.join(".claude").join("ways.yaml"))
}

// ── Validation ──────────────────────────────────────────────────

/// Way names follow the directory-derived form used by `way_id_from_path`:
/// lowercase ASCII alphanumerics, `_`, and `-`, joined by `/`. No empty
/// segments, no whitespace, no quoting characters, no comment markers.
/// Anything outside that grammar would either fail to resolve at runtime
/// or — worse — flow into a YAML key that the writer cannot reliably edit.
fn validate_way_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("way name cannot be empty");
    }
    for segment in name.split('/') {
        if segment.is_empty() {
            anyhow::bail!("way name must not have empty segments (no leading/trailing/double '/')");
        }
        if !segment.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
            anyhow::bail!(
                "invalid way name '{name}': segments must match [a-z0-9_-]+ (got '{segment}')"
            );
        }
    }
    Ok(())
}

fn way_exists(name: &str) -> bool {
    let project = PathBuf::from(project_dir()).join(".claude/ways").join(name);
    if project.is_dir() {
        return true;
    }
    let global = crate::util::home_dir().join(".claude/hooks/ways").join(name);
    global.is_dir()
}

fn warn_if_unknown(name: &str) {
    if !way_exists(name) {
        eprintln!(
            "[ways] warning: '{name}' does not match any way on disk. \
             Writing entry anyway (use `ways disable --list` to audit)."
        );
    }
}

// ── YAML edit ───────────────────────────────────────────────────

fn read_or_empty(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Disable `name` (`ways: {name: false}`) or remove its entry, through the
/// settings writer: locked, verified, renamed into place.
fn write_toggle(path: &Path, name: &str, disable: bool) -> Result<()> {
    agent_settings::writer::edit_file(path, Some(HEADER), |doc| toggle(doc, name, disable))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn toggle(doc: &mut agent_settings::yaml_edit::Doc, name: &str, disable: bool) -> Result<(), agent_settings::yaml_edit::EditError> {
    let key = ["ways".to_string(), name.to_string()];
    if disable {
        doc.set(&key, &serde_yaml::Value::Bool(false))
    } else {
        doc.unset(&key).map(|_| ())
    }
}

/// Returns true if `name` currently parses to disabled in the given content.
/// Uses the same parser the runtime config uses, so writer/reader agree.
fn is_disabled(content: &str, name: &str) -> bool {
    let mut cfg = crate::config::Config::default();
    cfg.apply_project_ways_overlay_public(content);
    cfg.disabled_ways().iter().any(|w| w == name)
}

/// The text `content` becomes when `name` is disabled or enabled: the
/// writer's edit without the file. Kept for the tests below.
#[cfg(test)]
pub(crate) fn rewrite_block(content: &str, name: &str, disable: bool) -> String {
    let start = if content.trim().is_empty() { HEADER } else { content };
    let mut doc = agent_settings::yaml_edit::Doc::parse(start).expect("test input parses");
    let before = doc.value().clone();
    toggle(&mut doc, name, disable).expect("edit");
    if doc.value() == &before {
        return content.to_string();
    }
    doc.verify().expect("round trip");
    doc.text()
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_canonical_names() {
        assert!(validate_way_name("itops/incident").is_ok());
        assert!(validate_way_name("softwaredev/code/quality").is_ok());
        assert!(validate_way_name("a_b-c/d-e_f").is_ok());
        assert!(validate_way_name("single").is_ok());
    }

    #[test]
    fn validate_rejects_misformed_names() {
        for bad in [
            "",
            "/leading",
            "trailing/",
            "double//slash",
            "with space",
            "Upper/Case",
            "has#comment",
            "has\"quote",
            "has\\backslash",
            "has\nnewline",
            "has:colon",
            "dot/./segment",
            "dot/../escape",
        ] {
            assert!(
                validate_way_name(bad).is_err(),
                "expected '{bad}' to be rejected"
            );
        }
    }

    #[test]
    fn disable_creates_block_in_empty_file() {
        let out = rewrite_block("", "itops/incident", true);
        assert!(out.contains("ways:"));
        assert!(out.contains("itops/incident: false"));
    }

    #[test]
    fn disable_appends_to_existing_block() {
        let input = "ways:\n  meta/introspection: false\n";
        let out = rewrite_block(input, "itops/incident", true);
        assert!(out.contains("meta/introspection: false"));
        assert!(out.contains("itops/incident: false"));
    }

    #[test]
    fn enable_removes_entry_keeps_others() {
        let input = "ways:\n  meta/introspection: false\n  itops/incident: false\n";
        let out = rewrite_block(input, "itops/incident", false);
        assert!(out.contains("meta/introspection: false"));
        assert!(!out.contains("itops/incident"));
    }

    #[test]
    fn enable_removes_ways_block_when_last_entry() {
        let input = "ways:\n  itops/incident: false\n";
        let out = rewrite_block(input, "itops/incident", false);
        assert!(!out.contains("ways:"));
        assert!(!out.contains("itops/incident"));
    }

    #[test]
    fn preserves_unrelated_keys_and_comments() {
        let input = "\
# top-level comment
language: en

ways:
  # comment inside ways
  meta/introspection: false

parent_boost_floor: 0.40
";
        let out = rewrite_block(input, "itops/incident", true);
        assert!(out.contains("# top-level comment"));
        assert!(out.contains("# comment inside ways"));
        assert!(out.contains("language: en"));
        assert!(out.contains("parent_boost_floor: 0.40"));
        assert!(out.contains("meta/introspection: false"));
        assert!(out.contains("itops/incident: false"));
    }

    #[test]
    fn handles_longform_entry_replacement() {
        // Long-form entry should be replaced by shorthand when re-disabled
        // (no harm; the schema accepts either).
        let input = "\
ways:
  itops/incident:
    enabled: false
";
        let out = rewrite_block(input, "itops/incident", true);
        // Should still contain one (and only one) disable for itops/incident.
        let count = out.matches("itops/incident").count();
        assert_eq!(count, 1);
        assert!(out.contains("itops/incident: false"));
    }

    #[test]
    fn enable_when_block_missing_is_noop() {
        let input = "language: en\n";
        let out = rewrite_block(input, "itops/incident", false);
        assert_eq!(out, input);
    }

    #[test]
    fn four_space_overlay_preserves_indent_on_disable() {
        let input = "\
ways:
    meta/introspection: false
";
        let out = rewrite_block(input, "itops/incident", true);
        // New entry must use the same 4-space indent — not mixed.
        assert!(out.contains("\n    meta/introspection: false"));
        assert!(out.contains("\n    itops/incident: false"));
        // Must parse cleanly.
        serde_yaml::from_str::<serde_yaml::Value>(&out)
            .expect("rewrite should produce valid YAML");
    }

    #[test]
    fn four_space_overlay_enable_removes_entry() {
        let input = "\
ways:
    meta/introspection: false
    itops/incident: false
";
        let out = rewrite_block(input, "itops/incident", false);
        assert!(out.contains("meta/introspection: false"));
        assert!(!out.contains("itops/incident"));
        serde_yaml::from_str::<serde_yaml::Value>(&out).unwrap();
    }

    #[test]
    fn four_space_overlay_longform_replacement() {
        let input = "\
ways:
    itops/incident:
        enabled: false
";
        let out = rewrite_block(input, "itops/incident", true);
        // Long-form gets replaced by shorthand at the same indent.
        assert_eq!(out.matches("itops/incident").count(), 1);
        assert!(out.contains("    itops/incident: false"));
        assert!(!out.contains("enabled: false"));
        serde_yaml::from_str::<serde_yaml::Value>(&out).unwrap();
    }

    #[test]
    fn matches_only_full_key_not_prefix() {
        // `itops/incident-secondary` must not be matched by `itops/incident`.
        let input = "\
ways:
  itops/incident-secondary: false
";
        let out = rewrite_block(input, "itops/incident", true);
        assert!(out.contains("itops/incident-secondary: false"));
        assert!(out.contains("\n  itops/incident: false"));
        assert_eq!(out.matches("itops/incident:").count(), 1);
    }

    #[test]
    fn round_trip_through_real_config_load() {
        // Writer's output must parse back to the same disabled set.
        let out = rewrite_block("", "itops/incident", true);
        let mut cfg = crate::config::Config::default();
        cfg.apply_project_ways_overlay_public(&out);
        assert_eq!(cfg.disabled_ways(), vec!["itops/incident".to_string()]);

        let out2 = rewrite_block(&out, "meta/introspection", true);
        let mut cfg2 = crate::config::Config::default();
        cfg2.apply_project_ways_overlay_public(&out2);
        assert_eq!(cfg2.disabled_ways().len(), 2);
    }
}
