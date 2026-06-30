---
status: Accepted
date: 2026-06-29
deciders:
  - aaronsb
  - claude
related:
  - ADR-112
  - ADR-128
---

# ADR-141: Knowledge Graph as Evidential Memory Backend

## Context

Cross-session knowledge has two homes in this framework, and both are *editorial* — they keep a single current view and discard what they supersede:

- **Native auto-memory** (ADR-128): `MEMORY.md` + topic files, loaded at session start. Its compaction cycle ("Auto-Dream") performs minimal consistent revision — when session 12 contradicts session 3, it picks one and rewrites. The *fact that understanding shifted* is destroyed. It also exposes **no programmatic integration hooks** — no `memoryWrite` intercept, no API, only the `/memory` toggle.
- **The session ledger** (ADR-112) preserves raw prose, but is a flat append-only stream. It records *what was said*, not the relationships between assertions across sessions.

ADR-112 already anticipated a third tier: ingesting ledger entries into an external **evidential** knowledge graph (KG) — one that, per Dempster–Shafer rather than AGM, keeps contradictory assertions and *scores the balance of evidence* (AFFIRMATIVE / CONTESTED / CONTRADICTORY / INSUFFICIENT_DATA). That tier was sketched as a one-line file copy guarded by `if -d`, but left two things unspecified:

1. **Activation and detection** — how a session decides, safely and without configuration ceremony, that a valid KG is present *and* that this project has opted in.
2. **The return path** — ADR-112 only described writing *into* the KG. It did not address consuming the KG's evidential view *back* into the session, where the contradiction-preserving signal would actually change reasoning.

The motivating force is general: an agent operating under a shrinking context window needs a durable, *non-editorial* memory whose value compounds across sessions, and it must integrate without fighting a host memory system that offers no hooks. This ADR records how the framework attaches to such a backend.

## Decision

Integrate an external evidential knowledge graph as an **optional, presence-gated** memory backend, in two independently activatable directions. The framework owns *detection and activation*; the KG owns *curation*. The reference backend is a `kg`-compatible system exposing per-ontology ingestion (e.g. a FUSE mount at `${KG_KNOWLEDGE_ROOT:-$HOME/Knowledge}/ontology/<ontology>/ingest`, or an equivalent `POST /ingest` API).

### 1. The activation contract: presence *is* opt-in

A project's KG ontology is named after its slug. The **existence of that ontology** answers two questions at once:

| Question | Established by |
|---|---|
| Is a valid KG reachable? | The ontology's ingest endpoint resolves — the backend had to answer for the ontology to exist |
| Has *this project* opted in? | The resolved ontology is named for *this* project |

Opt-in therefore requires no separate config in the happy path: if the ontology is absent, the integration is inert at zero cost. A single `ways.json` flag exists only as an explicit veto.

**Three states**, evaluated as `not-vetoed ∧ ontology-present`:

- `inactive` — ontology absent → integration silently does nothing (default).
- `active` — ontology present → session knowledge flows.
- `hard-off` — `reflection.kg = false` → never flows, even if the ontology exists (per-machine / per-session veto).

**Activation is the act of creating the ontology.** A helper exposes `activate` (create the ontology), `status` (report the state for this project), and `deactivate` (set the veto flag; never deletes data).

### 2. Detection is fail-safe and never blocks the session

Detection resolves the project ingest target or reports absence. It must:

- treat an unreachable or orphaned backend as *absent*, not as an error (a dead mount stats false → `inactive`);
- never block — any copy to the backend is backgrounded and fire-and-forget, consistent with the Stop hook running `async` (ADR-112). A wedged backend must not stall a turn.

The same contract has two transports: a live ingest directory (file copy) where a mount is present, or `GET /health` + ontology-exists + `POST /ingest` for headless/CI hosts with no mount.

### 3. Outbound — session knowledge to the KG

When `active`, the framework copies its existing durable artifacts to the ontology's ingest target: ledger entries (ADR-112) and auto-memory writes (ADR-128). These are already curated prose with frontmatter; no new artifact is produced. The KG deduplicates across channels. This is fire-and-forget: the source artifact exists regardless of whether ingestion succeeds.

### 4. Inbound — the KG as a read-only memory projection (staged)

