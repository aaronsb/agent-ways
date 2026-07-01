---
status: Draft
date: 2026-07-01
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-147]]"
  - "[[ADR-134]]"
---

# ADR-149: operator config interview skill

## Context

ADR-147 built the `ways settings` CLI: deterministic primitives for managing
Claude Code's `settings.json` as composable fragments — `lint`, `new` (scaffold
from schema), `schema` (show/refresh), `compile` (merge → baked settings.json +
provenance), `project` (install into the live config). These are **mechanism**:
dumb, testable, no cleverness. They are also, per ADR-147, a *shape contract* — a
surface a separate consumer can drive.

Three assets now sit unused by any conversational layer:

1. **Claude Code already understands its own configuration.** It ships with
   context and skills that know what `statusLine`, `permissions`, `hooks`, and the
   ~90 settings keys *mean* and how to configure itself. Re-teaching that would be
   waste and drift.
2. **We have a structured schema** — the vendored 84-key schema with types and
   descriptions (ADR-147) — the *spine* for a guided authoring flow.
3. **Ways carries operator telemetry** — firing stats, near-miss logging
   (ADR-134), a `permissions audit`, governance/provenance. This is a record of
   *learned behavior*: what the operator actually does, repeatedly, by hand.

Nothing composes these into an authoring experience. A user still hand-writes
fragments. The mechanism exists; the **conductor** does not.

## Decision

Build a **skill** that interviews the operator to author configuration, then
drives the `ways settings` primitives to lint, compile, and project it. The skill
is the intelligence; the CLI keeps the guarantees.

**Core thesis — synthesize, don't rebuild.** The skill does *not* re-implement
Claude Code's knowledge of its own settings. It **composes three sources**:

- Claude Code's own config self-knowledge (what a key is *for*, sensible values);
- our schema (the authoritative key set, types, and descriptions — the interview's
  spine, and what keeps suggestions valid by construction);
- ways telemetry (what the operator repeatedly grants/does — the raw material for
  *suggestions*).

The result is a management system more capable than any of the three alone: CC's
understanding, made **composable, inspectable, lintable, and projectable** by the
ADR-147 substrate, and **informed by the operator's own history**.

**Shape.** A skill (not a way, not a slash command — per the Skills Way, it *runs
a procedure*) whose sub-functions map onto the primitives:

| Sub-function | Drives |
|---|---|
| interview / author | `ways settings new` + fills the value **and the body rationale** from the conversation |
| check | `ways settings lint` |
| rebuild | `ways settings compile` |
| project | `ways settings project` |
| pull-schema | `ways settings schema --refresh` |
| suggest *(v2)* | mine telemetry (`permissions audit`, firing stats) → propose fragments |

**The interview's byproduct is documentation.** The operator's answer to "why do
you want this?" becomes the fragment's markdown body — so `git blame` on the config
answers *who* and *why*, captured at authoring time for free. This is the payoff of
ADR-147's markdown-with-rationale format.

**MVP vs. v2.** MVP is the interview→author→lint→compile→project loop with the
schema + CC self-knowledge. Telemetry-driven `suggest` is v2 (it needs a read
contract against the audit/stats sources) — deferred so the interview lands first.

**Boundaries (Skills Way).** The skill projects into personal scope
(`~/.claude/skills/`), so: a **tight trigger** naming the specific task and the
words an operator says (with an explicit "not for" clause), and
**location-independence** (resolve the target up front, assume no cwd). It leans on
the `ways settings` CLI and `ways` telemetry — it does not reimplement them.

## Consequences

### Positive

- The full author → lint → compile → project loop becomes **conversational**, with
  Claude Code's own config understanding driving it — a materially more capable
  manager than hand-editing JSON or the enterprise console's textarea.
- Rationale is captured at authoring time (the interview *is* the documentation).
- Suggestions (v2) turn passive telemetry into proactive config hygiene ("you keep
  approving this by hand — want a fragment?").
- Realizes ADR-147's independence promise: the skill is a *consumer* of the CLI
  shape contract, swappable and separately versioned.

### Negative

- A conversational surface over live config must be careful: it drives `project`,
  which writes `~/.claude/settings.json`. It inherits `project`'s safety
  (dry-run/backup) but adds a trust surface (the skill proposing changes).
- Telemetry `suggest` (v2) couples the skill to internal ways data shapes — a
  maintenance surface, deferred deliberately.
- A global skill with a loose trigger would hijack unrelated requests; the trigger
  discipline is load-bearing, not optional.

### Neutral

- Depends on the ADR-147 primitives (all now built) — extends them, invents no new
  mechanism.
- Composes CC's self-knowledge rather than encoding it, so it tracks CC's evolution
  for free where the schema lags.

## Alternatives Considered

- **A settings GUI / TUI.** Rejected: rebuilds interaction Claude Code already does
  conversationally, and can't leverage CC's self-knowledge or the operator's
  history the way an in-session skill can.
- **Re-encode CC's settings knowledge in ways.** Rejected as the core mistake this
  ADR avoids — waste, and guaranteed drift against a surface that changes often.
- **A way (hook-injected guidance) instead of a skill.** Rejected per the Skills
  Way: this *runs a multi-step procedure* (interview → CLI calls), which is a
  skill; a way shapes behavior, it doesn't execute a workflow.
- **Fold the orchestration into the `ways` binary** (a `ways settings interview`
  subcommand). Rejected: the intelligence is conversational and model-driven, which
  is exactly what a skill is for; the binary stays the deterministic mechanism.

## Open Questions

- **Trigger surface** — the exact `description` phrasing and "not for" clause that
  fires on "help me configure Claude Code" without hijacking adjacent requests.
- **Telemetry read contract (v2)** — which sources (`permissions audit`, firing
  stats, governance) and in what shape the `suggest` function consumes them.
- **Store bootstrapping** — does the skill scaffold an empty store on first run,
  and where (the ADR-147 default `$XDG_CONFIG/agent-ways/settings/`)?
