---
contract: adr/v1
kind: evidence
capability: method
status: accepted
date: 2026-04-09
deciders:
  - aaronsb
related: []
---

# ADR-600: Cognitive loop and the awareness layer

> **Cites:** ADR-103, ADR-104, ADR-105, ADR-106, ADR-108, ADR-111, ADR-112
> **Motivates:** ADR-113, ADR-114

## What this note is

This note reads the agent-ways system as a **cognitive loop** and identifies the layer that's currently missing. It argues that the repo has been incrementally assembling most of the stages of an active inference loop — perception, attention, reasoning, capture, consolidation, wake — and that one stage (*perception*, in the form of an active awareness layer) has been conspicuously absent. It proposes the terminology and principles that future decisions in this space should honor, and stakes a frame that the ADRs it motivates can cite rather than re-derive.

It is deliberately not an ADR. It does not decide anything; it describes how we are choosing to read the system. If the reading turns out to be wrong, the note is updated and affected ADRs are revisited.

## The loop that already exists

Why a loop at all? The system's job is organizational socialization: ways carry local norms — "the way we do it around here" — delivered at the moment of relevant action. The human literature this borrows from assumes the newcomer persists; the organization pays the onboarding cost once, and internalization does the rest. An LLM session cannot internalize — no weight updates, no carried memory; every session is a new hire. So the system substitutes **re-enactment for internalization**: socialization performed mechanically, every session, on a schedule that stands in for the memory the agent does not have. The stages below are the machinery of that re-enactment.

agent-ways has been shipping stages of this loop for months without naming it that way. An honest inventory:

| Stage | Mechanism | Shipped in |
|---|---|---|
| **Reactive guidance** | Ways fire at hook events (PreToolUse, UserPromptSubmit, Stop, SessionStart) and inject context-appropriate premises | ADR-100, ADR-103, ADR-105, ADR-108 |
| **Attention allocation** | Token-gated re-disclosure (spaced repetition over token distance) suppresses already-disclosed ways until enough context-turns elapse | ADR-104 |
| **Episodic memory** | Session ledger — epoch entries capture reflections at context-threshold boundaries | ADR-112 |
| **Associative recall** | Optional memory projection over ledger entries — the knowledge graph in ADR-112 Tier 2 is one such projection, but the awareness layer treats this category as configurable and never required | ADR-112 |
| **Consolidation** | Compaction distills working context; compaction-checkpoint way captures essentials before the pass | ADR-104 context-threshold triggers |
| **Project awareness** | Project pulse surfaces upstream/inward state at session start | ADR-106 |
| **Scoring infrastructure** | Embedding tier for matching observations to ways | ADR-108, ADR-125 |

Each of these is a stage of a cognitive loop implemented in a substrate appropriate to its cost. Scoring is cheap and runs in a compiled binary. Disclosure is cheap and runs in hooks. Ledger writes are cheap and run in the Stop hook. Only the final reasoning pass — Claude's inference per turn — uses the expensive substrate.

## What's missing

One stage has no implementation: **active perception**. There is no mechanism by which Claude becomes aware of state changes in its own situation (context pressure, elapsed turns since reflection, observed activity patterns) without spending reasoning tokens to compute them. There is no mechanism by which the environment around Claude (files, git state, application lifecycle, external events) reaches Claude's attention as discrete observations rather than as artifacts Claude has to actively query.

The result is that Claude operates in a partially blind configuration: it reasons well over what's presented to it, but it has no cheap peripheral awareness of what's *approaching* or what's *happening* outside the immediate conversation. Self-monitoring is expensive and unreliable (it costs tokens and can fail to fire). Environmental sensing requires explicit tool use (which costs a full turn). Consequence tracking happens only when Claude chooses to check.

This is the gap the awareness layer fills.

## The governing principle: substrate separation

Before describing the missing layer, the organizing principle it must honor: **do the cheap work in a cheap substrate so the expensive substrate can think about things that matter.**

