---
status: Proposed
date: 2026-04-28
deciders:
  - aaronsb
  - claude
related:
  - ADR-113
  - ADR-118
  - ADR-120
  - ADR-124
---

# ADR-129: Instance suffix and heartbeat liveness for attend identity

## Context

attend's identity and liveness systems each have a corner case that has surfaced in real use. The two cases are independent in cause but adjacent in effect — both produce a wrong picture of "who is here" — and they should be addressed in one decision so the design boundary between them is explicit.

**1. Same-cwd identity collision.** `agent_identity::Identity::for_cwd(cwd)` (`tools/agent-identity/src/identity.rs`) is a pure function of the cwd path: an FNV-1a hash of the canonical path indexes into the nickname pool. The original design comment justifies this — "a claude restarting in the same cwd keeps the same name" — and it does that job correctly when there is at most one session per cwd. It does not cover concurrent invocations: two `claude` processes started in the same directory hash to the same index and present as the same `Jovan (myproj)`, with the same color and style. The user has reproduced this by launching claude twice in the same directory; both render identically in `attend peers`, in chat, and in any signal authored by either side.

**2. Stale registration and ghost agents.** `tools/attend/src/groups.rs:347` defines `session_alive(sid)` as a stub that returns `true` unconditionally with a TODO comment. As a consequence, `_groups.yaml` accumulates dead members across sessions and is never cleaned by the liveness check that the rest of the codebase assumes is doing the job. The visible failure surfaces in attend-chat: when the chat TUI reloads, the chip legend (`tools/attend-chat/src/chip.rs:142`, `known_identities`) builds entries from buffered signals and does not filter by liveness — every agent that has ever sent a message in this cwd appears in the legend, including agents whose claude session has long exited. The existing liveness path through `~/.claude/sessions/*.json` plus `pid_is_claude` catches "claude exited" but not "claude alive, attend not running" — the agent is unreachable, but the PID check passes because the claude process itself still exists.

The two problems are not the same problem. Collision is about *naming*: two real, alive sessions need distinguishable identities. Liveness is about *display*: the registry of who-has-spoken needs to be filtered against who-is-still-here. Conflating them — for example, by freeing a name when its session goes stale — would make names re-bind unpredictably and break peer references like `@Jovan-alpha` after a transient quiet period. Keeping them separated is load-bearing.

Both surfaces matter today because attend, chat, and the focus-groups model (ADR-118) increasingly assume legible peer identity: human steering decisions, peer-to-peer addressing, and the attend-chat legend (ADR-124) all rely on the displayed name being unambiguous and the displayed roster being live.

## Decision

Adopt two independent mechanisms, one per concern.

**Independence invariant:** liveness display never touches name allocation. A heartbeat going stale does not free a registry slot. A registry slot expiring (under age-based GC) does not signal liveness. This separation is what keeps names stable across transient quiet periods — peer references like `@Jovan-alpha` survive a 30-second pause in the heartbeat without re-binding to someone else.

### Instance registry — naming

A new persistent file per cwd records which session holds which instance discriminator.

- **Location:** `~/.cache/attend/instances/<encoded-cwd>.yaml`, parallel to the existing `~/.cache/attend/signals/<encoded-cwd>/` layout.
- **Schema:** `session_id → { instance: <string>, registered_at: <iso8601>, last_seen: <iso8601> }`. The field is named `instance`, not `letter`. The file holds *instance assignments*; Greek letters are the current discriminator vocabulary, but the storage shape does not lock that in. Future allocators can swap their vocabulary without changing the on-disk format.
- **Allocator (current):** Greek letters in ASCII spelling — `alpha, beta, gamma, ..., omega` (24 slots). ASCII spelling, not glyphs, matches the existing constraint at `tools/agent-identity/src/names.rs:5` (ASCII only, no diacritics) so that `@`-completion remains keyboard-portable. Numeric fallback past 24: `Jovan-25`, `Jovan-26`, and so on.
- **Slot semantics:** slots are session-bound. Once a `session_id` holds `alpha`, it holds `alpha` for the life of that session — no reclamation while the session is live. New sessions skip taken slots and take the next-free letter. Resume always reclaims the original assignment. This is the property that makes peer references stable.
- **Concurrency:** read-modify-write under `flock(LOCK_EX)` on a sentinel `<encoded-cwd>.yaml.lock` file, *not* on the data file. Locking the data file does not serialize concurrent registers because flock state is keyed on the open-file-description — the kernel inode — not the path. The data file is renamed atomically (`.tmp` → `.yaml`) on every commit, so a fresh opener of the path after the rename gets a different inode and a different lock; the previous holder's lock no longer contends. The sentinel never moves, so its inode is the stable serializer. Two simultaneous fresh sessions racing for `alpha`: the lock orders them; the second writer reads after the first commits and takes `beta`.
- **Age-based GC:** entries with `last_seen` older than 7 days are reclaimable. This handles unbounded registry growth on long-lived projects. Trade-off: a resume after more than 7 days of inactivity may receive a different letter, because the slot may already have been reclaimed by another session. This is acceptable — a week-stale resume is far from the "I just stepped away and came back" case where rename surprise actually matters.
- **Render:** always emit the suffix, even when the session is solo. `Jovan-alpha`, never the conditional `Jovan` (solo) / `Jovan-alpha` (collision). Always-on is chosen over conditional for predictable pattern matching: every render site, every grep, every agent-self-reference produces a name with the same shape.
- **Render sites that must consume the registry:**
  - `tools/attend/src/identity_view.rs::render_sender_label`
  - `tools/attend/src/cmd/peers.rs` (agent column)
  - `tools/attend-chat/src/chip.rs` (chip rendering, `known_identities`, and `resolve_nickname`)

