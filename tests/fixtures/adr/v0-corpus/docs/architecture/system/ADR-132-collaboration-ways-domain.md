---
status: Accepted
date: 2026-06-04
deciders:
  - aaronsb
  - claude
related: []
---

# ADR-132: Collaboration ways domain

## Context

Ways are organized into top-level domains (`meta`, `softwaredev`, `ea`, …). A domain is **not** a matching mechanism — disclosure is driven by frontmatter (`description`/`vocabulary`) and triggers (`pattern`/`files`/`commands`/state). The directory governs two other things: **organization** and the **include/exclude unit** (`ways.json` disabled domains, project-scope toggles per ADR-131). So a domain boundary is a low-risk, reversible choice — but it is the unit users reason about and toggle, so it's worth drawing on a real seam.

A new capability surfaced the gap: a way for *when to publish a repo's onboarding guide as a share link a teammate's own agent opens directly*. It had no natural home. Looking for one exposed that **collaboration concerns are scattered or mis-filed under `meta`**:

- `meta/teams` — coordination norms for agents working in a team.
- `meta/trust` — the relational model between Claude and the human.
- `meta/subagents` — delegating work to ephemeral helpers.

`meta` is meant for *how the agent itself operates* (knowledge, memory, reasoning, persistence). "Working across the boundary to other people and their agents" is a distinct concern, and `meta` was drifting into a catch-all for it.

## Decision

Create a top-level **`collaboration`** ways domain: ways about working across the boundary to other people and their agents — capabilities and norms that exist *because the collaborators are agent-mediated*.

Initial members:

- `collaboration/onboarding-share` — new; when to surface publishing an onboarding guide as a teammate-openable share link.
- `collaboration/teams` — moved from `meta/teams` (agent-team coordination norms).

**Explicitly kept in `meta`, with rationale** (so the boundary is enforceable, not aspirational):

- `meta/trust` — a *foundational* model other domains derive from (5 referrers: `ea`, `autonomy`, `delegation`, `onboarding-share`). A root concept, not a collaboration occupant.
- `meta/subagents` — execution/parallelization (referenced by `delivery/implement`). "How work gets done," not "collaborating with peers."

**Deferred:** `meta/attend`. Its children mix concerns — peer-session awareness is collaboration, but `context-pressure` is a *solo* signal. That's a per-child split, not a move; out of scope here.

**Naming:** chose `collaboration` over `agentic`. Every way is agent-run, so `agentic` fails to discriminate and invites junk-drawer growth. For the same reason, members sit flat (`collaboration/onboarding-share`) rather than under a redundant `collaboration/agentic/` layer.

## Consequences

### Positive

- A coherent home for a growing class of cross-agent collaboration ways; `onboarding-share` lands cleanly.
- `meta` narrows back toward "how the agent itself operates," reducing catch-all drift.
- Collaboration ways become a single include/exclude unit, toggleable as a group per project (ADR-131).

### Negative

- One more top-level domain to keep coherent — the boundary must be enforced or it becomes a different catch-all.
- Moving `teams` carries a one-time doc-reconciliation tail (done: 3 live docs updated; legacy ADR-013 left pointing at the old path as a historical record).
- Locale-alias translations and the embedding corpus for the new/moved ways must be regenerated; until then `onboarding-share` matches by `pattern:` only.

### Neutral

- No disclosure or behavior change — directory is organization + toggle unit, not a matching input. `teams` still fires identically (`session-start`, `scope: teammate`).
- Future platform sharing/handoff capabilities now have an obvious destination.

## Alternatives Considered

- **Keep `onboarding-share` in `meta` (e.g. `meta/handoff`)** — rejected: `meta` is already absorbing collaboration concerns; the point is to stop that drift, not extend it.
- **Name the domain `agentic`** — rejected: too broad to discriminate (all ways are agentic); a junk-drawer waiting to happen.
- **Sub-group as `collaboration/agentic/…`** — rejected: redundant nesting; the whole domain is agent-mediated.
- **Move `trust`/`subagents`/`attend` in too** — rejected/deferred: `trust` is foundational and heavily referenced, `subagents` is execution, `attend` mixes solo and collaboration signals (needs a split, not a move).