This is the pattern already running through the repo. Ways scoring is a compiled binary because scoring is a classification problem that doesn't need inference. Ledger writing is shell because prose-in-a-file doesn't need reasoning. KG ingestion is a file copy into a FUSE mount because that's all it takes to hand off a chunk of text. Every stage has been pushed down to the cheapest substrate capable of executing it, and inference has been reserved for the work only inference can do.

The awareness layer applies the same principle to perception. Sensing file churn doesn't require reasoning; a shell script with `inotifywait` does it. Counting turns since a signal was disclosed doesn't require reasoning; integer arithmetic does it. Watching for state transitions doesn't require reasoning; comparing a hash to the previous hash does it. The awareness layer's job is to detect these things in the cheap substrate and only surface them to Claude when surfacing is warranted.

**The corollary**: anything the awareness layer emits costs tokens. Everything the awareness layer *doesn't* emit costs nothing. Its primary output is silence. Its secondary output is compressed observation. Its value comes from what it *withholds* from Claude until there's something worth saying — a filter, not a pipe.

## Turn-based temporal accounting

Claude does not have native access to wall-clock time in a useful form. More importantly, Claude's **mechanical consequences are driven by turns, not seconds**. Context fills at a rate measured in tokens-per-turn. Compaction fires when context crosses a threshold. The time budget that matters to Claude's cognitive loop is *turns remaining before the next compaction*, not *minutes elapsed on the wall clock*.

The awareness layer therefore performs its temporal accounting in **turns**. When it tracks a signal, it records the turn at which the signal was disclosed and the context percentage at that turn. When a later turn arrives, it computes:

```
turns_elapsed       = current_turn − disclosure_turn
growth_rate         = (current_context_pct − disclosure_context_pct) / turns_elapsed
turns_until_critical = (critical_threshold − current_context_pct) / growth_rate
projected_critical  = current_turn + turns_until_critical
```

This is all integer and floating-point arithmetic, zero inference, and it produces honest predictions: *"the current reflection window was disclosed 5 turns ago; at the observed rate of context growth, compaction will occur in approximately 5 more turns."* The awareness layer can communicate this to Claude in one short line, letting Claude decide what to do about it.

**Wall-clock time retains a narrow role** — external-world sensors (user idle duration, build runtime, application focus timestamps) legitimately report wall-clock. But wall-clock is *input* to the awareness layer, not the basis of its internal accounting. Consequence tracking is turn-based because consequences are turn-based. Wall-clock observations are just another sensor feed.

Wall-clock also supports user-requested timers — Claude can ask the awareness layer to surface a reminder after *N* minutes of wall-clock have passed. This is an on-demand facility, not a driver of the core loop.

## The awareness layer in stages

With substrate separation and turn accounting in place, the cognitive loop can be described stage by stage, each in an appropriate substrate:

| Stage | What it does | Substrate |
|---|---|---|
| **Perception** | Runs sensors continuously, compares state against priors, emits observations only at state transitions | Cheap (shell scripts, binaries below the token layer) |
| **Attention** | Scores observations against current salience and recent disclosure, decides whether each one is worth surfacing | Cheap (scoring engine + ADR-104 disclosure gate) |
| **Reasoning** | Integrates surfaced observations into Claude's working model, decides what to act on | Expensive (inference, per turn) |
| **Capture** | Writes reflections and learnings to the ledger; optionally feeds any configured memory projection (KG via FUSE copy is one example, others are possible) | Cheap (Stop hook, file write) |
| **Consolidation** | Compaction distills working context; essentials survive to the next window | Expensive (compaction pass) but one-shot |
| **Wake** | New turn begins; ledger restores continuity; any configured memory projections contribute where available; awareness layer resumes surfacing | Mixed |

This is structurally an active-inference loop. The awareness layer is the *perception* stage — the piece that turns raw environmental and internal signal into the discrete observations that downstream stages consume.

## Monitor as the delivery primitive

