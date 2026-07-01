//! Fragment loader for the config store (ADR-147).
//!
//! A config store is a directory of `NN-*.md` files. Each file carries YAML
//! frontmatter with a `settings:` block (a `settings.json` fragment spelled in
//! YAML), a `scope`, and an optional `mandatory` flag; the markdown body below
//! the frontmatter holds the human-readable rationale. This module reads that
//! tree into [`Fragment`] values in deterministic filename order.
//!
//! Ordering is **alphabetical by filename**, matching Claude Code's
//! `managed-settings.d` merge law (systemd drop-in conventions): later files win.
//! The `NN-` prefix convention exists so lexical order matches authoring intent —
//! use zero-padded prefixes (`10-`, `20-`) so `10-` sorts before `20-` and not
//! after `2-`.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Configuration scope a fragment targets. Mirrors Claude Code's settings
/// precedence tiers (ADR-147, "Managed-scope interop").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    User,
    Project,
    Managed,
}

impl Scope {
    /// Lowercase name, for diagnostics.
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project => "project",
            Scope::Managed => "managed",
        }
    }
}

/// One config fragment: a parsed `NN-*.md` file.
///
/// The loader captures the whole fragment faithfully. The linter reads only
/// `scope` and `settings`; `order_prefix`, `mandatory`, and `body` are consumed
/// by the compile/project slices (org-lock emission, provenance manifest,
/// rendered rationale) — hence the forward-looking `allow(dead_code)` on them,
/// removed as each gains a reader.
#[derive(Debug, Clone)]
pub struct Fragment {
    /// Source file path.
    pub path: PathBuf,
    /// Numeric filename ordering prefix (the `NN` in `NN-name.md`), if present.
    /// Informational only — actual load order is the alphabetical filename sort.
    #[allow(dead_code)]
    pub order_prefix: Option<u32>,
    /// Target scope.
    pub scope: Scope,
    /// Org-lock flag. Only meaningful at managed scope; carried verbatim.
    #[allow(dead_code)]
    pub mandatory: bool,
    /// The `settings:` block as a `settings.json` fragment. Always a JSON
    /// object (an absent or null `settings:` normalizes to `{}`).
    pub settings: serde_json::Value,
    /// Markdown body after the frontmatter — the rationale, trimmed.
    #[allow(dead_code)]
    pub body: String,
}

/// The frontmatter shape we deserialize. `settings:` is deserialized straight
/// into a `serde_json::Value` — serde_yaml drives serde_json's `Value`
/// `Deserialize`, so YAML maps/seqs/scalars land in JSON's data model directly,
/// no hand-written YAML→JSON walk. (Fidelity caveat: YAML `yes`/`no`/`on`/`off`
/// are *strings* under serde_yaml's core schema, not booleans — a user who
/// writes `yes` expecting `true` gets `"yes"`, which the schema type-check
/// surfaces downstream. See the unit tests.)
#[derive(Debug, Deserialize)]
struct FrontMatter {
    scope: Scope,
    #[serde(default)]
    mandatory: bool,
    #[serde(default)]
    settings: serde_json::Value,
}

/// Load every `*.md` fragment in `dir`, alphabetically by filename. Non-`.md`
/// files and subdirectories are ignored. Errors on the first unreadable or
/// malformed fragment, naming the file.
pub fn load_dir(dir: &Path) -> Result<Vec<Fragment>> {
    if !dir.is_dir() {
        bail!("config store is not a directory: {}", dir.display());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading config store {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "md"))
        .collect();
    // Alphabetical by filename — matches Claude Code's managed-settings.d order.
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    files.iter().map(|p| load_file(p)).collect()
}

/// Parse a single fragment file.
pub fn load_file(path: &Path) -> Result<Fragment> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading fragment {}", path.display()))?;
    let (fm_yaml, body) =
        split_frontmatter(&raw).with_context(|| format!("in {}", path.display()))?;
    let fm: FrontMatter = serde_yaml::from_str(fm_yaml)
        .with_context(|| format!("parsing frontmatter in {}", path.display()))?;

    // Normalize an absent/null `settings:` to an empty object, and reject a
    // non-object `settings:` here so downstream stages can assume a JSON object.
    let settings = match fm.settings {
        serde_json::Value::Null => serde_json::json!({}),
        v @ serde_json::Value::Object(_) => v,
        other => bail!(
            "`settings:` in {} must be a mapping of settings.json keys, got {}",
            path.display(),
            json_kind(&other)
        ),
    };

    Ok(Fragment {
        path: path.to_path_buf(),
        order_prefix: order_prefix(path),
        scope: fm.scope,
        mandatory: fm.mandatory,
        settings,
        body: body.trim().to_string(),
    })
}

/// Split a leading `---`-delimited YAML frontmatter block from the markdown
/// body. The file must open with a `---` line; returns `(frontmatter, body)`.
fn split_frontmatter(raw: &str) -> Result<(&str, &str)> {
    // Tolerate a UTF-8 BOM.
    let s = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if !(s.starts_with("---\n") || s.starts_with("---\r\n")) {
        return Err(anyhow!(
            "fragment must begin with a `---` YAML frontmatter block"
        ));
    }
    // Skip past the opener line.
    let after_open = s.find('\n').map(|i| i + 1).unwrap();
    let rest = &s[after_open..];

    // Find a closing line that is exactly `---` (allowing a trailing `\r`).
    let bytes = rest.as_bytes();
    let mut i = 0usize;
    loop {
        let rel = rest[i..]
            .find("---")
            .ok_or_else(|| anyhow!("frontmatter `---` block is not closed"))?;
        let pos = i + rel;
        let at_line_start = pos == 0 || bytes[pos - 1] == b'\n';
        let after = pos + 3;
        let at_line_end =
            after == rest.len() || bytes[after] == b'\n' || bytes[after] == b'\r';
        if at_line_start && at_line_end {
            let fm = &rest[..pos];
            let body_start = match rest[after..].find('\n') {
                Some(j) => after + j + 1,
                None => rest.len(),
            };
            return Ok((fm, &rest[body_start..]));
        }
        i = after;
    }
}

