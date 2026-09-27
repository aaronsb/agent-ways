---
status: Superseded
superseded_by: ADR-169
date: 2026-07-01
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-169]]"
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

Four assets now sit unused by any conversational layer:

1. **Claude Code already understands its own configuration.** It ships with
   context and skills that know what `statusLine`, `permissions`, `hooks`, and the
   ~90 settings keys *mean* and how to configure itself. Re-teaching that would be
   waste and drift.
2. **We have a structured schema** — the vendored 84-key schema with types and
   descriptions (ADR-147) — the *spine* for a guided authoring flow.
3. **Ways carries operator telemetry** — firing stats, near-miss logging
   (ADR-134), a `permissions audit`, governance/provenance. This is a record of
   *learned behavior*: what the operator actually does, repeatedly, by hand.
4. **Claude Code already analyzes the operator's usage.** The `/insights` command
   writes a report (`~/.claude/usage-data/report-<timestamp>.html`) from the last
   30 days of local sessions, and its sections are *already configuration
   recommendations*: **"Where Things Go Wrong"** (friction), **"Suggested CLAUDE.md
   Additions"**, **"Existing CC Features to Try"**, **"How You Use Claude Code"**.
   It is Claude Code pre-computing the very suggestions this skill wants to make.

Nothing composes these into an authoring experience. A user still hand-writes
fragments. The mechanism exists; the **conductor** does not.

## Decision

Build a **skill** that interviews the operator to author configuration, then
drives the `ways settings` primitives to lint, compile, and project it. The skill
is the intelligence; the CLI keeps the guarantees.

**Core thesis — synthesize, don't rebuild.** The skill does *not* re-implement
Claude Code's knowledge of its own settings or its usage analysis. It **composes
four sources**:

- Claude Code's own config self-knowledge (what a key is *for*, sensible values);
- our schema (the authoritative key set, types, and descriptions — the interview's
  spine, and what keeps suggestions valid by construction);
- ways telemetry (what the operator repeatedly grants/does — the raw material for
  *suggestions*);
- the **`/insights` report** — Claude Code's own analysis of the operator's last 30
  days (friction, pre-suggested CLAUDE.md rules, features to try).

The result is a management system more capable than any one alone: CC's
understanding *and its usage analysis*, made **composable, inspectable, lintable,
and projectable** by the ADR-147 substrate, and **informed by the operator's own
history**.

**On `/insights`: read it, don't parse it.** The report is HTML, not JSON — and a
brittle HTML *parser* would be the wrong dependency. Our consumer is a language
model: the skill **reads the latest report in-context** and lets Claude interpret
it the way a human would, focusing on the config-bearing sections. This sidesteps
the fragility of screen-scraping a format that may change, and it *is* the
synthesize-with-CC thesis at its purest — Claude Code generates the analysis, Claude
reads it, our schema turns what matters into valid fragments. Constraint: a skill
cannot invoke a slash command, so it consumes the existing report (noting its age)
and asks the operator to run `/insights` when the report is stale or absent.

**Shape.** A skill (not a way, not a slash command — per the Skills Way, it *runs
a procedure*) whose sub-functions map onto the primitives:

| Sub-function | Drives |
|---|---|
| interview / author | `ways settings new` + fills the value **and the body rationale** from the conversation |
| check | `ways settings lint` |
| rebuild | `ways settings compile` |
| project | `ways settings project` |
| pull-schema | `ways settings schema --refresh` |
| suggest *(v2)* | read the latest `/insights` report + mine ways telemetry (`permissions audit`, firing stats) → propose fragments |

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
- **Ship an HTML parser for `/insights`** (cheerio-style, as community tools do).
  Rejected: brittle screen-scraping of an unsupported, changeable format. Our
  consumer is a language model that reads HTML natively, so the skill reads the
  report in-context instead — more robust *and* less code.
- **Use `/usage` or an OpenTelemetry exporter** for usage data instead of
  `/insights`. Noted as complements, not replacements: `/usage` is tokens/cost (not
  config-shaped), and OTel is structured but high-friction to stand up. `/insights`
  already emits *config-shaped* recommendations, which is why it's the primary v2
  usage source.

## Open Questions

- **Trigger surface** — the exact `description` phrasing and "not for" clause that
  fires on "help me configure Claude Code" without hijacking adjacent requests.
- **Telemetry read contract (v2)** — which sources (`permissions audit`, firing
  stats, governance) and in what shape the `suggest` function consumes them.
- **`/insights` freshness (v2)** — the skill reads the newest
  `~/.claude/usage-data/report-*.html`; how stale is too stale before it should ask
  the operator to re-run `/insights`, and how it maps the report's sections
  ("Suggested CLAUDE.md Additions", "Where Things Go Wrong") onto *settings*
  fragments vs. CLAUDE.md memory (which is a different surface).
- **Store bootstrapping** — does the skill scaffold an empty store on first run,
  and where (the ADR-147 default `$XDG_CONFIG/agent-ways/settings/`)?
