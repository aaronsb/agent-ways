---
status: Superseded
superseded_by:
  - "ADR-123"
date: 2026-04-05
deciders:
  - aaronsb
  - claude
related:
  - ADR-103
  - ADR-104
  - ADR-105
---

# ADR-112: Session Ledger with Optional Knowledge Graph Enhancement

> **ARCHIVED — 2026-08-13.** No longer part of the active architecture set. Kept for history
> and so existing references still resolve.
>
> **Why:** Tiers 0 and 1 shipped under a different design: events.jsonl in state_root survives compaction and 'ways rethink' / 'ways introspect' replay it, delivered by ADR-123 and ADR-153/154. Tier 2 (knowledge graph ingestion) was never built and is dropped.
> **Superseded by:** ADR-123
>
> Nothing below this line has been edited.

## Context

Sessions are ephemeral. When context compacts or a session ends, everything Claude learned — decisions made, assumptions revised, patterns discovered, dead ends encountered — evaporates. The auto-memory system (MEMORY.md) captures *surprises*, but most session knowledge isn't surprising enough to memorize yet too valuable to lose. It's the mundane connective tissue: "we tried X, it didn't work because Y, so we pivoted to Z."

Two gaps in the current system motivate this design:

### 1. No durable session record

Ways fire, checks verify, tools execute — but the narrative arc of a session is never captured. The transcript exists but is a raw log, not a knowledge artifact. There's no structured account of what happened, what was learned, and what changed over the course of work. Prior sessions are invisible to current sessions except through the narrow lens of auto-memory.

### 2. Context compaction destroys working knowledge

When compaction fires, Claude loses the detailed understanding built over the session. The compaction-checkpoint way (ADR-104 context-threshold trigger) mitigates this by prompting a summary, but the summary is Claude's attempt to compress knowledge under time pressure. There's no progressive externalization that captures knowledge *as it forms*, while context is still rich.

### The underlying principle

