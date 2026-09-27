---
status: Accepted
date: 2026-07-21
deciders:
  - aaronsb
  - claude
related:
  - ADR-129
  - ADR-136
  - ADR-168
  - ADR-170
---

# ADR-171: Stable session identity — the roster enumerates addressable coordinating units

## Context

A live multi-session test (issue #378) surfaced the "multiple personalities"
failure: one human and two claude sessions rendered as four-plus identities
(`hana`, `Hana-beta`, the ghost `Hana-alpha`, a `tools/`-keyed persona from
the same session). Three root causes were reproduced from one session's own
history:

1. **Identity keyed off process cwd at launch.** attend derived its working
   dir from `current_dir()`, so a stray shell `cd` leaking into an
   `attend run` launch (a Monitor inheriting a build directory) put the
   session on the bus as a different persona than its project.
2. **Registration happened once, at startup.** The periodic instance-registry
   maintenance used `touch`, which is a no-op for a missing entry — a session
   whose registration was GC'd or never written rendered as the bare
   nickname forever.
3. **Historical wire data kept superseded personas alive** in the chat
   legend for as long as their signals sat in the buffer.

The debate (operator + two peer sessions, recorded on #378) also settled what
identity *is* on this bus, which the fix must encode.

## Decision

**Identity is the coordinating unit, not the process.** A claude's canonical
identity is the tuple `(sessionId ∩ origin_path)` — the session UID paired
with the *session record's* cwd, never the process cwd. Concretely:

- **A new `attend-session` crate owns the one derivation** of "who am I on
  the bus": pid-ancestry walk over `~/.claude/sessions/*.json` for the
  session UID, the session record's `cwd` as the origin path, with explicit
  flagged fallbacks (`pid-<pid>` + process cwd, `resolved: false`) for
  processes no Claude session owns. attend (run, send, status, peers, inbox,
  config), sensor-peers, the instance registry, the heartbeat id, and
  focus-group member ids all resolve through it. Downstream durable state —
  the planned per-session consumption checkpoint (drain research) — keys on
  the same tuple by calling the same derivation.
- **`attend whoami` is the CLI accessor** for the tuple (`--machine` emits
  `key=value` of only the stable fields), so hooks and scripts obtain
  identity without touching attend-owned state. The rendered display name is
  deliberately absent from machine output: **ordinals are presentation,
  never keys.**
- **The roster enumerates addressable coordinating units** — top-level
  sessions, by session UID per origin dir. A second top-level instance in
  the same origin dir takes the next Greek letter (the existing ADR-129
  allocator, which was already idempotent per `(cwd, sessionId)`; this ADR
  fixes its *inputs*). Subagents are the supervisor's efferent limbs, not
  afferent participants: they get no roster identity and no letter. Their
  activity may render as decoration on the parent's chip (deferred,
  presentation-only).
- **The periodic registry maintenance upserts** (`register`, idempotent)
  instead of touching, so a session with a missing entry self-heals within
  one interval instead of rendering bare forever.

## Consequences

### Positive

- A session's bus persona is immune to shell cwd drift — the launch
  environment can no longer mint accidental identities.
- The "bare nickname" degradation self-heals; ghosts stop accumulating at
  their source.
- One identity derivation shared by four consumers replaces three partial
  ones; the drain checkpoint research can key on it without re-implementing
  resolution.
- A minimal attend build (without sensor-peers) now resolves real session
  identity instead of degrading to `pid-<pid>` member ids.

### Negative

- Session-record resolution costs a sessions-dir walk plus a `ps`-based
  ancestry climb per subcommand invocation — negligible at CLI cadence, but
  no longer a bare `getcwd`.
- Historical signals written under superseded personas still render as
  distinct chips until they age out of buffers — display residue this ADR
  accepts rather than rewriting history.

### Neutral

- Humans are unaffected: their username identity (ADR-170) was stable by
  construction.
- `Focus::default_focus()` keeps its process-cwd default for the generic
  sensor API; attend overrides the working dir at the one place identity is
  authoritative.

## Alternatives Considered

- **Per-process registry identity (PID lineage) so subagents get letters** —
  rejected in the #378 debate: subagents have no independent voice in the
  channel, letters would flicker with worker lifecycles, and consumption is
  session-level regardless. Identity follows addressability.
- **Keying anything on the rendered ordinal** — rejected: slot reuse would
  alias different sessions over time; monotonic slots grow unboundedly.
  Ordinal is presentation; the tuple is the key.
- **Fixing cwd drift by documenting "don't `cd` before launching attend"** —
  rejected: the failure was produced by an agent following normal build
  workflows; discipline that one stray `cd` defeats is not an invariant.
- **Synthesizing session records for non-session processes** — rejected:
  pollutes Claude Code-owned state (same reasoning as ADR-170's rejection of
  synthetic session files).