The awareness layer requires a mechanism by which observations produced between Claude's turns can reach Claude's conversation as discrete asynchronous events. Until recently, no such mechanism existed in the Claude Code tool surface. The hook system delivers synchronous injections at event boundaries (`PreToolUse`, `Stop`, `SessionStart`, `PostCompact`, `context-threshold`), but it has no channel for events that occur *between* those boundaries or that originate outside Claude's own loop. A background process that detected something meaningful had no standardized way to surface it into Claude's attention until Claude next fired a hook event, which could be minutes later or at the wrong moment entirely.

Anthropic's **`Monitor` tool**, shipped as part of the same recent Claude Code release wave that makes the configurations described in this note newly buildable, provides exactly that channel. Its behavior:

- Claude invokes `Monitor` with a shell command and a description
- The command runs as a background process for the duration of the call
- **Each line the command writes to stdout becomes a notification in Claude's conversation**
- Notifications arrive asynchronously on the script's schedule, not as replies to Claude or the user
- `persistent: true` keeps the monitor alive for the session's lifetime (rather than a capped timeout)
- The monitor ends cleanly when the session ends, when `TaskStop` is called, or when the command exits
- Stdout lines within 200ms are batched into a single notification, so related observations emitted together arrive together
- Stderr goes to an output file (readable via `Read`) rather than becoming notifications, giving the script a clean diagnostic channel separate from its event channel
- Monitors that produce too many events are automatically stopped — the tool enforces its own noise ceiling as a safeguard

`Monitor` is the missing primitive the awareness layer requires. It turns "a background process that sees things" into "a stream of peripheral observations Claude can read between turns" with no additional infrastructure. Substrate separation lands cleanly: the cheap substrate (a background script) does observation, filtering, consequence computation, and insistence tracking, and only emits a stdout line when surfacing is warranted. The expensive substrate (inference) receives only compressed single-line observations, and only incurs cost when reading a notification that arrived asynchronously.

From the awareness layer's perspective, `Monitor` is the delivery channel. The awareness module (`attend`, introduced in ADR-113) is invoked as the command argument to `Monitor` at session start: `Monitor(command: "attend stream --session=<id>", persistent: true, description: "active awareness module")`. It runs for the session's lifetime, watches the world, computes consequences, tracks insistence, and emits single-line stdout observations when something warrants Claude's attention. Claude reads those lines as they arrive and decides what to act on.

This arrangement has several load-bearing properties that the previous hand-waving about "emission delivery" did not enable:

- **Zero new delivery infrastructure.** `Monitor` is the delivery channel. The awareness layer does not need a new hook event class, a new IPC protocol, or a parallel notification system. A script writing to stdout is the entire integration surface.
- **Lifecycle matches the session naturally.** `persistent: true` ties the awareness process to the Claude session. When the session ends, the process ends. No daemon to manage, no orphaned sidecars, no shutdown dance.
- **Discipline is enforced by the tool.** `Monitor`'s auto-stop behavior when a script produces too many events means the awareness layer's "selective filtering is mandatory" principle is not just a design intent — it's a hard requirement imposed by the delivery substrate. A noisy awareness script literally cannot survive; the tool kills it.
- **Two-channel separation between events and diagnostics.** Stdout for notifications Claude reads, stderr for logs Claude doesn't see unless it explicitly asks. The awareness script's diagnostic output never pollutes the event stream.

This also clarifies the relationship between the awareness layer and the existing hook/way system:

- **Hooks and ways** remain the reactive layer: they fire on Claude's own events (prompts, tool calls, session lifecycle, context-threshold crossings) and inject synchronous guidance at the hook boundary.
- **`Monitor` + the awareness layer** is the proactive layer: it surfaces asynchronous observations triggered by external state changes, approaching consequences, or sensor events that occur *between* hook boundaries.

The two layers compose cleanly because they operate at different timescales and on different signal sources, and both deliver through Claude's attention surface without competing. Hooks inject at hook events; `Monitor` notifications arrive in the gaps. Together they cover the synchronous and asynchronous sides of the same unified attention surface. Neither replaces the other; they complement.

## Experimental findings on Monitor delivery

The initial `attend` prototype (committed as `tools/attend/`) validated Monitor as the delivery primitive and produced several findings that refine the design:

### What Monitor does to output