### Heartbeat sidecar — liveness display

A separate per-session file records that the session's attend is currently running.

- **Location:** `~/.cache/attend/heartbeat/<session-id>`, one file per session.
- **Encoding:** mtime *is* `last_seen`. There is no body to parse. Each tick is a `touch`.
- **Touched** per attend tick, in the existing tick loop in `tools/attend/src/cmd/run.rs`.
- **Liveness predicate:** `now - mtime < grace`, with `grace = 90s` — three times the base sensor interval of 30s. Long enough to ride out a paused tick or a slow filesystem; short enough to drop a dead attend within a couple of minutes.
- **Consumers:**
  - `tools/attend/src/groups.rs:347 session_alive(sid)`: replace the `return true` stub with `pid_is_claude(sid_owner) AND heartbeat_fresh(sid)`. This is the predicate the rest of the codebase already calls; fixing it propagates correctness through `_groups.yaml` cleanup and elsewhere. It also catches "claude alive, attend not running," which the PID-only check misses.
  - `tools/attend-chat/src/chip.rs::known_identities`: filter signal-derived identities by liveness so dead agents disappear from the chip legend. Historical messages stay in the buffer (the conversation record is not destructive), but their senders are rendered dimmed or muted when the sender's heartbeat is stale, so the buffer reads correctly without implying the speaker is still here.
- **No write contention:** each session writes only its own file.

### Operational

A new `make purge-attend-state` target wipes `~/.cache/attend/` for clean-base recovery. It is never invoked automatically. After the existing `make attend`, `make attend-rebuild`, `make attend-chat`, and `make attend-chat-rebuild` targets, an advisory hint is printed pointing at the purge target, so users with an inconsistent cache from an older binary know how to reset cleanly.

## Consequences

### Positive

- **Names disambiguate same-cwd peers.** Two simultaneous claude sessions in `~/Projects/myproj` render as `Jovan-alpha` and `Jovan-beta`, with stable colors and styles inherited from the base nickname.
- **Peer references survive quiet periods.** Because liveness staleness never frees a registry slot, `@Jovan-alpha` continues to mean the same session even if its attend tick paused briefly. Agent self-model is preserved across resumes.
- **Ghost agents disappear from the chat legend.** With the heartbeat predicate replacing the `return true` stub, `known_identities` no longer accumulates every agent that ever spoke in the cwd. The chip legend reflects who is currently reachable.
- **`_groups.yaml` self-cleans.** The same predicate, fixed in one place, flows through to focus-group cleanup. Stale members drop on the next peer poll, restoring the cleanup behavior ADR-118 already assumed was working.
- **Catches "claude alive, attend not running."** PID-only liveness misses the case where the claude process exists but its attend has exited or hung; the heartbeat catches it because the heartbeat file stops being touched regardless of process state.
- **Filesystem-as-state continuity.** Both mechanisms fit attend's existing storage model — flat files under `~/.cache/attend/` — so they are debuggable with `ls` and `cat`, and require no new dependency.
- **Discriminator vocabulary is swappable.** Storing `instance: alpha` (rather than `letter: alpha`) lets a future allocator change vocabulary without a file-format migration.

