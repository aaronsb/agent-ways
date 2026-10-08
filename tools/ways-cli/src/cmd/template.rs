//! Scaffold a new way with frontmatter and body template.
//!
//! Creates the way directory, the English .md file, and the golden-prompt
//! sidecar (ADR-701 §9) with blank direct and situational prompts, which a core
//! way must fill before lint passes. Per ADR-139, new ways are
//! English-only; localization is adopter-run via the ways-localize skill, not
//! pre-generated as translation stubs.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};


/// The golden sidecar a new way starts with. Lint rejects the blanks in a core
/// root until they are filled.
const GOLDEN_TEMPLATE: &str = "{\"kind\":\"direct\",\"prompt\":\"\"}\n{\"kind\":\"situational\",\"prompt\":\"\"}\n";

pub fn run(
    path: String,
    description: String,
    vocabulary: Option<String>,
    scope: String,
    global: bool,
) -> Result<()> {
    // Resolve the ways root: the project's `.claude/ways/`, or the operator's
    // own root (ADR-143). Never the shipped ways under `~/.claude/hooks/ways`:
    // that is a projection of the app, and `ways update` replaces it.
    let ways_root = if global {
        crate::paths::user_ways_root()
    } else {
        match crate::util::detect_project_dir() {
            Some(proj) => PathBuf::from(proj).join(".claude/ways"),
            None => crate::paths::user_ways_root(),
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

    let golden_file = way_dir.join(format!("{way_name}.golden.jsonl"));
    std::fs::write(&golden_file, GOLDEN_TEMPLATE)?;
    eprintln!("Created: {}", golden_file.display());

    eprintln!();
    eprintln!("Next steps:");
    eprintln!("  1. Edit {}", way_file.display());
    eprintln!("  2. Fill both prompts in {} (a direct and a situational one)", golden_file.display());
    eprintln!("  3. ways author lint {}", way_file.display());
    eprintln!("  4. ways corpus");
    eprintln!("  5. ways author match \"<a prompt that should fire it>\"");

    Ok(())
}