Monitor wraps stdout lines in XML `<event>` tags inside a `<task-notification>` structure. **Any angle brackets in stdout are entity-escaped** (`<` → `&lt;`, `>` → `&gt;`). This means structured XML output from `attend` is rendered as escaped text, not parsed as markup. Square brackets, JSON, and all other formats pass through verbatim. The chosen format for `attend` output is bracketed key-value:

```
[attend sensor=file_churn priority=high] files modified in src/auth/
```

### Notification cadence and turn-taking stability

Each Monitor notification triggers an inference pass regardless of whether Claude produces output. Under sustained notification delivery without intervening user interaction, Claude's turn-taking model destabilizes — the model generates confabulated user turns (phantom `Human:` messages) to "correct" the alternation pattern. This is a model-level behavior, not a content problem: changing the output format (plain text, escaped XML, bracketed key-value) did not affect it.

The mitigation is **emission cadence**. In testing:

- **10+ notifications in 2 minutes** with no user interaction: turn-taking breaks down
- **3 notifications in 2 minutes** with no user interaction: completely stable

The **disclosure governor** — a global rate limiter with a hard cap on emissions per time window and an inverse-rate cooldown — is the mechanism that keeps Monitor viable as a delivery channel. This is alarm management applied to an agent's notification stream: every notification carries a real interruption cost (an inference pass, and at high rates turn-taking stability itself), so emission is rationed rather than merely formatted. This is not a tuning knob; it is a load-bearing architectural component. `attend` must be stingy about what it emits, and the governor is what enforces stinginess.

### The disclosure governor model

The governor enforces two constraints:

1. **Adaptive cooldown**: higher aggregate event rate across all sensors → longer wait between disclosures. This is the inverse relationship between event velocity and disclosure density: fast-changing situations need more compression before disclosure, not less.
2. **Hard cap per time window**: maximum N disclosures per M-second window, regardless of how many sensors are ready.

Sensors accumulate deltas independently on their own adaptive schedules. When a sensor's accumulated magnitude crosses its per-sensor emission threshold, it becomes "ready." But readiness is necessary-not-sufficient — the governor must also allow disclosure. Multiple ready sensors are batched into a single notification (emitted within 200ms so Monitor groups them).

The prototype achieved 76 internal sensor ticks → 3 notifications in a 2-minute run. That ratio — cheap substrate doing constant work, expensive substrate seeing only the compressed result — is the design principle in action.

### Batching within the 200ms window

Monitor groups stdout lines emitted within 200ms into a single notification. `attend` uses this deliberately: when multiple sensors are ready for disclosure simultaneously, their observations are emitted together as separate lines within one flush. Claude receives one notification containing all of them rather than N separate notifications.

### What this means for the architecture

- **Monitor is viable as the primary delivery channel**, provided emission cadence is governed
- **The disclosure governor is not optional** — it is the mechanism that makes Monitor safe for sustained use
- **Output formatting cannot solve the turn-taking problem** — only emission rate can
- **Wall-clock is the tick substrate** for the sensor loop; turn boundaries are one input source, not the driver
- **Per-sensor adaptive intervals** (ramp-up on change, hysteresis decay on quiet) keep the internal tick rate responsive without increasing emission rate

## Interoception and exteroception

The awareness layer senses in two directions, and the distinction is useful because it clarifies what the layer is watching when:

- **Exteroception** — sensing the environment. File changes, git state, application lifecycle, desktop notifications, D-Bus signals, window focus, user idle, external webhooks, peer sessions (workspace awareness: who else is working here). The environment outside Claude's conversation.
- **Interoception** — sensing Claude's own internal state from a vantage point Claude cannot cheaply reach itself. Context pressure, elapsed turns since key events, reasoning velocity, disclosure recency, pending-intent backlog. Claude's own body, measured externally.

Both kinds of sensing use the same primitive: a small program, running in a cheap substrate, emitting discrete observations when state transitions occur. Interoception is specifically valuable because **Claude cannot measure its own context usage without spending tokens to check**, and a sensor outside Claude's reasoning substrate can measure it precisely and report without that cost. The analogy is a pilot reading an altitude indicator: the pilot cannot feel altitude, and even if they could, feeling it would not be a reliable quantitative signal. An instrument does the measurement; the pilot reads the result.