/// Parse the leading run of ASCII digits in a filename (the `NN` in `NN-*.md`).
fn order_prefix(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Human name for a JSON value's kind, for error messages. Shared with the
/// linter so both surfaces speak the same JSON vocabulary.
pub(crate) fn json_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// Fresh, empty temp directory for a single test.
    fn tmpdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ways-cfgfrag-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, contents).unwrap();
        p
    }

    #[test]
    fn parses_scope_mandatory_settings_and_body() {
        let dir = tmpdir();
        let p = write(
            &dir,
            "10-git.md",
            "---\n\
             scope: user\n\
             mandatory: false\n\
             settings:\n\
             \x20 permissions:\n\
             \x20   allow: [\"Bash(git:*)\", \"Bash(gh:*)\"]\n\
             \x20   deny: [\"Bash(rm -rf *)\"]\n\
             ---\n\
             # Git & GitHub permissions\n\
             Let Claude run git/gh unprompted.\n",
        );
        let f = load_file(&p).unwrap();
        assert_eq!(f.scope, Scope::User);
        assert!(!f.mandatory);
        assert_eq!(f.order_prefix, Some(10));
        assert_eq!(
            f.settings["permissions"]["allow"][0],
            serde_json::json!("Bash(git:*)")
        );
        assert_eq!(f.settings["permissions"]["deny"][0], serde_json::json!("Bash(rm -rf *)"));
        assert!(f.body.starts_with("# Git & GitHub permissions"));
        assert!(f.body.contains("unprompted"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn yaml_yes_no_are_strings_not_booleans() {
        // Fidelity boundary: under serde_yaml's core schema, `yes`/`no`/`on`/`off`
        // deserialize as STRINGS, while `true`/`false` are booleans. A user who
        // writes `yes` expecting JSON `true` gets `"yes"` — the schema type-check
        // (a later slice) is what turns that into an actionable lint finding. This
        // test pins the boundary so a serde_yaml upgrade that changes it is caught.
        let dir = tmpdir();
        let p = write(
            &dir,
            "10-coerce.md",
            "---\n\
             scope: user\n\
             settings:\n\
             \x20 looksBool: yes\n\
             \x20 realBool: true\n\
             ---\n\
             body\n",
        );
        let f = load_file(&p).unwrap();
        assert_eq!(
            f.settings["looksBool"],
            serde_json::json!("yes"),
            "YAML `yes` must land as a string, not a boolean"
        );
        assert_eq!(f.settings["realBool"], serde_json::json!(true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn absent_settings_normalizes_to_empty_object() {
        let dir = tmpdir();
        let p = write(&dir, "10-empty.md", "---\nscope: project\n---\njust rationale\n");
        let f = load_file(&p).unwrap();
        assert_eq!(f.settings, serde_json::json!({}));
        assert_eq!(f.scope, Scope::Project);
        assert_eq!(f.body, "just rationale");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_frontmatter_is_an_error() {
        let dir = tmpdir();
        let p = write(&dir, "10-nofm.md", "# just a markdown file\nno frontmatter here\n");
        let err = load_file(&p).unwrap_err();
        assert!(
            err.to_string().contains("frontmatter") || format!("{err:#}").contains("frontmatter"),
            "expected a frontmatter error, got: {err:#}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unclosed_frontmatter_is_an_error() {
        let dir = tmpdir();
        let p = write(&dir, "10-open.md", "---\nscope: user\nsettings:\n  model: opus\n");
        let err = load_file(&p).unwrap_err();
        assert!(format!("{err:#}").contains("not closed"), "got: {err:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_scope_is_an_error() {
        let dir = tmpdir();
        let p = write(&dir, "10-noscope.md", "---\nmandatory: true\n---\nbody\n");
        let err = load_file(&p).unwrap_err();
        assert!(format!("{err:#}").contains("scope"), "got: {err:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_object_settings_is_an_error() {
        let dir = tmpdir();
        let p = write(&dir, "10-scalar.md", "---\nscope: user\nsettings: \"just a string\"\n---\nb\n");
        let err = load_file(&p).unwrap_err();
        assert!(format!("{err:#}").contains("must be a mapping"), "got: {err:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_dir_orders_alphabetically_and_skips_non_md() {
        let dir = tmpdir();
        write(&dir, "20-b.md", "---\nscope: user\n---\nsecond\n");
        write(&dir, "10-a.md", "---\nscope: user\n---\nfirst\n");
        write(&dir, "notes.txt", "ignored\n");
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        let frags = load_dir(&dir).unwrap();
        assert_eq!(frags.len(), 2, "only .md files load");
        assert_eq!(frags[0].body, "first");
        assert_eq!(frags[1].body, "second");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_numeric_prefix_yields_none() {
        let dir = tmpdir();
        let p = write(&dir, "permissions.md", "---\nscope: user\n---\nb\n");
        let f = load_file(&p).unwrap();
        assert_eq!(f.order_prefix, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