An agent operating under a shrinking resource (context window) must externalize knowledge at a rate that tracks resource consumption, not wall-clock time. This is the same principle as write-ahead logging in databases (log before the transaction commits), incremental checkpointing in HPC (save every N iterations, not N minutes), and medical shift handoffs (outgoing nurse writes a narrative per patient: what happened, what's pending, what to watch for).

### Knowledge tier geometry

Each knowledge tier offers a fixed "surface area" (depth × breadth) that reshapes along an attention dimension — echoing the capacity-precision tradeoff described in Oberauer's concentric model of working memory (2002) and formalized by rate-distortion theory (Shannon/Berger), where a fixed information budget is allocated between fidelity and coverage:

| Tier | Shape | Persistence | Bounded By |
|---|---|---|---|
| **Live context** | Tall, narrow (deep on current task, blind to rest) | Session (collapses at compaction) | Context window |
| **Transcripts** | Shorter, wider (raw history, low signal-to-noise) | Uncertain (possible TTL) | Disk, retention policy |
| **Ledger** | Flatter, wider (curated summaries, high signal) | Permanent (user-controlled) | Disk |
| **Knowledge graph** | Flattest, widest (concepts + edges across all time) | Regenerable from ledger | KG infrastructure |

Each project has its own stack of these tiers with different surface areas based on maturity. The **overlap projection** across all tiers and all projects is the total knowledge coverage. Gaps in the projection are things unknown at any tier. Overlap between tiers is redundancy — and how the tiers validate each other. This projection model has precedent in Formal Concept Analysis (Wille, 1982), where coverage gaps are computed from overlapping concept extents.

Retrieval triggers (domain entry, post-compaction, etc.) are **attention-routing operations** — they select the operating point on the rate-distortion curve, reshaping which slice of the lower tiers gets projected into the live-context tier. Without them, the lower tiers exist but never influence the first-class attention space where work actually happens.

### The economic bet

The reflection entries cost tokens — Claude spends part of its response on summation rather than forward progress. The KG ingestion costs tokens against a separate inference framework (the KG's LLM extraction pipeline). The bet is that this combined cost returns a **greater than 1:1 ratio of value** through connected concept reasoning.

The value comes not from storing what Claude said, but from what the KG *does* with it: decomposing prose into concepts, deduplicating against prior sessions, building epistemic confidence, and discovering structural connections that no single session could see. A 150-token reflection entry might produce 3-5 concepts with edges to dozens of prior concepts. When those connections surface at a future state transition, they provide reasoning context that would otherwise require Claude to re-derive from scratch — or never discover at all.

The cost is bounded (3-4 reflections per session, ~500 tokens total). The value compounds (each session enriches the graph, making future retrievals denser). The ratio improves over time as the graph grows. Early sessions pay more than they get back; mature projects get back far more than they pay.

## Decision

Introduce a three-tier progressive system where each tier is independently activatable:

```
Tier 0 (current):  Ways fire → guidance injected → forgotten at compaction
Tier 1 (ledger):   Epoch entries written to durable ledger → survive compaction → replayable
Tier 2 (KG):       Ledger entries ingested into knowledge graph → cross-session connections emerge
```

Each tier builds on the previous. Tier 1 works without Tier 2. Tier 0 works without either. The tiers are activated via `~/.claude/ways.json` configuration:

```json
{
  "reflection": {
    "ledger": true,
    "kg": false
  }
}
```

### Tier 1: The Session Ledger

#### Epoch-triggered reflection

A new way (`meta/reflection/reflection.md`) fires at context-threshold boundaries. The existing `context-threshold` trigger type (used by compaction-checkpoint and memory ways) provides the timing. The reflection way fires at multiple thresholds with escalating depth:

| Context Used | Phase | Depth Guidance |
|---|---|---|
| ~30% | Orientation | What's the task? Initial direction. First decisions. |
| ~50% | Progress | What changed since orientation? Revised understanding. Open threads. |
| ~70% | Consolidation | Knowledge gained. Patterns observed. What surprised. |
| PreCompact | Handoff | Complete state for post-compaction self. Assumptions. Unfinished work. |
| PostCompact | Resume | Read ledger + orient. No writing — just restore continuity. |

The way **does not ask Claude to track epochs or write files**. The ways framework auto-generates epoch metadata as frontmatter in the ledger entry — session ID, project, epoch number, context percentage, timestamp. Claude reflects naturally in conversation using a keyphrase; the Stop hook extracts the prose and writes the entry.

#### Ledger structure

The ledger is a single chronological stream per project — not partitioned by session. Session boundaries are metadata on entries, not structural divisions. The ordering that matters is *when things happened on this project*, because the ledger is the summed experience of working on that project across all sessions.

```
~/.claude/ledger/
  {project-slug}/
    2026-04-05T1423Z_e000.md
    2026-04-05T1445Z_e001.md
    2026-04-05T1502Z_e002.md
    2026-04-07T0910Z_e003.md      ← different session, same project stream
    ...
```

Epoch numbering is monotonic across the project, not per-session. Entry 003 from session `def` follows entry 002 from session `abc` because that's the temporal order of experience. Any method that resets at session boundaries loses signal as the ledger builds.

Each entry file:

```markdown
---
session: abc123
transcript: /path/to/transcript.jsonl
epoch: 1
context_pct: 50
timestamp: 2026-04-05T14:45:00Z
phase: progress
---

Shifted from the initial plan of refactoring auth middleware to addressing
the token storage compliance issue first. Legal flagged that session tokens
in Redis aren't encrypted at rest.

Key decisions:
- Chose libsodium over OpenSSL for token encryption
- Keeping old middleware path as fallback behind feature flag

Open threads:
- Haven't tested Redis upgrade path
- Need to check mobile client token caching
```

The frontmatter is written by the hook script (which has access to session state), not by Claude. Claude writes everything below the frontmatter delimiter. This separation ensures metadata accuracy — Claude doesn't guess its epoch number or context percentage. The frontmatter is the framework's record; the prose is Claude's.

The `transcript` field links back to the source session transcript. This means the ledger entry can serve as a pointer into the full session history — the prose is a curated summary, and the transcript is the raw record. If deeper context is needed, the transcript is always reachable.

#### Capture mechanism — keyphrase extraction

Claude doesn't write to the ledger directly. Instead, the reflection way injects a prompt like:

> *Time to reflect on what's happened since the last reflection. Here's the first line of your last reflection: "{first_line}". Use the phrase "my reflections on" to begin.*

Claude reflects naturally in its response to the user — no tool calls, no file writes, no interruption. The user sees the reflection as part of the conversation.

The **Stop hook** (`check-response.sh`) then:

1. Detects that the reflection way fired this turn (session marker exists)
2. Extracts prose from the transcript between the keyphrase "my reflections on" and the next section break or response end
3. Writes the ledger entry file — framework-generated frontmatter + extracted prose
4. Appends to the ephemeral session narrative (`${SESSIONS_ROOT}/${session}/narrative.md`)

The keyphrase serves as a machine-parseable delimiter that's natural enough to appear in conversation without feeling mechanical. The ways embedding engine can score responses for the keyphrase to gate capture — if Claude uses the phrase casually without a reflection way having fired, the marker check prevents false capture.

**Total Claude-side cost: zero tool calls.** Claude just talks. The framework handles filing.

#### Additional capture channel: memory writes

Claude's auto-memory system (MEMORY.md + topic files) produces the same class of knowledge artifact as ledger entries — curated prose about what was learned, written to disk with structured frontmatter. Memory writes are triggered by explicit user requests ("remember this") or by the memory way at context thresholds (surprise test).

When KG is enabled, a PostToolUse hook on Write can detect writes to the memory directory and copy the file to the KG FUSE ingest — the same pattern as ledger entry ingestion. Memory files already have frontmatter with name, description, and type, making them well-structured KG input. This means the KG receives knowledge from two channels:

1. **Ledger entries** — periodic epoch reflections (what happened)
2. **Memory writes** — explicit or surprise-gated observations (what was surprising or important)

Both channels are fire-and-forget copies to the FUSE mount. The KG deduplicates across channels naturally.

#### Session narrative (ephemeral working copy)

During a session, entries are also appended to `${SESSIONS_ROOT}/${session}/narrative.md` — a concatenated view of entries this session. This is what Claude reads for in-session continuity (e.g., at resume after compaction). It's ephemeral (lives in XDG_RUNTIME_DIR) and regenerable from the ledger.

The resume way does not inject the entire narrative — only the last 2-3 entries plus the handoff entry. In long sessions with many compaction cycles, the narrative could grow large; injecting it all would immediately consume the fresh context window. The KG (if active) handles deeper cross-session structural context; the narrative provides only the immediate thread of continuity.

#### Epoch numbering

Epochs are monotonic across the project — the next epoch number is derived from the count of existing entries in the project's ledger directory. A new session picks up where the last session left off. This means the ledger is a continuous record of project experience, with session boundaries visible in the metadata but not in the numbering.

### Tier 2: Knowledge Graph Enhancement (Optional)

When `reflection.kg` is enabled and a knowledge graph MCP server is available, ledger entries additionally feed into the KG. The KG is a **derived view** of the ledger — never the source of truth. Not all users will have or want a KG. The ledger is fully functional without it.

#### Ingestion (file copy)

After the Stop hook writes a ledger entry, it copies the entry file to the KG's FUSE-mounted ingest directory:

```bash
KG_INGEST="${HOME}/Knowledge/ontology/${PROJECT_SLUG}/ingest"
if [[ -d "$KG_INGEST" ]]; then
  cp "$ENTRY" "$KG_INGEST/"
fi
```

The FUSE layer picks up the file, the KG processes asynchronously — decomposing into concepts, deduplicating, assigning epistemic status. No CLI invocation, no MCP call, no tool use. Just a file copy. If the KG isn't mounted or the directory doesn't exist, the `if` guard skips silently — the ledger entry exists regardless.

The ledger entry file *is* the KG input document. Same file, same format. The YAML frontmatter is ignored by the KG's text chunker; the prose below the delimiter is what gets ingested. One artifact serves both purposes.

#### Retrieval (state-transition-driven)

Ingestion and retrieval are **completely decoupled**. Searching right after ingest is just an expensive echo. The value of the KG is associative context that surfaces at *state transitions* — moments where Claude's working model shifts and prior knowledge would reshape the shift:

| Trigger | Hook Event | Why This Moment |
|---|---|---|
| **Domain entry** | Way fires (first time in session) | Cross-session experience with this domain is most valuable now |
| **Post-compaction** | PostCompact | KG provides breadth across prior sessions, not just this session's continuity |
| **New session** | SessionStart (first for project in N days) | KG provides project orientation across all prior sessions |
| **Tool failure** | PostToolUseFailure | KG may have prior experience with similar failures |

This is not RAG. There's no query to answer — the triggers are events, not questions. The KG works on a different timescale than the active session, building concepts in the background while Claude works. When a state transition fires, connections that didn't exist before have materialized. This serves as a form of **subconscious attachment** — prior session knowledge accretes silently and surfaces at natural pause points.

#### Scoping

One KG ontology per Claude project (named after the project slug). All retrieval queries scope to the project ontology. Foundational knowledge relevant to a project is ingested into that project's ontology. Concepts can point to items outside their own ontology via edges, but retrieval never crosses the project boundary.

#### Replay

The ledger enables KG regeneration. If the KG is lost or reset, ingest all ledger entries across all projects (`~/.claude/ledger/*/`). Order doesn't matter — the KG's epistemic model measures honesty and convergence, not recency, so ingesting entry 47 before entry 3 produces the same concept graph. Deduplication makes replay idempotent. The temporal ordering in the ledger serves human readability and in-session continuity, not the KG.

#### Graph maturity phases

The KG isn't static storage — it progresses through distinct phases as ledger entries accumulate, each producing a qualitatively different kind of value:

**Linear** — Early sessions. The KG registers prose as clusters of concepts. Each entry creates new nodes with sparse edges. The graph is shallow — many islands, few bridges. Retrieval returns individual concepts, not connections. Value is low but the foundation is being laid.

**Expansion** — New prose attaches to existing concepts rather than creating new ones. Evidence instances accumulate on established nodes. The graph becomes denser within domains. Retrieval starts returning concepts with multiple evidence sources, increasing confidence. The KG begins to say "you've seen this pattern before" rather than just "this concept exists."

**Convergence** — Evidence grows enough to form directed graph networks in the concept corpus. Edges between concepts gain epistemic weight. The graph structure starts to reflect real relationships — SUPPORTS, CONTRADICTS, IMPLIES — rather than just co-occurrence. Retrieval returns paths between concepts, not just individual nodes. The KG can now surface connections like "this decision supports that principle but contradicts this earlier assumption."

**Reasoning** — Node relationships, edge vector directions, and grounding scores with substantiating evidence form reasoning networks. The graph topology itself encodes arguments. High-grounding paths represent well-established reasoning chains; contested edges represent open questions. Retrieval at this phase provides not just context but structured reasoning — "here's why X, supported by evidence from sessions 3, 7, and 12, with a counterpoint from session 9."

Each phase increases the value-to-cost ratio of the reflection investment. The linear phase costs the same as the reasoning phase in tokens spent, but the reasoning phase returns qualitatively richer context.

#### Why KG over editorial curation (the ephemera problem)

Memory curation approaches fall into two categories from the formal literature: **editorial** (the AGM framework — Alchourrón, Gärdenfors, Makinson, 1985) which performs minimal consistent revision, and **evidential** (Dempster-Shafer theory, 1976) which preserves contradictions and scores the balance of evidence. Claude Code's built-in memory curation ("dream mode") is editorial. The KG is evidential.

When editorial curation encounters "X is true" from session 3 and "X is false" from session 12, it must pick one. The *fact that understanding shifted* is itself knowledge, and it's destroyed. The KG keeps both assertions with their evidence and scores the balance through epistemic status classifications that editorial curation cannot represent:

- **AFFIRMATIVE** — consistent evidence across sessions (editorial curation's only output)
- **CONTESTED** — evidence in both directions (more informative than either assertion alone)
- **CONTRADICTORY** — later evidence actively contradicts earlier (tells you understanding shifted)
- **INSUFFICIENT_DATA** — not enough evidence to classify (tells you this is unexplored)

The deeper problem is ephemera. Editorial curation keeps big signals and discards small ones. But minor observations compound — a low-signal concept from session 5 might become the key insight in session 40, but only if it survived 35 curation cycles. The KG preserves it as a low-ranked node with live edges, costing nothing in attention budget until a future session produces a structurally adjacent concept. The minor signal surfaces not because anyone remembered to preserve it, but because the graph topology kept it connected.

The KG's composed scoring reinforces this. A concept that's individually weak but connected to three strong concepts along a coherent semantic axis is more informative than its individual score suggests. Polarity analysis projects concepts onto axes — a minor concept near the midpoint of a contested axis is the most interesting concept in the graph, not the least.

The scaling curves go in opposite directions. As data grows over months, editorial curation becomes increasingly destructive — each pass loses more ephemera. The KG becomes increasingly valuable — each new concept has more neighbors to connect to, more axes to project onto, more composed scores to participate in. The ledger preserves raw signal; the KG preserves the *relationships between signals* — including the minor ones that editorial curation discards.

### Hook Changes

#### New hook entries

**PreCompact** — Fires the handoff reflection phase:

```json
"PreCompact": [
  {
    "hooks": [
      {"type": "command", "command": "${HOME}/.claude/hooks/ways/check-precompact.sh"}
    ]
  }
]
```

**PostCompact** — Fires the resume phase:

```json
"PostCompact": [
  {
    "hooks": [
      {"type": "command", "command": "${HOME}/.claude/hooks/ways/check-postcompact.sh"}
    ]
  }
]
```

**PostToolUseFailure** — Injects error-context ways when tools actually fail:

```json
"PostToolUseFailure": [
  {
    "matcher": "Bash",
    "hooks": [
      {"type": "command", "command": "${HOME}/.claude/hooks/ways/check-error-post.sh"}
    ]
  }
]
```

#### Existing hook improvements

**Conditional `if` on PreToolUse:Bash** — Reduces hook overhead by skipping read-only commands:

```json
{
  "matcher": "Bash",
  "if": "Bash(git *) || Bash(make *) || Bash(docker *) || Bash(npm *) || Bash(cargo *) || Bash(kg *)",
  "hooks": [{"type": "command", "command": "...check-bash-pre.sh"}]
}
```

**Async on non-blocking hooks** — Stop hook and marker writes don't need to block:

```json
{"type": "command", "command": "...check-response.sh", "async": true}
```

### New Ways

| Way | Trigger | Purpose |
|---|---|---|
| `meta/reflection/reflection.md` | context-threshold (30/50/70%) | Write epoch entries to ledger |
| `meta/reflection/handoff.md` | PreCompact (direct injection) | Full-depth handoff before compaction |
| `meta/reflection/resume.md` | PostCompact (direct injection) | Read ledger, orient, resume |

### New/Modified Scripts

| Script | Hook Event | Purpose |
|---|---|---|
| `check-response.sh` (modified) | Stop | Extracts keyphrase-delimited reflection, writes ledger entry, copies to KG FUSE if available |
| `check-precompact.sh` | PreCompact | Fires handoff way, passes epoch state |
| `check-postcompact.sh` | PostCompact | Fires resume way with ledger path |
| `check-error-post.sh` | PostToolUseFailure | Fires error-context ways on failure |
| `ledger-replay.sh` | (utility) | Replays ledger entries into KG |

## Consequences

### Positive

- Sessions produce durable knowledge artifacts (ledger entries) that survive beyond the session
- Progressive externalization captures knowledge while context is rich, not under compaction pressure
- The ledger is append-only and replayable — a project's session history is always available
- Each tier is independently activatable — ledger works without KG, current system works without ledger
- Epoch metadata is framework-generated, not Claude-generated — accurate by construction
- PreCompact/PostCompact hooks provide natural timing for handoff and resume
- `if` field and `async` on existing hooks reduce latency on every turn
- When KG is active, sessions get smarter over time as epistemic trust builds across sessions

### Negative

- Ledger entries consume disk space over time (mitigated: prose entries are small, ~500 bytes each)
- The reflection way adds content to Claude's response at threshold boundaries (mitigated: keyphrase capture is conversational, not a tool-call interruption)
- Requires `ways` binary awareness of ledger directory for epoch numbering
- Stop hook must parse transcript to extract keyphrase-delimited prose (mitigated: simple regex on a known marker)
- When KG is active: depends on FUSE mount availability (mitigated: `if -d` guard skips silently, ledger is unaffected)

### Neutral

- The compaction-checkpoint way (context-threshold at 95%) continues to handle user-facing compaction dialogue — reflection operates at lower thresholds for knowledge capture, not compaction coordination
- Auto-memory (MEMORY.md) remains for surprises and cross-session pointers — the ledger captures the mundane connective tissue that memory intentionally ignores
- The epoch counter for checks (ADR-103) is independent — it counts hook events, not reflection entries. The ledger epoch is a different concept (knowledge externalization events)
- Opens the question of ledger pruning/archival — we defer this (append-only until proven problematic)
- The KG's epistemic status starts low (few evidence instances) and builds confidence over time — this is correct behavior, not a deficiency

## Alternatives Considered

- **Single narrative file per session** — Rejected because individual entry files enable atomic KG ingestion and clean replay. A single file requires parsing to find entry boundaries.

- **Claude tracks its own epochs** — Rejected because Claude's self-reported metadata is unreliable after compaction. The framework has authoritative access to session ID, context percentage, and epoch count via session state files. Generating frontmatter externally ensures accuracy by construction.

- **KG-only (no ledger)** — Rejected because it creates a dependency on KG availability for knowledge continuity. The ledger is the source of truth; the KG is a derived, regenerable view. If the KG goes down, the ledger still provides session history and replay capability.

- **Ledger in project `.claude/` directory** — Rejected because the ledger is user-scoped knowledge about project work, not project configuration. Committing session narratives to a shared repo exposes working notes. The `~/.claude/ledger/` location keeps it user-private alongside other user-scoped state.

- **Write entries at fixed turn counts** — Rejected because turn count doesn't track context consumption. 10 turns of complex tool use consumes more context than 50 turns of brief Q&A. Context-threshold triggers track the resource that actually matters.

- **Claude writes ledger entries via Write tool** — The initial design had the reflection way instruct Claude to call Write with a specific file path. Rejected in favor of keyphrase extraction from natural conversation. The Write approach interrupts Claude's flow with a tool call, requires the way to communicate file paths, and makes the reflection feel mechanical. Keyphrase capture is invisible — Claude just reflects in conversation and the framework handles filing.

- **Claude-driven KG ingestion via MCP tool calls** — Considered having Claude call `session_ingest` or `ingest` directly. Rejected for the normal flow because it costs tool calls and context window tokens for something the framework can handle invisibly. The FUSE file copy achieves the same result with zero Claude involvement. Note: Claude can still use KG MCP tools directly for advanced introspection and deep reasoning within the knowledge graph — this is an intentional capability but not part of the standard reflection flow.

- **Search immediately after ingest (RAG pattern)** — The naive design searches the KG right after ingesting an entry. Rejected because this is an expensive echo — the search returns concepts derived from what Claude just wrote. The value of the KG is *associative context from prior sessions*, not retrieval of current knowledge. Ingest and retrieval are decoupled: ingest is periodic (epoch boundaries), retrieval is event-driven (state transitions like domain entry, post-compaction, tool failure). This is not RAG.

- **Configurable ontology sets per project** — Considered allowing projects to declare a list of ontologies to query (e.g., `["claude-config", "cognitive-frameworks"]`). Rejected in favor of one ontology per project. Foundational knowledge that matters to a project is ingested into the project's ontology. The KG deduplicates at the concept level, and edges can cross ontology boundaries naturally. Simpler model: the project slug *is* the query scope, no configuration needed.

- **Cross-project KG queries** — Considered allowing retrieval to search across all ontologies. Rejected because it violates project scoping — sessions from project A shouldn't bleed into project B's context. Cross-ontology edges exist at the concept level (a concept can point to concepts in other ontologies), but retrieval is always scoped to the current project's ontology.

## Related Projects

- [aaronsb/agent-ways](https://github.com/aaronsb/agent-ways) — The ways framework this ADR extends
- [aaronsb/knowledge-graph-system](https://github.com/aaronsb/knowledge-graph-system) — The KG system used for Tier 2

## Theoretical References

- Oberauer (2002) — Concentric model of working memory: capacity-precision tradeoffs across nested tiers
- Shannon/Berger — Rate-distortion theory: fixed information budget allocated between fidelity and coverage
- Wille (1982) — Formal Concept Analysis: coverage gap detection from overlapping concept extents
- Dempster-Shafer (1976) — Evidential reasoning: preserve contradictions, score balance of evidence
- AGM framework (1985) — Belief revision via minimal consistent editing (the editorial model we contrast against)
- Tishby et al. (1999) — Information Bottleneck: attention as selection of operating point on R(D) curve
- Behrouz et al. (2025) — Google Titans: multi-tier memory (short-term/long-term/persistent) with surprise-gated writes. Rhymes with this design's tier structure, though Titans operates within model architecture while this design operates via external infrastructure.

## Implementation Plan

### Phase 1: Hook improvements (no binary changes)
1. **`if` field on PreToolUse:Bash** — Config-only change to reduce hook overhead
2. **`async: true`** on Stop hook and non-blocking markers — Config-only latency win
3. **PreCompact/PostCompact hook entries** — New hooks in settings.json

### Phase 2: Ledger (Tier 1)
4. **Ledger infrastructure** — Directory structure, entry format, project slug derivation in `ways` binary
5. **Reflection way** (`meta/reflection/reflection.md`) — Keyphrase-prompted reflection at context thresholds
6. **Stop hook extension** — Keyphrase extraction from transcript, ledger entry writing with framework frontmatter
7. **Handoff/resume ways** — `meta/reflection/handoff.md`, `meta/reflection/resume.md`
8. **Shell scripts** — `check-precompact.sh`, `check-postcompact.sh`
9. **Observe** — Run for several sessions, tune context-threshold boundaries and keyphrase discrimination

### Phase 3: KG enhancement (Tier 2, optional)
10. **FUSE integration** — Copy ledger entries to `~/Knowledge/ontology/{project}/ingest/` if mounted
11. **Retrieval triggers** — KG search at domain entry, post-compaction, session start
12. **Replay utility** — `ledger-replay.sh` for KG regeneration from ledger
13. **Observe** — Measure KG injection information density and epistemic trust growth

### Phase 4: Additional hook diversification
14. **PostToolUseFailure** — Error-context way injection via `check-error-post.sh`
15. **CwdChanged / FileChanged** — Project-local way activation, corpus rebuild on way edits
