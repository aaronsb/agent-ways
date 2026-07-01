---
status: Draft
date: 2026-07-01
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-142]]"
  - "[[ADR-143]]"
  - "[[ADR-145]]"
---

# ADR-147: Composable settings.json — a store of YAML config fragments

## Context

Claude Code's configuration surface is large — ~90 `settings.json` keys (many
version-gated), plus file artifacts (skills, agents, commands, hooks, statusline) and
MCP servers. Today a user manages all of it as a single hand-edited
`~/.claude/settings.json` and a scattering of files: no composition, no lint, no
rationale, no lifecycle. The one *management* surface Anthropic ships — the enterprise
**managed settings** console — is a raw `settings.json` textarea whose entire safety
story is one sentence: *"Invalid settings may disable Claude Code for your
organization."* It is a deploy backstop, not an authoring workflow.

Three observations shape the decision:

1. **Claude Code already ships the composable pattern — but only for managed scope.**
   File-based managed settings support a `managed-settings.d/*.json` drop-in directory:
   numbered fragments, merged in filename order, with a *defined* merge law. That is
   the conf.d pattern — and it exists for the *enterprise* layer and nowhere else. User
   and project scope are each a single file.

2. **The merge law is a spec, not something to invent.** Claude Code documents exactly
   how settings merge: most keys override by precedence, `permissions.allow/deny/ask`
   **concatenate + deduplicate**, `env`/objects deep-merge. A compiler mirrors this
   table; it does not design new semantics.

3. **This shape is proven prior art.** A tree of markdown files with YAML frontmatter —
   structured fields up top, human-readable body below — is exactly agent-ways' own
   ways format, the Open Knowledge Format (Google, 2026), and this author's `kg-fuse`
   knowledge store. The format demonstrably scales to typed graphs and epistemic
   metadata in those richer systems. We adopt only the **node shape**; the graph,
   content-addressing, and query machinery those systems carry solve problems
   `settings.json` does not have (see Non-goals).

This refactors the earlier ADR-147 draft ("projectable user config layer"), which
framed the same idea too small — as a sync mechanism. The projection/capture it
described survives here as the final pipeline stage.

## Decision

Manage Claude Code configuration as a **store of composable YAML config fragments**: a
tree of markdown files with YAML frontmatter, compiled deterministically into
`settings.json` and projected by the existing reconciler.

### The unit

One file per concern. Frontmatter carries a `settings:` block — a `settings.json`
fragment expressed in YAML — plus minimal meta; the body carries the rationale.

```markdown
---
scope: user            # user | project | managed
mandatory: false       # org-lock (managed scope only)
settings:              # a settings.json fragment, in YAML
  permissions:
    allow: ["Bash(git:*)", "Bash(gh:*)"]
    deny:  ["Bash(rm -rf *)"]
---
# Git & GitHub permissions
Let Claude run git/gh unprompted — constant use, prompts are pure friction.
`rm -rf` stays denied: destructive, never worth auto-allowing.
```

- `settings:` is a literal `settings.json` fragment in YAML — the readable spelling of
  the JSON, mapping 1:1.
- The body is the *why* — the thing the console's textarea can never hold. `git blame`
  on `permissions/git.md` answers "who allowed this, and why."
- Ordering is filename prefix (`10-permissions.md`, `20-hooks.md`) — the same
  convention as `managed-settings.d` and a modular shellrc.

### The pipeline

`author → lint → compile → project`

- **lint** — three flat, deterministic checks: (a) *schema-valid* (key exists, right
  type) against Claude Code's settings schema; (b) *scope-legal* (no managed-only key
  authored at user/project scope); (c) *duplicate-scalar* (two fragments set `model` →
  warn; last-wins resolves it). This catches the disable-Claude-Code footgun *locally,
  at author time* — the thing the console cannot do.
- **compile** — deep-merge every fragment's `settings:` block in filename order,
  applying Claude Code's documented merge law (a fixed per-key lookup, not a merge
  engine). Emit a baked `settings.json` and a `key → source-fragment` provenance
  manifest.
- **project** — the existing reconciler (ADR-144/145) materializes the compiled output
  into `~/.claude` and three-way-merges `settings.json`, using the provenance manifest
  as its base (the same base hardened in the ADR-145 settings-merge work).

### Independence and the shape contract

