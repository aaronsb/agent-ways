---
status: Accepted
date: 2026-04-09
deciders:
  - aaronsb
  - claude
related:
  - ADR-104
  - ADR-105
  - ADR-108
  - ADR-112
  - ADR-113
---

# ADR-114: `attend` Events as an Insistent Way Trigger Type

## Context

[ADR-113](./ADR-113-attend-active-awareness-module.md) introduces `attend`, a sibling binary in the agent-ways workspace that implements the active awareness layer described in the [Cognitive Loop and the Awareness Layer](../../design-notes/cognitive-loop-and-awareness-layer.md) design note. `attend` observes Claude Code session state and environmental signal, tracks approaching mechanical consequences (context pressure, reflection deferral, etc.), and produces emissions that need to reach Claude.

The question this ADR answers is: **how do those emissions become guidance Claude reads?**

### Current trigger types

The ways system supports a small set of trigger types, each keyed to a hook event in Claude Code's loop:

| Trigger | Fires on | Example |
|---|---|---|
| `UserPromptSubmit` | User sends a message | Intent classification ways |
| `PreToolUse` | Before Claude calls a tool | Tool guidance ways |
| `Stop` | After Claude's response | Reflection capture, cleanup |
| `SessionStart` | New session begins | Project pulse (ADR-106), orientation |
| `PostCompact` | After a compaction pass | Resume ways, ADR-112 |
| `context-threshold` | Context crosses a percentage boundary | Reflection, memory distillation (ADR-104, ADR-112) |

Every existing trigger is **reactive**: it keys off an event inside Claude's own loop (Claude did something, or Claude is about to do something, or Claude's context is changing). There is no trigger type for events *outside* Claude's loop — no way for a file change, an idle timeout, a context-pressure projection, or a user-requested timer to reach Claude through the same machinery.

### The delivery primitive: `Monitor`

`attend` produces exactly that class of event: externally-observed signals that occur between hook boundaries and need to surface into Claude's attention. The delivery primitive for those signals is already available in Claude Code — the `Monitor` tool, introduced in the same release wave that makes ADR-113 buildable. `Monitor` accepts a shell command, runs it as a background process, and delivers each line the command writes to stdout as an asynchronous notification in Claude's conversation. With `persistent: true`, a single invocation covers the session's lifetime. This is the mechanism by which `attend`'s stdout emissions reach Claude.

`Monitor` is described in detail in the [design note](../../design-notes/cognitive-loop-and-awareness-layer.md) as the *delivery primitive*. ADR-113 commits `attend` to writing its observations as single-line stdout notifications that `Monitor` delivers. **That is the primary delivery channel for the awareness layer**, and it requires no involvement from the ways system at all.

### Why ways integration still matters

Given that `Monitor` handles delivery, why does this ADR exist? Because there is a second class of `attend` emission — high-salience observations where the notification alone is insufficient and deeper guidance would materially improve Claude's response. Examples:

- `attend` detects context pressure imminent enough that the reflection window is closing. The one-line notification *"projected critical in 3 turns"* tells Claude the stakes, but the *actual reflection guidance* — what to reflect on, how to compress it, what structure to use — lives in a way body that ADR-112 already defined.
- `attend` notices a peer Claude Code session has modified a file this session is editing. The notification surfaces the conflict, but the *coordination pattern* — how to reconcile, whether to rebase, how to communicate — belongs in a peer-coordination way.
- `attend` detects a build failure. The notification says "build failed," but the *triage playbook* belongs in a way.

For these cases, `attend` formats the `Monitor` notification as an **affordance**: a string that explicitly names a `ways show attend/<signal-type>` command Claude can invoke if it wants the deeper guidance. When Claude invokes that command, the ways system runs the matcher and ADR-104 disclosure gate normally, and the matched way's body is injected through the standard guidance pipeline.

This gives the awareness layer **two composable delivery paths**:

1. **`Monitor` notification only** — default for most emissions. Claude reads the one-line observation, integrates it, acts or dismisses. No ways involvement.
2. **`Monitor` notification + affordance → `ways show attend/<signal>`** — for high-salience emissions. Claude reads the notification, recognizes the stakes, invokes the named ways command, receives the full guidance body.

The ways system does not need to know `attend` exists until Claude invokes `ways show attend/<signal>`. At that point, the matcher treats the invocation as it would any other explicit way query, runs ADR-104's disclosure gate normally, and injects the matched way. No new hook event, no new matcher input source, no automatic firing — just a well-formed query Claude chose to make.

## Decision