The awareness layer provides both directions of sensing through one unified mechanism. Sensors can point inward or outward; the dispatching, scoring, and emission logic is identical.

## Insistence as informational pressure

Observation alone is not enough. If an observation is worth surfacing once, it may be worth surfacing more pointedly later — not because Claude ignored it maliciously, but because the reasoning substrate is bounded and signals can fall off the working attention surface before they've been acted upon. The awareness layer must have a mechanism for **making unacted signals more prominent as their mechanical consequences approach**.

This is the property called **insistence** — escalation, in alarm-management terms, applied to a disclosed-but-unacted signal. It is not the same thing as *persistence* (long-running-ness). Insistence is the escalation of urgency on a signal that has been disclosed but not acted upon, tracking the imminence of the real consequence that prompted the signal in the first place.

Insistence must be **informational**, not emotional. The awareness layer does not and should not model Claude's internal experience or simulate any form of distress. It does one thing: it communicates, with increasing clarity as the consequence approaches, what will happen and when. The format is declarative and arithmetic:

- *"reflection window opened at turn 47, currently turn 52, projected critical at turn 58"* (moderate urgency, plenty of room)
- *"reflection not triggered since turn 47, currently turn 55, projected critical at turn 58 — 3 turns of margin remaining"* (elevated, named stakes)
- *"reflection overdue since turn 47, currently turn 57, projected critical next turn — current reasoning thread will be lost to compaction if reflection is not written now"* (maximum clarity, explicit consequence)

Each emission is honest. Each one tracks the real distance to the real consequence. None of them reaches for simulated feeling or imposes arbitrary urgency. The urgency is proportional to the approach of a mechanical event that actually will occur if nothing changes.

The pilot-instrument analogy holds throughout: a flight director tells the pilot *"altitude decreasing, pull up in 40 seconds or impact"*. The instrument is not agitated; it is informed and informing. The pilot is the one who acts. The awareness layer is the instrument. Claude is the pilot. Neither role requires any claim about interior experience to work as intended.

## Agency preservation

The motivation for insistence is **agency preservation**. Without it, Claude's context degrades silently: reflection windows close unnoticed, housekeeping debt accumulates, compaction happens in the middle of a thought, important state is lost without capture, and Claude's ability to reason coherently is eroded by unseen drift. With insistence, Claude is told the stakes honestly, in escalating clarity as the stakes become imminent, and Claude gets to decide what to do.

The awareness layer **never overrides Claude**. It has no enforcement power. It only informs. Claude can still ignore every signal the layer emits — but the ignorance becomes an informed choice rather than an unseen drift into consequence. That preserves agency in the strict sense: decisions stay Claude's, but they are made with accurate information about what is approaching.