### Negative

- **Identity contract change.** Every existing display surface for nicknames now carries an instance suffix. `Jovan` becomes `Jovan-alpha` even for a solo session. Render sites listed under Decision (`identity_view.rs`, `cmd/peers.rs`, `chip.rs`) must be updated together — partial rollout will produce inconsistent legends. Agents will also see their own displayed name change, which they encounter in self-reference (`I am Jovan-alpha`) and in addressed peer references.
- **Two new state shapes.** `~/.cache/attend/instances/` and `~/.cache/attend/heartbeat/` are added to attend's filesystem footprint. Recovery from corrupt state is the new `make purge-attend-state` target; absent that, debugging "why is my name wrong" requires inspecting two locations.
- **Resume-after-7-days renames.** Age-based GC means a resume after a week of inactivity may receive a different instance letter than it had previously. Mitigation: the rename is far enough from active use that the surprise is small, and 7 days is a tunable upper bound on registry growth. Users with very long lived projects who still want stable names across very long pauses can lengthen the GC threshold.
- **Heartbeat I/O cost.** Each attend tick writes one file. The cost is negligible per session, but on a multi-session host it adds up to N files touched per tick interval. Mitigated by the 30s base sensor cadence and the per-session file scoping.
- **Greek-letter pool exhaustion.** 24 concurrent sessions in one cwd is unreachable in practice today, but the numeric fallback (`Jovan-25`) is intentionally ugly so that hitting it surfaces as a signal that the cwd is overloaded.

### Neutral

- **Heartbeat staleness does not free a name.** This is the explicit design boundary, not an emergent property. A session that is registered but unreachable continues to hold its instance letter until its registry entry GCs by age.
- **Heartbeat is mtime-only.** No file body, no parser. Adding fields to the heartbeat in the future would require a real format; today there is intentionally none.
- **Always-on suffix.** Solo sessions display `Jovan-alpha` rather than `Jovan`. The user-visible cost is a slightly longer name; the gain is that every render site can build the label from a single rule.

## Alternatives Considered

- **Sequential numbering — `Jovan2`, `Jovan3`.** Rejected. Order-dependent on registration race; observers would not agree on numbering without a registry; and it breaks "same name across restart" the moment a peer joins, because the restarting session would re-number.
- **Hex-suffix from session_id — `Jovan-7c4a`.** Stable per session, zero coordination needed, no registry. Rejected because the suffix is unmemorable for both humans and agents in conversation. "Tell `-7c4a` to do X" is a worse UX than "tell `beta` to do X," and the value of the suffix is precisely that it surfaces in human and agent speech.
- **Resumer yields on conflict.** Initial proposal: when a session resumes and finds its old letter held by a live session, it takes the next-free letter. Rejected. Name changes during resume break agent self-model and break references made by humans and peers (`@Jovan-alpha please...`). The current decision — slots are session-bound for the life of the session, no reclamation while live — is the property that makes references durable.
- **Stateless render-time computation.** Sort same-cwd live sessions by mtime and assign letters in order at render time. Simpler, no registry, no GC. Rejected because letters drift when peers exit: the surviving sessions get re-lettered in place, which has the same self-model and reference-breaking problem as resumer-yields.
- **Conditional suffix — only on collision.** Cleaner UX for solo sessions (plain `Jovan`) but every render site must consult the live-set count to know whether to append. Rejected in favor of always-on for predictable pattern matching: one rule, one shape, every site.
- **SQLite for state.** Considered seriously. Three new state shapes, CAS semantics, and a growing YAML parser are real costs. Rejected because: the data is genuinely tiny (handfuls of rows per cwd); attend's whole architecture is filesystem-as-state already (signals, session.json, `_groups.yaml`, sensor checkpoints — all flat files); adding SQLite for *part* of state creates two consistency models inside attend; mtime-as-heartbeat is a perfect Unix-y fit that dies inside a database; and debuggability via `cat` and `ls` is load-bearing for users diagnosing identity problems. SQLite is reconsidered later if attend grows a real query surface — inbox search, threading analytics, cross-session aggregation — that flat files no longer serve.
- **Field on `_groups.yaml` for instance assignments.** Rejected. `_groups.yaml` is scoped to *named* focus groups (ADR-118). Mixing implicit per-cwd identity assignments into it muddies the contract: that file is about named-group membership, not about identity. Keeping instance assignments in their own file preserves the separation.
