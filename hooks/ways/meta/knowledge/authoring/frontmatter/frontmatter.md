---
description: the way frontmatter field reference — pattern, files, and commands triggers, semantic description and vocabulary, state triggers, when preconditions, macro, and scope
vocabulary: frontmatter field schema when precondition project scope subagent teammate macro prepend append threshold file-exists context-threshold session-start path commands files
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Way Frontmatter Fields

All values must be single-line. `ways author lint` validates each field against `frontmatter-schema.yaml`.

## Pattern-based

- `pattern:` - Regex matched against user prompts
- `pattern_strict:`, `pattern_keep:` - keyword-lane modifiers; see knowledge/authoring/keyword-lane(meta)
- `files:` - Regex matched against file paths (Edit/Write)
- `commands:` - Regex matched against bash commands

## Semantic

- `description:` - Natural language reference text for what this way covers
- `vocabulary:` - Space-separated domain keywords users would say
- Firing is global: the embedded prompt's cosine `s` against the alias (`description` + `vocabulary`) maps through the calibrated `g(s)` to a relevance probability, thresholded against global `τ_s` / `τ_k`. There is no per-way threshold (ADR-156).
- Engine: embedding-only (per ADR-125). Explicit `pattern:` / `commands:` regex still fire independently.

## State-based

- `trigger:` - State condition type (`context-threshold`, `file-exists`, `session-start`)
- `threshold:` - For context-threshold: percentage (0-100)
- `path:` - For file-exists: glob pattern relative to project

## Preconditions (`when:` block)

- `when:` - Deterministic gate checked before any matching. If unmet, way is skipped entirely.
  - `project:` - Only fire in this project directory (e.g., `~/.claude`). Path is resolved for comparison.

```yaml
when:
  project: ~/.claude    # only fire when working in claude-code-config
```

Ways without a `when:` block fire everywhere (the default). Use `when:` sparingly — only for self-referential ways that are meaningless outside their home project.

## Firing cadence (`refire:`)

Fire-bearing ways should carry a `refire:` field: a fraction of the session's context window (`refire: 0.15`) or a preset name (`refire: normal`) that sets how soon the way re-discloses (ADR-126). Without it the firing gate refuses the way, and `ways author lint` reports an error. Forms, presets, and common choices are in knowledge/authoring/refire(meta).

## Other

- `macro:` - `prepend` or `append` to run `macro.sh` for dynamic context
- `scope:` - `agent`, `subagent`, `teammate` (comma-separated, default: agent). `agent` means any agent that is not a teammate, so it also fires on a plain subagent's own tool calls. `subagent` reaches a subagent only through the Task stash at dispatch. `teammate` fires for team members.

## See Also

- knowledge/authoring(meta) — parent: way format and matching strategy
- knowledge/authoring/keyword-lane(meta) — the `pattern:` lane in depth
- knowledge/authoring/refire(meta) — firing cadence in depth