Because native memory exposes no hooks, the only integration surface for the *return path* is the memory files themselves (ADR-128's "observe and rewrite" constraint). The framework therefore consumes the KG by **projecting it into the native memory surface as read-only files**: a generated `MEMORY.md` index of the highest-value concepts (ranked by grounding × connectivity, scoped to the project ontology, within the 25 KB session-start window) plus concept topic files served on demand. Claude reads KG concepts through the same slot it already reads memory — including `CONTESTED`/`CONTRADICTORY` entries that editorial curation would have erased.

This direction is **staged behind the outbound bridge** because it carries three constraints that must be resolved in the implementation ADRs/PRs, not assumed away:

- **The projection is read-only, and the editorial layer must be disabled.** Auto-Dream rewrites and prunes memory files on its own schedule and cannot be hooked; a projected surface must present as read-only and run with Auto-Dream toggled off, otherwise the curation this integration exists to escape runs against the projection.
- **Writes redirect to ingestion, not to the memory path.** With the read surface read-only, the framework's "save a memory" path must target the KG ingest channel (direction 3), closing the loop: reads come from the projection, writes go to ingestion.
- **Mounting over a populated, host-owned directory is unsolved by a pure API-backed filesystem.** Presenting the projection at the memory path requires either relocating genuine user-level memory into the KG (takeover) or union/overlay support in the backend filesystem. This is an implementation decision deferred to the inbound PR.

### 5. The generalized principle

Cross-session memory should be **artifact-presence-gated and evidentially curated**: the framework detects and gates an external backend by the presence of the project's own artifact (not by configuration), pushes its existing durable artifacts to it fire-and-forget, and — where the host permits — consumes the backend's evidential view back through whatever surface the host already reads. The framework never becomes the curator; it routes. Any backend satisfying the ingest/projection contract is substitutable.

## Consequences

### Positive

- Sessions gain a memory whose value compounds — contradictions are preserved and scored rather than overwritten, and minor signals survive as connected low-weight nodes instead of being pruned.
- Zero-ceremony opt-in: creating the ontology activates the project; its absence is a silent, costless no-op.
- No coupling to a specific backend — the ingest/projection contract is the interface; the reference KG is replaceable.
- Outbound ships independently of inbound; each direction is separately activatable, consistent with ADR-112's tiered model.
- Works without fighting the host: outbound copies existing artifacts; inbound uses the only surface native memory exposes.

### Negative

- Inbound depends on disabling Auto-Dream and on resolving the populated-mountpoint problem — non-trivial, hence staged.
- Detection correctness hinges on fail-safe behavior against orphaned/wedged backends; a naive `-d` check without liveness/timeout discipline could stall a turn (a portable timeout is itself a follow-up, as stock macOS lacks `timeout`).
- A ranked projection within the 25 KB window means the session sees a *curated slice* of the graph, not all of it; ranking quality becomes load-bearing.

### Neutral

- This refines ADR-112's Tier 2 (gives it an activation contract and a return path) and consumes ADR-128's "redirect, don't suppress" model (the projection *is* the redirect, taken to its conclusion).
- The ledger remains the source of truth and the replay seed; the KG is a derived, regenerable view.
- Implementation spans two repos (the framework: detection, activation, hooks, write-redirect; the backend: memory-view formatter, ranked projection query, read-only/overlay mount mode) and will land as separate referencing PRs.

## Alternatives Considered

- **Query the KG via MCP at state transitions instead of projecting into memory.** Rejected as the *primary* return path: it requires the session to issue queries and the host to surface results, whereas the memory slot is read unconditionally at session start. MCP query remains a viable complementary retrieval mode and is not precluded.
- **Disable native memory entirely.** Rejected for the same reasons as ADR-128 — it fights the harness, breaks on updates, and removes legitimate cross-project memory. The projection coexists with the feature instead of replacing it.
- **Configuration-flag opt-in (enable per project in `ways.json`).** Rejected as the primary mechanism: it duplicates state that the ontology's existence already encodes and invites drift (flag on, ontology absent). The flag is retained only as a veto.
- **Push via a CLI/MCP call rather than a file copy.** Rejected for outbound: a guarded file copy is fire-and-forget, has no failure surface that can block a turn, and needs no tool call. The API transport is the documented fallback for hosts without a mount.
- **Full takeover vs. overlay for the inbound mount.** Deferred, not decided here — both satisfy the contract; the choice belongs to the inbound implementation ADR.
