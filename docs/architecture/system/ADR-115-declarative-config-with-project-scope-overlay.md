---
status: Accepted
date: 2026-04-10
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-111
---

# ADR-115: Declarative Configuration with Project-Scope Overlay

## Context

`attend` (ADR-113) introduced a two-layer configuration pattern during implementation: a user-scope config at `~/.config/attend/config.yaml` and a project-scope overlay at `{project}/.claude/attend.yaml`. The project overlay uses `+/-` syntax to add or remove sensors without rewriting the full config. This pattern proved clean enough to propose as the standard for the agent-ways workspace.

`ways` currently has no central configuration file. Its "config" is distributed across individual way files (frontmatter declares thresholds, vocabulary, triggers) and environment variables. This works for way authoring but leaves system-level tuning scattered: disclosure gate parameters live in the Rust source, embedding engine paths are hardcoded or env-var-driven, and there's no project-scope override for global behavior.

This ADR proposes adopting attend's config pattern for the workspace — a shared convention that both `ways` and `attend` (and future sibling tools) follow.

## Decision

### The pattern

Each tool in the agent-ways workspace may declare a config file at two scopes:

```
~/.config/{tool}/config.yaml          # user scope — always loaded
{project}/.claude/{tool}.yaml         # project scope — layered on top
```

User scope provides defaults. Project scope overrides or extends them. Tools load user scope first, then apply project scope on top. Missing files at either scope are a no-op (compiled defaults apply).

### Config format

YAML subset — flat keys, nested sections, lists. No full YAML parser required; the minimal subset that covers key-value pairs and two-level nesting is sufficient. This keeps tools zero-dependency (no serde, no yaml crate).

### Project-scope overlay syntax

For collection-type configs (sensors in attend, potentially way groups in ways), the project overlay uses `+/-` to modify the set:

```yaml
# project/.claude/attend.yaml
sensors:
  +disk-pressure:              # add a project-local sensor
    script: .claude/sensors/check-disk.sh
    interval: 120
  -processes:                  # disable a user-scope sensor
```

The `+` prefix adds an entry that doesn't exist in user scope. The `-` prefix disables an entry from user scope. Unprefixed entries override properties of existing entries.

### Trust model

Same as ways scoping:

- **User scope** (`~/.config/`) is trusted — the user installed it.
- **Project scope** (`{project}/.claude/`) has the same trust level as project-scope ways. Scripts declared in project config are code that runs on poll — same scrutiny as project-scope way macros.

### CLI convention

Each tool provides a `config` subcommand:

```
{tool} config init     # write default config to user scope
{tool} config show     # display effective config (both layers merged)
{tool} config path     # show user and project config file paths
```

### What this means for ways

`ways` could adopt this pattern for:

- **Engine configuration**: embedding model path, corpus path, fallback behavior, forced engine selection — currently hardcoded or env-var-driven
- **Disclosure gate tuning**: re-disclosure intervals, token-gated thresholds — currently compiled defaults in Rust
- **Per-project way groups**: enable/disable way categories per project without removing files
- **Scoring overrides**: per-project BM25/embedding threshold adjustments

Example:

```yaml
# ~/.config/ways/config.yaml
engine:
  model: minilm-l6-v2.gguf
  fallback: bm25
  forced: auto

disclosure:
  redisclose_default: 10
  token_gate: 0.3

# project/.claude/ways.yaml
scoring:
  +softwaredev/code/security:
    threshold: 1.5              # lower threshold for security-sensitive project
  -ea/comms:                    # disable comms way in this project
```

### XDG compliance

Config files follow XDG conventions:
- Config: `$XDG_CONFIG_HOME/{tool}/` (default `~/.config/{tool}/`)
- State: `$XDG_CACHE_HOME/{tool}/` (default `~/.cache/{tool}/`)

This is consistent with the existing XDG separation documented in project memory.

## Consequences

### Positive

- **Tuning without recompiling.** Governor params, sensor intervals, scoring thresholds — all externalized. Edit a file, restart (or self-reload in attend's case).
- **Project-scope customization.** A security-focused project can lower security way thresholds. A hardware project can add system sensors. A documentation project can disable code-focused ways.
- **Shared convention.** All workspace tools follow the same pattern. Users learn it once.
- **Progressive adoption.** Tools can adopt the pattern incrementally. Attend shipped it first; ways can adopt it when ready without changing attend's implementation.
- **Zero dependency.** The minimal YAML parser handles the config subset without pulling in serde or yaml crates.

### Negative

- **Two files to manage.** Users must understand the layering. Mitigation: `config show` displays the effective merged result; `config path` shows where to look.
- **Minimal parser limitations.** The YAML subset doesn't handle anchors, multi-line values, or complex nesting. Mitigation: the config format is intentionally simple; complex configuration belongs in way files or sensor scripts, not in config.yaml.

### Neutral

- **attend already implements this.** This ADR documents the pattern for workspace-wide adoption. No changes to attend's existing implementation.
- **ways adoption is deferred.** This ADR defines the target; a follow-up implements it in ways when the need arises.

## References

- **attend config implementation**: `tools/attend/src/config.rs`
- **attend ADR**: [ADR-113](./ADR-113-attend-active-awareness-module.md) — config section documents attend's implementation
- **XDG separation**: project memory `xdg-separation.md`
