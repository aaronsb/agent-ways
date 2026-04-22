//! Scaffold a new way with frontmatter, body template, and locale stubs.
//!
//! Creates the way directory, .md file, and .locales.jsonl with entries
//! for all currently-covered languages. Locale descriptions start as
//! English placeholders that need translation.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::agents;
use crate::util::home_dir;

pub fn run(
    path: String,
    description: String,
    vocabulary: Option<String>,
    scope: String,
    global: bool,
) -> Result<()> {
    // Resolve the ways root
    let ways_root = if global {
        home_dir().join(".claude/hooks/ways")
    } else {
        // Try project-local first
        match crate::util::detect_project_dir() {
            Some(proj) => PathBuf::from(proj).join(".claude/ways"),
            None => home_dir().join(".claude/hooks/ways"),
        }
    };

    // Parse path: "softwaredev/code/newway" → dir + name
    let way_path = Path::new(&path);
    let way_name = way_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&path);

    let way_dir = ways_root.join(&path);
    let way_file = way_dir.join(format!("{way_name}.md"));
    let locales_file = way_dir.join(format!("{way_name}.locales.jsonl"));

    if way_file.exists() {
        bail!("Way already exists: {}", way_file.display());
    }

    // Create directory
    std::fs::create_dir_all(&way_dir)?;

    // Generate the way .md file
    let vocab = vocabulary.as_deref().unwrap_or("keyword1 keyword2 keyword3");
    let md_content = format!(
        r#"---
description: {description}
vocabulary: {vocab}
refire: 0.15
scope: {scope}
---
<!-- epistemic: convention -->
# {title}

<!-- Replace this section with your guidance. -->
<!-- Write for a reader with no prior context. Include the *why*. -->

## When This Fires

This way activates when the user or agent is working with:
- <!-- describe the trigger scenarios -->

## Guidance

<!-- Core guidance goes here. Be specific and actionable. -->
<!-- Aim for <80 lines total. If longer, consider a progressive disclosure tree. -->

## Common Patterns

<!-- Optional: patterns, examples, or anti-patterns -->

## See Also

<!-- Optional: related ways using the format: -->
<!-- - wayname(domain) — brief description -->
"#,
        description = description,
        vocab = vocab,
        scope = scope,
        title = way_name
            .chars()
            .enumerate()
            .map(|(i, c)| if i == 0 { c.to_uppercase().next().unwrap() } else { c })
            .collect::<String>()
            .replace(['-', '_'], " "),
    );

    std::fs::write(&way_file, md_content)?;
    eprintln!("Created: {}", way_file.display());

    // Generate locale stubs for active covered languages only
    let covered: Vec<String> = get_covered_languages(&ways_root)
        .into_iter()
        .filter(|lang| crate::agents::is_language_active(lang))
        .collect();

    if !covered.is_empty() {
        let mut lines: Vec<String> = Vec::new();
        for lang in &covered {
            let native_name = get_language_name(lang);
            let entry = serde_json::json!({
                "lang": lang,
                "description": format!("[TRANSLATE to {}] {}", native_name, description),
                "vocabulary": format!("[TRANSLATE to {}] {}", native_name, vocab),
            });
            lines.push(serde_json::to_string(&entry)?);
        }
        lines.sort();
        let content = lines.join("\n") + "\n";
        std::fs::write(&locales_file, content)?;
        eprintln!(
            "Created: {} ({} languages: {})",
            locales_file.display(),
            covered.len(),
            covered.join(", ")
        );
    }

    eprintln!();
    eprintln!("Next steps:");
    eprintln!("  1. Edit {}", way_file.display());
    eprintln!("  2. Translate descriptions in {}", locales_file.display());
    eprintln!("  3. ways corpus && ways tune --apply");
    eprintln!("  4. ways lint --global");

    Ok(())
}

/// Find which languages are currently covered by scanning existing .locales.jsonl files.
fn get_covered_languages(ways_root: &Path) -> Vec<String> {
    let mut langs = std::collections::BTreeSet::new();

    // Sample the first .locales.jsonl we find
    for entry in walkdir::WalkDir::new(ways_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !fname.ends_with(".locales.jsonl") {
            continue;
        }

        if let Ok(entries) = crate::frontmatter::parse_locales_jsonl(path) {
            for e in entries {
                langs.insert(e.lang);
            }
            // One file is enough to know what languages are covered
            break;
        }
    }

    langs.into_iter().collect()
}

/// Look up the English name for a language code from languages.json.
fn get_language_name(code: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(agents::LANGUAGES_JSON) {
        Ok(v) => v,
        Err(_) => return code.to_string(),
    };
    parsed
        .get("languages")
        .and_then(|v| v.get(code))
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(code)
        .to_string()
}