Extend the way trigger schema with a new type, `attend`, that declares a way as the handler for one or more `attend` signal types. Ways with this trigger are **invoked on demand** when Claude runs `ways show attend/<signal>`, typically in response to an affordance in a `Monitor`-delivered `attend` notification. The ways system runs the matcher and ADR-104 disclosure gate normally for each invocation.

The primary delivery channel for `attend` observations remains `Monitor` (see ADR-113). This ADR adds the secondary on-demand path for cases where a notification alone is insufficient.

### Schema

Ways declaring `trigger.type: attend` use the following frontmatter fields:

```yaml
---
name: reflect-on-context-pressure
description: Guide Claude through progressive ledger reflection as context approaches compaction
trigger:
  type: attend
  signals:
    - context-pressure
    - reflection-overdue
---

(way body — the guidance Claude reads when it invokes `ways show attend/context-pressure`
or `ways show attend/reflection-overdue`)
```

Field definitions:

- **`type: attend`** (required) — Marks this way as a handler for an `attend` signal type. Ways with this trigger are never fired automatically by a hook event; they are invoked on demand by Claude in response to an affordance.
- **`signals`** (required) — List of signal types this way handles. When Claude invokes `ways show attend/<signal-type>`, the ways CLI selects ways whose `signals` list contains the requested signal. A way may handle multiple signals; one signal may be handled by multiple ways (in which case the matcher scores among them as usual).

Standard fields (`name`, `description`, `embed_threshold`, way body, etc.) behave exactly as they do for other trigger types. The way body is the guidance Claude reads when the invocation succeeds and the disclosure gate permits the injection.

Absent from the schema by design: no `subscriptions`, no `debounce_turns`, no `salience_floor`. These were artifacts of an earlier draft that modeled automatic firing. In the Monitor-primary design, habituation is handled by the disclosure gate on each invocation (ADR-104), and salience decisions happen in `attend` before the affordance is ever emitted.

### Invocation via affordance

The full flow for a high-salience emission:

1. `attend` detects a signal worth surfacing (e.g., context pressure crossing a critical threshold)
2. The insistence emitter computes the observation text and determines that a deeper-engagement path is warranted (typically because the emission mode is `insistent` or `critical`)
3. `attend` writes a single notification line to stdout, formatted as an affordance:

   ```
   context at 86% — projected critical at turn 58 (3 turns remaining). Use `ways show attend/context-pressure` for reflection guidance.
   ```

4. `Monitor` delivers the line to Claude as an asynchronous notification
5. Claude reads the notification, recognizes the stakes from the text and projection, and decides whether to invoke the affordance
6. If Claude invokes `ways show attend/context-pressure`:
   - The ways CLI looks up ways with `trigger.type: attend` where `signals` contains `context-pressure`
   - Each candidate is scored through the standard embedding + BM25 + NCD pipeline against the request context
   - ADR-104's disclosure gate checks whether the matched ways have been disclosed recently
   - The matcher returns the surviving way's body as injected guidance
7. Claude receives the guidance and acts on it

At every step, Claude retains agency. `attend` suggests; Claude decides. The ways system provides the guidance only when asked. The disclosure gate ensures habituation applies uniformly regardless of source.

### Emission modes and delivery mapping

ADR-113 defines the emission modes. Each mode maps to a specific delivery pattern in this two-path model:

| Mode | Notification via `Monitor` | Includes affordance? | When Claude invokes the affordance |
|---|---|---|---|
| `silent` | No line emitted | N/A | N/A |
| `informational` | Short declarative observation | No | N/A — informational lines do not warrant deeper engagement |
| `affordance` | Observation + named tool invitation | Yes (optional) | Ways CLI runs matcher + disclosure gate; way fires if eligible |
| `insistent` | Observation + explicit stakes + named ways command | Yes (suggested) | Disclosure gate applies; way fires if eligible; if recently disclosed, terse re-surface |
| `critical` | Maximum-clarity observation + explicit consequence language + ways command | Yes (strongly suggested) | Disclosure gate is more willing to re-fire given the criticality; way injection prioritized |

The escalation between modes is handled entirely by `attend`'s insistence emitter. The ways system responds to invocations the same way regardless of mode — the mode determines how `attend` phrases the notification, not how ways responds to the resulting query.

The "acknowledged-but-silent" state from the design note is implemented in `attend`'s deferred intent store, not the ways system. Observations held below the emission threshold never become `Monitor` notifications, which means the ways system never sees them.

### Unified disclosure and habituation

Each invocation of `ways show attend/<signal>` passes through the same disclosure gate as any other way invocation. The disclosure tracker does not distinguish signal sources; it only tracks which ways have been disclosed and how recently. This means:

- A way that handles an `attend` signal is subject to the same recent-disclosure suppression that applies to reactively-triggered ways
- ADR-104's token-gated re-disclosure rules apply uniformly
- Habituation works the same way: first invocation triggers full guidance, rapid re-invocations are suppressed to terse re-surfacing, extended silence eventually allows re-disclosure at full weight

This uniform treatment is load-bearing. Claude does not need a separate mental model for reactive vs on-demand ways. Authors of `attend` signal handlers don't need to learn new disclosure rules. The underlying system is one system whose invocations sometimes come from hooks and sometimes come from Claude acting on an affordance.

### Way authoring

The author of an `attend` signal handler writes it the same way they write any other way. They identify what deeper guidance Claude should receive when a particular `attend` signal warrants engagement, they write the guidance as the way body, they declare which signals the way handles, and the system handles the rest. No knowledge of `attend` internals is required.

Example:

```yaml
---
name: note-build-completion
description: Guide Claude in responding to a completed background build
trigger:
  type: attend
  signals:
    - build-complete
---

A background build has finished. Consider:

- Running the test suite to validate the build
- Checking output for warnings worth investigating
- Updating the working context with what changed

The user may already be aware via terminal output — only engage with this if
the build result is directly relevant to current work.
```

This way is invoked when Claude runs `ways show attend/build-complete` in response to an `attend` affordance. The author didn't write any sensor code, didn't touch `attend`'s internals, and didn't need to know how the underlying signal is detected. The contract between `attend` and this way is a single string: `build-complete`.

### Graceful no-op when `attend` or `Monitor` is absent

If `attend` is not running, ways with `trigger.type: attend` are **inert, not broken**. No affordances are ever emitted, so `ways show attend/<signal>` is never invoked automatically. The rest of the ways system functions normally. When `attend` starts, affordances begin appearing in notifications and the ways become live.

