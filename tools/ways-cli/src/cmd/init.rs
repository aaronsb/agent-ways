//! Initialize project .claude/ways/ structure.
//! Replaces init-project-ways.sh (81 lines).

use anyhow::Result;
use std::path::PathBuf;

const GITIGNORE_CONTENT: &str = "\
# Developer-local files (not committed)
settings.local.json
todo-*.md
memory/
projects/
plans/

# Ways and CLAUDE.md ARE committed (shared team knowledge)
";

const TEMPLATE_CONTENT: &str = "---
# Template placeholder — this file's frontmatter is intentionally empty so it
# never matches at runtime. Copy this file to your new way's location and
# replace the frontmatter with the fields shown in \"Canonical Frontmatter\"
# below.
---
# Way Template

Starting point for authoring a new way. When the scaffolder
(`ways template <path> --description \"...\" --vocabulary \"...\"`) doesn't fit,
copy this file to `.claude/ways/{domain}/{wayname}/{wayname}.md` and edit.

## Canonical Frontmatter

```yaml
---
description: what this way covers, in natural language users would say
vocabulary: space-separated keywords users would say when this matters
refire: 0.15
scope: agent
---
```

**Required for fire-bearing ways:** `description`, `vocabulary`, `refire`.
**Common:** `scope: agent` (default); `subagent` or `teammate` for scoped ways.

## Other Trigger Types

| Field | Matches against |
|---|---|
| `pattern:` | User prompts (regex) |
| `files:` | File paths — Edit/Write hooks (regex) |
| `commands:` | Bash commands (regex) |
| `trigger:` | State conditions: `session-start`, `context-threshold`, `file-exists` |

Semantic matching (`description` + `vocabulary`) is additive with regex triggers — either channel can fire the way.

## Refire Cadence (ADR-126)

`refire:` controls re-disclosure — how quickly the way becomes eligible to fire again after a fire. The value is a fraction of the session's context window, resolved at fire time against the operator's model. Way files stop encoding host-specific token counts.

| Intent | Numeric | Preset name |
|---|---|---|
| Static-heavy payload, 1–2 fires per session | `0.4` | `rare` |
| Load-bearing guidance, ~3 fires per session | `0.15` | `normal` |
| Event handler, fires often relative to session | `0.05` | `frequent` |
| Disclose once per session | `1.0` | `once` |

Numeric values between presets are valid — e.g., `refire: 0.2` sits between `normal` and `rare` and is what ADR-127's 1M-Opus-tuned ways migrated to.

Numeric form pins the cadence to today's model. Preset form tracks the project's `refire_presets` config for portability across model generations.

## Writing the Body

- Be directive: \"Use conventional commits\" not \"It is recommended to use...\"
- Include the *why* — an agent that understands the reason applies better judgment at the edges.
- Aim for ~40 lines; decompose into a progressive-disclosure tree if >80 lines or >2 sub-topics.
- Use tables for dense, scannable signal. Skip preambles.

## Testing

- `ways lint <way>` — validate frontmatter against the schema
- `ways suggest <way>` — find vocabulary gaps
";

pub fn run(project: Option<&str>) -> Result<()> {
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            std::env::var("CLAUDE_PROJECT_DIR")
                .unwrap_or_else(|_| std::env::var("PWD").unwrap_or_else(|_| ".".to_string()))
        });

    let claude_dir = PathBuf::from(&project_dir).join(".claude");
    let ways_dir = claude_dir.join("ways");
    let gitignore = claude_dir.join(".gitignore");
    let template = ways_dir.join("_template.md");

    // Only create if .claude exists or this is a git repo
    let git_dir = PathBuf::from(&project_dir).join(".git");
    if !claude_dir.is_dir() && !git_dir.is_dir() {
        return Ok(());
    }

    if !ways_dir.is_dir() {
        std::fs::create_dir_all(&ways_dir)?;
    }

    if !gitignore.is_file() {
        std::fs::write(&gitignore, GITIGNORE_CONTENT)?;
        eprintln!("Created .claude/.gitignore");
    }

    if !template.is_file() {
        std::fs::write(&template, TEMPLATE_CONTENT)?;
        eprintln!("Created project ways template: {}", template.display());
    }

    Ok(())
}
