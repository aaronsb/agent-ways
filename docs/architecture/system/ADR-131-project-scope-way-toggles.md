---
status: Draft
date: 2026-05-22
deciders:
  - aaronsb
  - claude
related:
  - ADR-115
  - ADR-105
  - ADR-111
---

# ADR-131: Project-scope way toggles

## Context

Ways fire across every project the user opens, but not every way is wanted in every project. A research-heavy repo doesn't need `itops/incident`; a personal scratch project doesn't want `softwaredev/architecture/adr` nagging. Today the only enable/disable knob is `disabled_domains` in user-scope `~/.claude/ways.json` — coarse (domain-level) and global (applies everywhere). The result:

- Authors keep ways generic so they don't annoy users in unrelated projects, which weakens the matchers.
- Users tolerate noise rather than disabling a domain globally, because disabling globally would also kill the way in the one project where it *is* wanted.
- Per-project muting today requires deleting/renaming way files or editing user-scope JSON every time the user switches contexts — neither survives `git pull` and both leak between projects.

ADR-115 introduced the project overlay (`{project}/.claude/ways.yaml`) for tuning thresholds and disclosure parameters, and `config.rs` already loads it. But the overlay has no per-way enable/disable schema, and no CLI surface — users would have to hand-edit YAML to use it.

The need is narrow: *per-way, per-project, defaulted-enabled* toggles, with a CLI ergonomic enough that users actually use them.

## Decision

### Scope

- **Per-way granularity.** Toggles target individual ways by their canonical name (e.g., `itops/incident`, `meta/introspection`), not domains.
- **Project scope only.** No new global disabler — the existing `disabled_domains` in user-scope `ways.json` is retained for backward compat but not extended. Per-way toggles live exclusively in `{project}/.claude/ways.yaml`.
- **Default enabled.** Absence of a toggle means the way fires normally. The config is opt-out, not opt-in. A project that ships no `ways.yaml` behaves exactly as today.

### Schema

Extend the project overlay with a `ways` mapping. Keys are way canonical names; values are `enabled: true|false`:

```yaml
# {project}/.claude/ways.yaml
ways:
  itops/incident:
    enabled: false
  meta/introspection:
    enabled: false
```

Shorthand (boolean value) is also accepted for the disable-only case:

```yaml
ways:
  itops/incident: false
  meta/introspection: false
```

The mapping form is reserved for future per-way knobs (threshold overrides, refire presets) — see Consequences > Neutral.

### CLI

Add two subcommands to the `ways` binary:

```
ways disable <way>      # set ways.<way>.enabled: false in $PROJECT/.claude/ways.yaml
ways enable <way>       # remove the entry (or set enabled: true)
ways disable --list     # show currently disabled ways in this project
```

Both default to project scope. There is no `--global` flag — per the project-scope-only constraint.

Behavior:
- Creates `.claude/ways.yaml` if missing.
- Round-trips comments and unrelated keys via a minimal YAML edit (not a full re-serialize).
- Validates that `<way>` exists in the corpus before writing; warns but still writes if not (allows pre-emptive disable before a way is authored).
- `ways enable <way>` is a no-op if the way isn't currently disabled — exits 0.

### Enforcement

Toggle resolution happens at the same gate as `disabled_domains` today, in `ways scan` (the Rust production firer) and `inject-subagent.sh` (the bash subagent gate). Both consult `config::global().disabled_ways` — a new `Vec<String>` populated from the project overlay during config load.

A disabled way is skipped entirely: no scoring, no disclosure, no marker. It is as if the way did not exist for this session.

### Trust model

Same as the rest of `.claude/ways.yaml`: project-scope config is committed alongside the repo and reviewed like any other source file. Disabling a way is no more privileged than deleting it from the project's local `.claude/ways/` directory.

## Consequences

### Positive

- **Authoring freedom.** Way authors can write sharper triggers without worrying about a project where the way doesn't belong — users can just disable it there.
- **Per-repo discipline.** A repo's `ways.yaml` becomes the canonical record of "which guidance applies here," reviewable in PR and durable across machines.
- **Reversible.** `ways enable` is a single command — no file deletion, no merge conflicts with upstream ways.

### Negative

- **Second place to look** when debugging why a way isn't firing — alongside corpus presence, threshold, refire window. Mitigated by `ways status` surfacing disabled ways and `ways scan --explain` reporting "skipped: disabled in project config."
- **Drift risk.** A way renamed upstream silently stops being disabled. Mitigated by `ways disable --list` warning on entries that don't match the current corpus.

### Neutral

- The `ways:` mapping form (vs. shorthand boolean) is forward-compatible with per-way threshold/refire overrides — those are deliberately out of scope for this ADR but the schema doesn't preclude them.
- Existing `disabled_domains` in user-scope `ways.json` is unchanged. Domain-level disable remains the right tool for "I never want any `ea/*` way to fire anywhere."

## Alternatives Considered

- **Per-way disable in user scope.** Rejected: the whole problem is that user-scope is too broad. A user who wants `itops/incident` muted in project A but active in project B can't express that globally.
- **Reuse `disabled_domains` with deeper paths** (e.g., `itops/incident`). Rejected: domain-level and way-level are conceptually distinct; collapsing them muddles the schema and the existing `disabled_domains` lives in user-scope JSON, not the project YAML.
- **`+/-` overlay syntax from ADR-115.** Considered for symmetry — `ways: [-itops/incident]`. Rejected for the toggle case: `enabled: false` reads more clearly to humans, and the `+/-` form is better reserved for collection-shaped configs (sensors, way groups) where addition is also meaningful. Disable is monotonic; no `+` half is needed.
- **Delete the way file from `~/.claude/hooks/ways/`.** Rejected: that's a global, destructive change that wipes the way for every project and doesn't survive a re-sync.
