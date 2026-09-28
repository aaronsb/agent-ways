---
status: Accepted
date: 2026-07-01
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-142]]"
  - "[[ADR-143]]"
  - "[[ADR-106]]"
---

# ADR-148: framework surface ships operator content; dev harness in project scope

## Context

The projected framework surface — the roots the reconciler symlinks into every
operator's `~/.claude` (`skills/`, `agents/`, `commands/`, `hooks/ways/`, ADR-142) —
is the product. Everything in it reaches every install. That surface must therefore
carry **operator** content only: guidance and tools useful to someone who adopted
agent-ways to steer Claude in *their* project.

It had leaked. Two artifacts — the `project-pulse` skill and the `meta/project-health`
way — are **maintainer tools for developing agent-ways itself**: track Claude Code
upstream releases against agent-ways' commits, reconcile agent-ways' own ADRs against
its shipped code. An operator has no use for either, yet both sat in projected roots
(`skills/`, `hooks/ways/`), so they symlinked into every install. The backing tool,
`scripts/project-pulse`, correctly lived in non-projected `scripts/` — but the skill
and way that fronted it did not.

The tell was in the old gate. `project-health` fired on `when: project: ~/.claude`.
Pre-1.0 that *meant* "only when developing agent-ways," because `~/.claude` **was**
the repo then. 1.0 made `~/.claude` a projection (ADR-142), so the gate pointed at
the wrong place and the way went dead — but its intent was always dev-only. It was
never operator content; it was dev harness with a dev gate the projection model broke.

A second pre-1.0 fossil blocked the clean fix. The repo's `.gitignore` used a
deny-all-then-allowlist posture (`*`, then `!` each tracked path) — necessary when
the repo *was* an in-place overlay on a live Claude Code install and had to avoid
committing the install's runtime state. That posture made the repo's own project
scope (`<checkout>/.claude/`) untrackable, so dev harness had nowhere tracked-and-
non-projected to live.

## Decision

**The projected framework surface ships operator content only. agent-ways' own
dev-harness artifacts live in the repo's project scope** — `.claude/ways/` and
`.claude/skills/` — tracked and non-projected.

This is the placement test for any new skill/way/command: *would an operator who
adopted agent-ways use this, or is it for developing agent-ways itself?* Operator
content goes in the projected roots; agent-ways' own tooling goes in the repo's
project scope, exactly where any *other* project's dev guidance would live (ADR-143's
project tier). agent-ways thereby dogfoods its own three-root design — these are its
first project-scoped artifacts.

Concretely:
- `skills/project-pulse/` → `.claude/skills/project-pulse/`
- `hooks/ways/meta/project-health/` → `.claude/ways/meta/project-health/`
- `scripts/project-pulse` stays put (already non-projected; unchanged).
- The `project-health` way drops its `when: project:` gate entirely — a project-
  scoped way fires only in its own project by construction, so no gate (and no new
  matcher condition) is needed.

Enabling change: the `.gitignore` converts from the pre-1.0 deny-all overlay to a
conventional "track by default, ignore build output and runtime junk" list, which is
correct now that the repo is a normal application (ADR-142) rather than an overlay on
a CC install. `.claude/` runtime state stays ignored; `.claude/ways/` and
`.claude/skills/` are un-ignored so the dev harness is tracked and shared.

## Consequences

### Positive

- Operator installs stop carrying maintainer tooling they can't use — the product
  surface is honestly operator-only.
- agent-ways dogfoods ADR-143's project tier, which is a working proof of the
  three-root design (and directly relevant to the adopter story).
- A durable, one-question placement rule for contributors, so this class of leak
  doesn't recur.
- The broken `project: ~/.claude` gate disappears rather than being re-plumbed — and
  with it the need for a new `when: git_remote:` matcher condition that existed only
  to re-gate this one way.

### Negative / caveats

- A project-scoped *semantic* way (like `project-health`) only fires once the project
  corpus includes project-local ways; in the dev checkout that may need a `ways corpus`
  regen. The *skill* is discovered natively regardless, so the capability the dev
  actually reaches for (`/project-pulse`) works immediately.
- The conventional `.gitignore` is "risky by default" (junk creeps in unless ignored)
  where the deny-all was "safe by default." Mitigated by an explicit ignore list for
  every build-output and runtime-junk category, verified against the tracked baseline.

### Neutral

- `scripts/project-pulse` is unchanged — the capability was never the problem, only
  the surface its skill/way sat on.

## Alternatives Considered

- **Delete the skill and way outright.** Rejected: the capability is genuinely useful
  for developing agent-ways (tracking Claude Code changes, ADR reconciliation). The
  problem was placement, not existence — relocate, don't destroy.
- **Generalize `project-pulse` into an adopter-facing tool** (parameterize the upstream
  repo so any project can track its own upstream). Rejected as the framing here: it's a
  dev harness, not a half-built product feature. Generalizing it would be dressing up a
  maintenance tool as something operators want. If a genuine adopter-facing "project
  pulse" is ever wanted, that's its own decision with its own design — not a reason to
  keep this one in the shipped surface.
- **Re-gate the way with a new `when: git_remote:` matcher condition.** Rejected:
  building a matcher feature to keep one dev way firing in the projected surface is
  backwards. Project scope *is* the gate; the feature isn't needed.
- **Add targeted `.gitignore` negations for `.claude/ways` / `.claude/skills` only,
  keeping the deny-all.** Rejected: it preserves the pre-1.0 fossil and adds more
  allowlist cruft. Converting to conventional fixes the root cause once.