*Managing* the configuration is independent of the ways matching engine — someone can
use the fragment store with zero interest in ways. The **contract** between the store
and its consumers is the compiled output: a baked `settings.json` (or, for org scope,
`managed-settings.d/*.json` fragments Claude Code merges natively) plus the provenance
manifest. Consumers are swappable: the reconciler projects it into `~/.claude`; an
enterprise console receives it as a *deploy target*; a plain viewer browses the tree.

## Non-goals (the discipline)

Complexity is bounded to what `settings.json` needs. Explicitly **not** built:

- **No typed dependency graph / topological compile.** `settings.json` precedence is
  *linear* (filename order / last-wins). "Conflict" is a duplicate-scalar warning, not
  cycle detection.
- **No content-addressed store.** A plain `key → fragment` manifest is enough provenance
  for capture and diffing.
- **No epistemic layer** (grounding, contradiction scoring). Config is declarative fact,
  not uncertain knowledge.
- **No semantic query / FUSE projection.** Config is *composed*, not *queried*.

The prior art (ways, kg-fuse, OKF) proves the format *scales* to these if a future need
appears; none is a need for `settings.json` today.

Initially scoped to `settings.json`. File artifacts (skills/agents/commands/hooks) and
MCP (`~/.claude.json`) already project through the reconciler and stay there; the same
fragment pattern extends to them later if wanted.

## Consequences

### Positive

- Config becomes **literate, greppable, git-native** — the structure of JSON, the
  readability of YAML, with the rationale attached and `git blame` for provenance.
- Deterministic lint catches invalid/scope-illegal settings **before** they reach
  `~/.claude` — strictly better than the console's "may disable Claude Code."
- A vetted, tested, compiled `settings.json` can be **copied into an enterprise
  console** — the console as a deploy target, authoring done as a git-managed factory.
- Fixes the `statusLine`-class bug at the root: the framework stops force-claiming
  user-scoped keys; a user's own config lives in fragments the reconciler projects,
  rather than being orphaned or clobbered.
- Reuses the ways/OKF node shape the project already tools and understands.

### Negative

- A **compile step** sits between editing a fragment and it taking effect (edit →
  compile → project), versus editing `settings.json` directly.
- A **second faithful implementation of Claude Code's merge law**, which drifts as the
  settings schema evolves (version-gated keys). Mitigated by tracking the settings docs
  via project-pulse (a schema-freshness feed) — but it is a maintenance surface.
- Authoring YAML fragments has a small learning curve over a single JSON file.

### Neutral

- Depends on the reconciler and its provenance base (ADR-144/145); extends them rather
  than inventing machinery.
- The reverse direction (**capture** — classify a live `settings.json` back into
  fragments) is enabled and worth pursuing, but deferred. It is seeded by Claude Code's
  known in-situ writers (`advisorModel`, `effortLevel`) and the managed-only key list.

## Alternatives Considered

- **Leave user config untouched (status quo).** Rejected: drops the value (portable,
  versioned, testable config) and leaves the `statusLine`-class bug.
- **Plain numbered JSON fragments (`managed-settings.d` style), no bodies.** The minimum
  floor, zero new format. Rejected as the primary shape only because it cannot carry
  rationale — but it remains the fallback if the markdown shape proves too heavy.
- **Full OKF/kg-fuse machinery** (typed DAG, content-addressing, epistemic layer,
  semantic query). Rejected as over-complexity for a JSON file with one merge quirk;
  deferred to if-ever (see Non-goals).
- **A standalone dotfiles tool.** Rejected: the reconciler already projects and merges
  `settings.json`; the fragment store is the missing *authoring* layer, not a second
  projector.
- **Rely on the enterprise managed-settings console.** Rejected as the authoring
  surface: no lint, no lifecycle, no rationale, no composition. It is a deploy target
  the factory *feeds*, not a place to author.

## Open Questions

- **Store location** — `$XDG_CONFIG_HOME/agent-ways/config/` (mirroring the user-ways
  root, ADR-143) vs. a dedicated repo an org can share.
- **Compile target for managed scope** — pre-merge to one file vs. emit
  `managed-settings.d/*.json` and delegate the merge to Claude Code (leaning: delegate,
  it is less code and uses CC's own semantics).
- **Test depth** — static only (schema/scope/duplicate) for v1 vs. a dynamic gate that
  boots a sandbox Claude Code against the compiled output.
- **Manager/ways boundary** — how much of the lint/compile is its own tool vs. a
  subcommand of `ways`, given the stated independence.
