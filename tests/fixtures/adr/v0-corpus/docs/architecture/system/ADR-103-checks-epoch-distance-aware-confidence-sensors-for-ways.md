---
status: Accepted
date: 2026-03-13
deciders:
  - aaronsb
  - claude
related:
  - ADR-004
  - ADR-013
  - ADR-014
---

# ADR-103: Checks — Epoch-Distance-Aware Confidence Sensors for Ways

## Context

Ways inject contextual guidance when a domain becomes relevant — triggered by keywords, tool use, or state conditions. This guidance fires once per session and decays in attentional influence as the conversation progresses (more tokens accumulate between the injection point and the current generation step, reducing relative positional weight via RoPE decay).

The problem: when Claude acts on assumptions within a domain, there is no mechanism to verify those assumptions before committing. Ways tell Claude *how to approach* work; nothing tells Claude *what to check* before acting. This produces "four-foot circle" failures — confident action based on interpolated knowledge rather than verified understanding.

The frozen model problem compounds this: Claude cannot develop calibrated uncertainty through experience. Unlike a human apprentice who learns "I should check before cutting," Claude has uniform confidence across known facts and confabulations. External scaffolding must compensate.

The existing permission-seeking pattern ("Want me to proceed?") is noise — it asks for *permission* rather than surfacing *assumptions*. The useful question is not "may I act?" but "did I check?"

### Observations driving this design

1. **Ways decay.** A way injected 20 turns ago has weak attentional influence on current generation. Re-anchoring is valuable when the context has drifted.

2. **Each domain has different assumptions to verify.** A generic "check your work" directive is useless. Architecture checks are different from deployment checks are different from security checks. The sensor must be coupled to its domain.

3. **Repeated nudging has diminishing returns.** A check that fires every turn becomes noise — the same failure mode as permission-seeking. Successive firings must become progressively harder.

4. **Context budget matters.** Injecting a full re-anchor when the way is still warm wastes tokens. Injecting a light check when the way is cold wastes an opportunity.

### Landscape analysis: why no existing primitive covers this

Claude Code's native extension points were evaluated:

| Primitive | What it does | Why it's insufficient |
|-----------|-------------|----------------------|
| **PreToolUse hooks** | Block, modify input, inject additionalContext | Binary fire/don't-fire — no scoring curve, no distance awareness, no decay |
| **Ways** | Domain-coupled guidance, once per session | Fire-and-forget — no re-anchoring, no verification at action time |
| **Skills** | Intent-driven multi-step workflows | Heavyweight, user/Claude-initiated — not automatic pre-action sensors |
| **CLAUDE.md / rules/** | Static project instructions | Front-loaded, no timing control, maximum positional distance from action |
| **Permissions** | Allow/deny tool access | Access control, not contextual verification |
| **Agents / Subagents** | Delegated personas with tool restrictions | Scoping mechanism, not confidence checking |

The gap: **no native primitive provides adaptive, domain-coupled, decay-modulated context injection at the moment of action.** PreToolUse hooks provide the infrastructure (we already use them); checks add the scoring model that makes injection context-aware.

Checks are a **subclass of ways**, not a new primitive category. They share the same ecosystem (directories, frontmatter, matching engine, hook infrastructure) but have different firing semantics.

## Decision

Introduce **checks** as a second file class within way directories. A `check.md` sits alongside `way.md` in the same directory, coupled by domain but decoupled by timing and trigger model.

### File structure

```
ways/{domain}/{wayname}/
  way.md        # directive: how to approach (fires on domain entry)
  check.md      # sensor: what to verify (fires before action)
```

### Trigger model

Checks fire on **PreToolUse** (before Edit, Write, Bash) — the moment Claude is about to commit to an action. Ways fire on **UserPromptSubmit** or tool-pattern match — the moment a domain becomes relevant. These are different hook events with natural temporal separation.

### Epoch counter

A monotonic counter increments on every hook event within a session. When a way fires, its epoch is stamped. When a check evaluates, the distance from the parent way's epoch is available.

```bash
# /tmp/.claude-epoch-{session_id}        — current epoch (integer)
# /tmp/.claude-way-epoch-{way}-{session} — epoch when way fired
# /tmp/.claude-check-fires-{check}-{session} — check fire count
```

### Scoring curve

Checks use the same BM25/semantic matching as ways, but the raw match score is modulated by two contextual factors:

```
effective_score = match_score × distance_factor × decay_factor
```

Where:

- **match_score** — BM25 or gzip NCD score against current tool input / description
- **distance_factor** = `ln(min(epoch_distance, 30) + 1) + 1` — grows sublinearly with distance from parent way, **capped at 30** to prevent score explosion when the way hasn't fired or is very distant. Max multiplier: ~4.4×.
- **decay_factor** = `1 / (fire_count + 1)` — shrinks with each successive check fire in the session. Diminishing returns on repeated nudges.

The check fires if `effective_score >= threshold` (threshold set in check.md frontmatter, same as ways).

### Behavioral properties of the curve

| Scenario | Distance | Fires | Effective multiplier | Behavior |
|----------|----------|-------|---------------------|----------|
| Way just fired | 0 | 0 | 1.0 | Barely fires — way is warm |
| Some work done | 5 | 0 | 2.8 | Fires easily — re-anchor valuable |
| Deep in session | 20 | 0 | 3.3 | Fires — way is cold |
| Deep, nagged twice | 20 | 2 | 1.1 | Barely fires — diminishing returns |
| Deep, nagged 4x | 20 | 4 | 0.66 | Doesn't fire — stops nagging |

### Check fires before way

If a check's effective score exceeds threshold but the parent way has *not* fired this session, inject the way alongside the check. The way's context is needed to make the check meaningful. Distance is treated as maximum (way is infinitely cold).

### check.md format

```yaml
---
description: what this check verifies (for semantic matching)
vocabulary: domain terms for matching
threshold: 2.0
scope: agent
---
```

Body contains two sections, selected by the loader based on epoch distance:

```markdown
## anchor
[1-2 line re-anchor to parent way's intent — injected when distance is large]

## check
[the actual verification questions — always injected]
```

A distance threshold (e.g., epoch_distance < 5) determines whether the anchor section is included or omitted.

### Differences from ways

| Property | way.md | check.md |
|----------|--------|----------|
| Fires per session | Once (idempotent) | Multiple (with decay) |
| Trigger phase | UserPromptSubmit, PreToolUse | PreToolUse only |
| Scoring | match_score vs threshold | match_score × distance × decay vs threshold |
| Purpose | Directive (how to approach) | Sensor (what to verify) |
| State tracked | Fired yes/no (marker file) | Fire count + parent way epoch |

### Stats and observability

Check firings are logged to the same `events.jsonl` as way firings, with additional fields:

```json
{
  "event": "check_fired",
  "check": "softwaredev/architecture/design",
  "domain": "softwaredev",
  "trigger": "semantic",
  "epoch": 17,
  "way_epoch": 4,
  "distance": 13,
  "fire_count": 1,
  "match_score": 2.4,
  "effective_score": 3.1,
  "anchored": true,
  "scope": "agent",
  "project": "/home/aaron/myproject",
  "session": "abc-123"
}
```

This enables:
- Tracking check fire frequency per domain (are some checks too noisy?)
- Measuring average epoch distance at fire time (is the curve well-calibrated?)
- Comparing anchor vs non-anchor fires (is re-anchoring happening at the right distance?)
- Correlating check fires with session outcomes (do checked sessions produce fewer corrections?)

The `stats.sh` tool and `/ways-tests` evaluation harness are extended to report on checks alongside ways.

### Authoring and evaluation

The `/ways` authoring skill and `/ways-tests` evaluation harness are updated to support checks:

**Authoring** — The ways scaffolding wizard gains a check template option. When creating a new way, the author can optionally scaffold a paired check.md. Guidance includes:
- Keep checks short (3-5 verification questions)
- Anchor section should be 1-2 lines that semantically bridge to the parent way
- Vocabulary should overlap with but be narrower than the parent way's vocabulary (checks are more specific)
- Threshold tuning: start at parent way's threshold, adjust based on observed fire rate

**Evaluation** — The ways-tests harness gains check-specific test cases:
- Verify check fires after parent way (temporal ordering)
- Verify decay curve behavior (fire count reduces effective score)
- Verify anchor inclusion at distance (epoch distance > threshold includes anchor)
- Verify check-before-way pulls in parent way
- Measure false positive rate (checks firing on irrelevant tool actions)

## Consequences

### Positive

- Compensates for frozen model's inability to develop calibrated uncertainty
- Domain-coupled verification — each check knows what's relevant for its domain
- Self-limiting via decay — prevents check-as-noise failure mode
- Context-budget-aware — light injection when way is warm, full re-anchor when cold
- No new trigger infrastructure — uses existing PreToolUse hooks
- Empirically tunable — the curve constants can be adjusted based on observed behavior
- Rich observability — epoch distance, fire count, effective score all logged

### Negative

- Adds a second file class to the ways system (more to maintain)
- Epoch counter adds one file read/write per hook event
- Scoring curve introduces floating-point math (awk dependency in bash)
- Risk of over-engineering if checks proliferate without discipline
- Authoring and evaluation tooling must be updated

### Neutral

- check.md files are optional — ways without checks behave exactly as before
- The epoch counter is useful beyond checks (could inform other distance-aware behaviors)
- Opens the question of whether a third file class will be needed (we explicitly defer this — two classes until proven otherwise)
- Stats collection grows but remains append-only JSONL (same infrastructure)

## Alternatives Considered

- **Native PreToolUse hooks alone** — Already in use. Checks build on top of this infrastructure. The gap isn't the hook mechanism but the scoring model — native hooks have no concept of contextual distance or firing decay.

- **Section within way.md** — Rejected because it prevents separate injection timing. The whole point is that checks fire at a different moment (pre-action) than ways (domain entry). A single file means both inject together, wasting context budget.

- **Generic "check your assumptions" directive** — Rejected because it's domain-unaware. A generic check is just another verbose system prompt that gets ignored. The value is in domain-specific verification coupled to the domain's way.

- **Fixed-interval firing (every N epochs)** — Rejected because it's a timer, not a sensor. Checks should fire based on match relevance modulated by context, not on a clock. A timer would fire during irrelevant actions and miss relevant ones.

- **No decay (fire every match)** — Rejected because repeated nudging becomes permission-seeking noise. The decay factor is what distinguishes checks from the "Want me to proceed?" anti-pattern.

- **Confidence introspection directive** — Rejected because Claude cannot reliably introspect on confidence. The model's self-reported uncertainty is generated text, not measured signal. Empirical checking (use a sensor / run a test) is more reliable than asking the model to evaluate its own certainty.

- **Skills-based approach** — Rejected because skills are intent-driven and user/Claude-initiated. Checks must fire automatically at the pre-action moment without anyone remembering to invoke them. Skills also lack the decay model.

## Implementation Plan

1. **Framework** — `epoch.sh` (counter), `show-check.sh` (loader with curve), wire into `check-file-pre.sh` and `check-bash-pre.sh`
2. **Prototype check** — `architecture/design/check.md` as first test case
3. **Stats** — Extend `log-event.sh` calls and `stats.sh` reporting for check events
4. **Authoring** — Update `/ways` skill with check template and guidance
5. **Evaluation** — Update `/ways-tests` with check-specific test fixtures and assertions
6. **Observe** — Run for several sessions, tune curve constants based on logged data
