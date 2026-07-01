//! `ways settings new <key>` — scaffold a syntactically-correct fragment from
//! the vendored schema (ADR-147).
//!
//! The MVP authoring aid: the user names a key, and we instantiate a valid
//! `NN-<key>.md` with a type-correct placeholder in the `settings:` block and the
//! key's schema description pre-filled in the body. The user fills in the value.
//! Only keys the schema knows can be scaffolded, so every template is valid by
//! construction.

use super::fragment::Scope;
use super::{schema, schema_doc};
use anyhow::{bail, Result};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Scaffold a fragment for `key` into `dir`. `scope` defaults to `managed` for
/// managed-only keys, else `user`.
pub fn run(key: &str, scope: Option<Scope>, dir: &Path) -> Result<()> {
    let info = match schema_doc::bundled().get(key) {
        Some(i) => i,
        None => bail!(
            "`{key}` is not a known Claude Code settings key. \
             Run `ways settings schema` for the source, or check the spelling."
        ),
    };

    let scope = scope.unwrap_or_else(|| match schema::scope_class(key) {
        Some(schema::ScopeClass::ManagedOnly) => Scope::Managed,
        _ => Scope::User,
    });

    std::fs::create_dir_all(dir)?;
    if let Some(existing) = existing_fragment_for(dir, key)? {
        bail!("a fragment for `{key}` already exists: {}", existing.display());
    }

    let prefix = next_prefix(dir)?;
    let path = dir.join(format!("{prefix:02}-{key}.md"));
    std::fs::write(&path, render(key, scope, info))?;

    println!("Created {}", path.display());
    println!(
        "  {} scope. Fill in the placeholder, then: ways settings lint {}",
        scope.as_str(),
        dir.display()
    );
    Ok(())
}

/// Render the fragment text: `---` frontmatter (scope + a one-key `settings:`
/// block with a type-correct placeholder) then a body carrying the description.
fn render(key: &str, scope: Scope, info: &schema_doc::PropInfo) -> String {
    let mut settings = Map::new();
    settings.insert(key.to_string(), info.placeholder.clone());
    let mut fm = Map::new();
    fm.insert("scope".to_string(), Value::String(scope.as_str().to_string()));
    fm.insert("settings".to_string(), Value::Object(settings));

    let fm_yaml = serde_yaml::to_string(&Value::Object(fm))
        .unwrap_or_else(|_| format!("scope: {}\nsettings: {{}}\n", scope.as_str()));

    let desc = if info.description.trim().is_empty() {
        "(no description in the schema)".to_string()
    } else {
        info.description.trim().to_string()
    };

    format!(
        "---\n{fm_yaml}---\n# {key}\n\n{desc}\n\n\
         <!-- type: {ty}. Replace the placeholder value above, then run \
         `ways settings lint`. -->\n",
        ty = info.ty.name()
    )
}

/// The next `NN` filename prefix: max existing + 10, or 10 if the store is empty.
fn next_prefix(dir: &Path) -> Result<u32> {
    let mut max = 0u32;
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.extension().is_some_and(|x| x == "md") {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
                if let Ok(n) = digits.parse::<u32>() {
                    max = max.max(n);
                }
            }
        }
    }
    Ok(if max == 0 { 10 } else { max.saturating_add(10) })
}

/// An existing `*-<key>.md` (or bare `<key>.md`) fragment, if any.
fn existing_fragment_for(dir: &Path, key: &str) -> Result<Option<PathBuf>> {
    let suffix = format!("-{key}.md");
    let bare = format!("{key}.md");
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            if name.ends_with(&suffix) || name == bare {
                return Ok(Some(p));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::settings::fragment::load_file;
    use crate::cmd::settings::lint::{check, Severity};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn tmpdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ways-scaffold-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scaffolds_a_normal_key_that_loads_and_lints_clean() {
        let dir = tmpdir();
        run("cleanupPeriodDays", None, &dir).unwrap();
        let path = dir.join("10-cleanupPeriodDays.md");
        assert!(path.exists());
        // The round-trip guarantee: generated file loads and lints clean.
        let frag = load_file(&path).unwrap();
        assert_eq!(frag.scope, Scope::User);
        assert!(frag.settings.get("cleanupPeriodDays").unwrap().is_number());
        assert!(!frag.body.is_empty(), "description should land in the body");
        let findings = check(&[frag]);
        assert!(
            findings.iter().all(|f| f.severity != Severity::Error),
            "scaffold must lint without errors: {findings:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn managed_only_key_defaults_to_managed_scope() {
        let dir = tmpdir();
        run("allowManagedHooksOnly", None, &dir).unwrap();
        let frag = load_file(&dir.join("10-allowManagedHooksOnly.md")).unwrap();
        assert_eq!(frag.scope, Scope::Managed);
        // At managed scope it is scope-legal — no findings.
        assert!(check(&[frag]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_key_is_refused() {
        let dir = tmpdir();
        let err = run("totallyMadeUpKey", None, &dir).unwrap_err();
        assert!(err.to_string().contains("not a known"), "got: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_to_clobber_existing_fragment() {
        let dir = tmpdir();
        run("model", None, &dir).unwrap();
        let err = run("model", None, &dir).unwrap_err();
        assert!(err.to_string().contains("already exists"), "got: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefix_increments_with_store_contents() {
        let dir = tmpdir();
        run("model", None, &dir).unwrap();
        run("cleanupPeriodDays", None, &dir).unwrap();
        assert!(dir.join("10-model.md").exists());
        assert!(dir.join("20-cleanupPeriodDays.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