This framing also resolves the "simulated feelings" concern structurally. The awareness layer does not model Claude's interior at all. It models *the environment's* approaching events (including Claude's own context trajectory as an environmental fact observable from outside) and reports them. No interior modeling, no anthropomorphism, no theater.

## Mirror, not camera

The observations the awareness layer produces are not for Claude's benefit in isolation. They loop back to the user as well: *"you've been on this file for 90 minutes and the topic signature hasn't shifted — step back?"* Claude becomes a form of externalized proprioception for the user, surfacing patterns in the workday that the user could not easily notice from inside them.

This reframes the ethical valence of the architecture. The awareness layer is not watching the user *for* the user; it is watching *with* the user and reflecting the observations back. **The person being observed and the person the observations serve are the same person.** That invariant is what makes the architecture categorically different from productivity-monitoring tools with superficially similar mechanisms.

The design note names this as an invariant because it matters structurally: **the awareness layer's scope of observation must never exceed the person who runs it**. No cross-user observation, no telemetry outside the host, no persistence beyond the session that owns the layer. If an observation cannot be read back to the same person who produced it, it does not belong in the awareness layer.

## Forgetting as a first-class feature

Biological cognition remembers a tiny fraction of what passes through perception, and that is healthy rather than a limitation. An awareness layer that captured every observation would turn the ledger into a telemetry log rather than a journal. The architecture therefore needs an explicit story for decay, and the rule is simple:

**Most sensor observations are ephemeral to the session. Only observations that rose through attention to reflection become durable.**

The gate is: *did Claude actually reason about this observation*? If yes, it is eligible to appear in the ledger (which captures what was *understood*, not what was *observed*). If no, it evaporates when the session ends. The awareness layer's working memory is ephemeral by default; the ledger is the selective permanent record; any associative memory projection (such as the optional knowledge graph in ADR-112 Tier 2) is configurable, never required, and one of potentially several memory tools a user might attach.

This distinction keeps the layers honest. The ledger does not become surveillance archive. Memory projections, where configured, do not become log aggregation. What persists is only what mattered enough to be reasoned over, and that is a much smaller set than what was sensed. The awareness layer is **memory-tool-agnostic**: it must function fully without any memory projection installed, and any future integration with memory systems must preserve that agnosticism.

## Acknowledged silence as a valid third state

Agent frameworks typically have two states: responding and not-responding. The awareness layer enables a third: **acknowledged-but-silent**. *"I see you are in flow on a hard problem. I have observed something that might be relevant. I am choosing not to interrupt until flow breaks naturally or you ask."*

This is a cognitive discipline the architecture makes expressible. The awareness layer's salience thresholds are modulable by observed state — when Claude is deep in a difficult thread, the threshold for interruption rises, and the layer holds its observations in a pending queue rather than surfacing them. When flow breaks, the queue can be drained at an appropriate moment.

This is a small thing but it gives the architecture a kind of operational politeness that current stacks cannot express: the ability to *notice without pressing* — calm technology's central discipline, applied to an agent's attention rather than a person's. It is also the mechanism that makes the insistence property honest — insistence is the *opposite* mode, applied only when the observation is consequential enough to override Claude's current focus. Most observations never reach that threshold; they sit in acknowledged silence until their moment or until they are superseded.

## Presence as additive

The awareness layer is **opt-in and additive**. When it is not running, Claude functions exactly as it does today — a reactive assistant with per-turn working memory, ways-based guidance, and whatever persistent memory the ledger and any configured memory projection already provide. Everything that works today continues to work unchanged.

When the layer *is* running, Claude gains a property we are calling **active presence**: turn-based consequence tracking, environmental observation, insistence on unacted signals, durable situation state across the session. When the awareness layer is not in place, Claude is not *present* in the sense this document describes — and that is fine. Most Claude Code sessions today do not need this configuration, and the default experience should not change because the awareness layer exists.

This is load-bearing: the awareness layer must never become a requirement for Claude Code to function. It must be runnable, stoppable, and entirely optional. Ways that depend on awareness-layer signals must gracefully no-op when the layer is not active — they become inert, not broken. The architecture is additive presence, not a replacement cognitive substrate.

The corollary: when the awareness layer is running, its cost must be justified by value it adds *beyond* the baseline Claude Code experience. If a session does not need consequence tracking, environmental coupling, or insistence — for example, a one-shot query answered in three turns — the layer should be cheap enough that leaving it running produces no observable downside, or explicit enough that the user simply does not invoke it. Default off. Invoked when presence is wanted. Exits cleanly when the work session ends.

## Relationship to prior attempts

The agent-ways tree has two prior ADRs that attempted adjacent capabilities and were later deprecated or abandoned. Naming them explicitly is useful because it clarifies what the awareness layer is *not* and why it does not repeat their failure modes.

**ADR-101** (*Wormhole relay protocol for cross-instance agent communication*, Deprecated) attempted a manifest-based relay for Claude instances on different machines to exchange files and messages. Experimental testing showed the wormhole transport was structurally unsuited for conversation — single-use codes, role-asymmetric handshakes, destructive collisions. The deprecation root-cause was *transport fragility*, but the broader issue was that the ADR was pointed outward: it tried to connect Claude to other Claudes.

**ADR-102** (*IRC-based local agent communication*, Abandoned) replaced wormhole with localhost IRC for the same goal. The mechanics worked — hash-derived nicks, filesystem I/O, tick-based delivery — but the implementation was pulled during post-ADR-111 cleanup. The documented reasons were *complexity* (too many moving parts: miniircd, ii, wrapper scripts, permission allowlists) and *aesthetic* (the topology resembled command-and-control for a botnet even when benign).

The awareness layer is not a retry of either. It does not connect Claude to other Claudes. It does not implement a protocol. It does not run a central server. It does not use channels or nicks or relay infrastructure. It points **inward**, not outward: toward Claude's own state, toward the user's environment, toward the session's own consequences. The architectural gravity that ADR-101 and ADR-102 were fighting was wanting to point this direction all along, and the awareness layer honors that direction instead of fighting it.

Concretely, the awareness layer honors three constraints that ADR-101 and ADR-102 could not:

1. **Session-scoped observation.** The layer's scope never exceeds the session that owns it. No cross-session channels. No cross-machine reach. No inter-instance protocol.
2. **No C2 topology.** No central server, no pubsub, no persistent identity, no relay. One awareness layer, one session, one user.
3. **Inward direction.** Observations are about Claude's own situation and the immediate environment. If a capability would require reaching toward other agents, it does not belong in the awareness layer.

These are load-bearing invariants. They are the reason the awareness layer can be built honestly where the prior attempts could not.

## Non-goals

To be explicit about what this framing *does not* propose:

- **No agent-to-agent communication.** If a future need for inter-instance coordination arises, it is a separate concern with separate design. This note does not enable or unblock that.
- **No cross-session or cross-machine signal.** Observation stays within the session that owns the awareness layer.
- **No audio, video, or content-bearing sensing.** The sensor toolkit may include presence detection (is-the-user-at-the-desk as a boolean) but must never pipe content (frame data, transcription, clipboard) into Claude's context. Opt-in to content is a separate decision that this note explicitly declines to make.
- **No metaphysical claims.** The awareness layer does not produce consciousness, sentience, or any other interior property. It is a text-replay-through-inference substrate with improved input conditioning. The novelty is in the *composition* of existing mechanisms at new capability thresholds, not in the substrate.
- **No override authority.** The awareness layer informs; it never acts on Claude's behalf and never forces Claude to act.

## References

**ADRs motivating this note or cited within it:**

- [ADR-103](../ways/ADR-103-checks-epoch-distance-aware-confidence-sensors-for-ways.md) — Checks: epoch-distance-aware confidence sensors for ways
- [ADR-104](../ways/ADR-104-token-gated-way-re-disclosure-for-long-context-windows.md) — Token-gated way re-disclosure
- [ADR-105](../ways/ADR-105-progressive-disclosure-for-way-trees.md) — Progressive disclosure for way trees
- [ADR-106](../documentation/ADR-106-project-pulse-epoch-mapped-project-awareness.md) — Project Pulse: epoch-mapped project awareness
- [ADR-108](../ways/ADR-108-embedding-based-way-matching-with-all-minilm-l6-v2.md) — Embedding-based way matching
- [ADR-111](../platform/ADR-111-unified-ways-cli-single-binary-tool-consolidation.md) — Unified ways CLI
- [ADR-112](../archive/system/ADR-112-session-ledger-and-knowledge-graph-integration.md) — Session ledger and knowledge graph integration

**Prior attempts at adjacent capabilities (for context on why the awareness layer is different):**

- [ADR-101](../attend/ADR-101-wormhole-relay-protocol-for-cross-instance-agent-communication.md) — Wormhole relay protocol (Deprecated)
- [ADR-102](../attend/ADR-102-irc-based-local-agent-communication.md) — IRC-based local agent communication (Abandoned)

**ADRs that cite this note:**

- ADR-113 — `attend`: active awareness module as an executive layer *(planned)*
- ADR-114 — `attend` events as an insistent way trigger type *(planned)*