Equally, if `Monitor` is unavailable (because the running Claude Code version doesn't ship it, or because the SessionStart way that drives invocation was not installed), the awareness layer produces no notifications at all, and `attend` signal handler ways are simply never invoked through the automatic path. Claude could still invoke `ways show attend/<signal>` manually from a prompt, and the ways CLI would handle that correctly — the invocation path is not conditional on `attend` running.

This is the "presence as additive" property from the design note, expressed at the ways layer. An agent-ways installation that never runs `attend` or `Monitor` never experiences any downside from having `trigger.type: attend` ways in the corpus — they are dormant until Claude invokes them, and nothing else breaks.

### Scope of this ADR

This ADR defines:

- The new `trigger.type: attend` schema with the `signals` field
- Invocation via affordance: how `ways show attend/<signal>` integrates with the matcher and ADR-104 disclosure gate
- The mapping from `attend` emission modes to `Monitor` notification format and affordance presence
- Way authoring for signal handler ways
- Graceful inert behavior when `attend` or `Monitor` is absent

It explicitly does not define:

- The `attend` binary's internal architecture (that is ADR-113)
- The `Monitor` tool's behavior (that is Claude Code documentation)
- The sensor catalog or specific signal types (those grow incrementally)
- Any changes to ADR-104's disclosure gate — the gate applies as-is to `ways show` invocations sourced from `attend` affordances

## Consequences

### Positive

- **Minimal surface area.** The ways system gains one new trigger type and one new invocation path (`ways show attend/<signal>`). It does not need to accept emission streams, process salience weights, or implement a second matching pipeline. The integration is one schema extension and one CLI convention.
- **Uniform habituation.** ADR-104's disclosure gate applies to every `ways show attend/<signal>` invocation exactly as it applies to any other way lookup. No special cases. No separate habituation rules for proactive vs reactive ways.
- **Clear separation of concerns.** `attend` + `Monitor` handle observation and delivery. The ways system handles guidance retrieval and disclosure. The boundary is "Claude invoked a `ways show` command with a specific signal name," which is a clean contract both sides can reason about independently.
- **Incremental adoption.** Ways with `trigger.type: attend` can be added to the corpus before `attend` is built — they simply remain dormant until Claude invokes them. Claude can also invoke them manually from a prompt even without `attend` running, which makes the ways immediately useful as signal-shaped handlers regardless of the awareness layer's state.
- **Two delivery paths, one attention surface.** Most `attend` observations arrive as `Monitor` notifications and never touch the ways system. High-salience ones route through ways for deeper guidance. Claude sees one consistent attention surface even though two paths are active underneath.
- **Claude retains agency.** `attend` suggests affordances; Claude decides whether to invoke them. The ways system only produces guidance in response to Claude's explicit request. There is no path by which the awareness layer forces injection.

### Negative

- **Two-path cognitive load for way authors.** Authors writing `trigger.type: attend` ways need to understand that their ways fire on `ways show` invocations from Claude, not on sensor events. The authoring model is simple (`signals: [...]` and a body), but the mental model is new.
- **Affordance format is a contract between `attend` and ways.** If the `ways show attend/<signal>` command name convention changes, both sides need to update. Mitigation: the format is a short documented subset — command name + signal identifier — and versioning it if needed is cheap.
- **Implicit dependency in way corpora.** A corpus with many `trigger.type: attend` ways assumes a source that knows to invoke them. Without `attend`, they are reachable only via explicit user or Claude invocation. Mitigation: the ways are documented as "dormant without `attend`" and do not fail when `attend` is absent; reactive ways should cover baseline behavior.

### Neutral

- **No change to existing trigger types.** This ADR adds a new type; it does not modify `UserPromptSubmit`, `PreToolUse`, `Stop`, `SessionStart`, `PostCompact`, or `context-threshold`. Existing ways continue to work exactly as before.
- **Way authoring model is unchanged for non-attend ways.** Only authors who want to write signal handler ways need to learn the new fields. Everyone else continues as before.
- **Scoring infrastructure is unchanged.** The embedding + BM25 + NCD tier (ADR-107, ADR-108) handles `ways show attend/<signal>` invocations exactly as it handles any other way lookup. No new scoring path.

## Alternatives Considered

- **New hook event type (e.g., `AttendEmission`).** Rejected. Duplicates what `Monitor` already provides. `Monitor` is a first-class Claude Code tool that delivers async notifications by design; a new hook event class would be a parallel mechanism doing the same job with less flexibility.
- **Automatic firing of ways from `attend` emissions (the original draft of this ADR).** Rejected after the `Monitor` delivery primitive became available. Automatic firing required `attend` to invoke the ways matcher directly, injecting way bodies into Claude's context without Claude's explicit consent. In the `Monitor`-primary model, this violates the "Claude retains agency" invariant from ADR-113 — the awareness layer should inform, and Claude should decide. Keeping ways on-demand via affordance preserves that invariant cleanly.
- **`Monitor`-only with no ways integration at all.** Considered seriously. The argument is that a well-formed notification can carry enough information that deeper guidance isn't needed — `attend` just writes the full guidance into the notification text. Rejected because: (1) Notification text is one line (plus 200ms batching); long prose guidance doesn't fit the format. (2) The ways system already provides rich, habituation-aware guidance retrieval; duplicating that in notification text would compromise the brevity that makes notifications useful. (3) High-salience guidance benefits from ADR-104's disclosure gate — a `Monitor`-only model can't use it.
- **Direct injection bypassing the disclosure gate.** Rejected. Bypasses ADR-104's habituation rules and would cause repeated signals to dominate Claude's context. The disclosure gate is exactly the right place for these decisions, regardless of whether the invocation came from a hook or from Claude acting on an affordance.
- **Invoke ways from `attend` directly (not via Claude).** Rejected. `attend` invoking ways on Claude's behalf would inject content without Claude's agency, violating the design note's invariants. Claude must be the one to invoke `ways show attend/<signal>` because Claude is the one deciding the affordance is worth engaging with.
- **A single catch-all "external" trigger type instead of `attend`-specific.** Considered and rejected. Tying the trigger type to a specific source documents what's producing the affordance convention and prevents the trigger type from becoming a dumping ground. If another source of proactive signal is ever needed, it gets its own trigger type and its own ADR.
- **`subscriptions` as the field name (earlier draft).** Renamed to `signals` because the new model doesn't involve subscribing to an event stream — the way simply declares which signal names it handles when invoked.

## References

- **Design note:** [Cognitive Loop and the Awareness Layer](../../design-notes/cognitive-loop-and-awareness-layer.md)
- **Related ADRs:**
  - [ADR-104](./ADR-104-token-gated-way-re-disclosure-for-long-context-windows.md) — Disclosure gate that this ADR reuses
  - [ADR-105](./ADR-105-progressive-disclosure-for-way-trees.md) — Progressive disclosure model
  - [ADR-108](./ADR-108-embedding-based-way-matching-with-all-minilm-l6-v2.md) — Matcher that scores attend emission payloads
  - [ADR-112](./ADR-112-session-ledger-and-knowledge-graph-integration.md) — Reflection and ledger ways that will be the first consumers of attend signals
  - [ADR-113](./ADR-113-attend-active-awareness-module.md) — The attend binary whose emissions this ADR routes
