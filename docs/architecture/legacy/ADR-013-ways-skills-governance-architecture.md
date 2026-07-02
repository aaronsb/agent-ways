---
status: Accepted
date: 2026-02-09
deciders:
  - aaronsb
  - claude
related:
  - ADR-004
  - ADR-005
  - ADR-200
---

# ADR-013: Ways, Skills, and Governance Architecture

> **Refined by [ADR-200](../governance/ADR-200-compliance-claims-and-session-derived-findings.md).**
> This ADR's core insights stand — ways as *activation cues* for latent knowledge, the
> Type 1–4 way classification, the author-as-compiler model, and the formation intuition
> in its "Shift Lead Wisdom" type. But its **epistemics are corrected**: wherever it calls
> provenance "evidence," "auditability," "proof," or what "turns Claude from a hack into a
> professional," read **claim**. A hand-authored mapping is a control-*design* assertion
> (SOC 2 Type I), not an assessed **finding** (SOC 2 Type II); a citation on demand is an
> assertion awaiting a finding, not proof. Its analogies (kitchen poster, shift lead, FAA
> tool) are illustration; the grounded vocabulary — *first line*, *tacit knowledge*,
> *claim/finding* — lives in ADR-200. Mechanism has also moved on: `governance.sh` is now
> `ways governance`/`ways-audit` (ADR-111), and provenance lives in a `provenance.yaml`
> sidecar, not way frontmatter (ADR-110).

## Context

An evaluation of Anthropic's official Claude Code skills system against the custom "ways" system revealed both complementary functions and overlapping capabilities. This ADR documents the architectural relationship between skills, ways, and governance provenance — and establishes a framework for how they compose into a professional-grade agent governance stack.

The evaluation was prompted by genuine doubt: do ways and skills actually complement each other, or is the ways system duplicating what Anthropic's official primitives now provide?

## The Three-Layer Professional Practice Stack

### Layer 1: Skills — Capability ("Here's how to use the tool")

Skills are what the agent CAN do. They represent organized knowledge and specific capabilities.

**Analogy**: A mechanic's toolbox with foam cutouts, labels, and knowledge of what every tool does and how to use it.

**Characteristics**:
- Semantically discovered by Claude based on intent (description matching)
- User-invocable via `/slash-commands`
- Can restrict tool access (`allowed-tools`)
- Can fork into subagent contexts (`context: fork`)
- Distributable via plugins/marketplace
- Follow the Agent Skills open standard (agentskills.io)

**What skills cannot do**:
- Fire before a specific tool executes (no PreToolUse access)
- Fire based on file patterns being edited
- Fire based on session state (context threshold, file existence)
- Session-gate (fire once then stay silent)
- Differentiate agent/teammate/subagent scopes
- Carry governance provenance metadata

### Layer 2: Ways — Policy ("Here's why you must account for every tool before closing the job")

Ways are what the agent MUST do, and the practical guidance for how to do it. They represent codified policy in actionable form.

**Analogy**: The rule that says you must account for every tool before closing the job — because if one's missing, it might be in the wing, the engine, or the break room, and you do not close out the job until it's located.

**Characteristics**:
- Triggered by actions (tool use, file edits, commands), keywords, or state conditions
- Session-gated: fire once per session via marker system
- Support macros for dynamic context (shell scripts that run when the way triggers)
- Scope-filtered: can target agent, teammate, or subagent contexts
- Domain-organized with enable/disable via `ways.json`
- Built on Claude Code's hook primitives (PreToolUse, UserPromptSubmit, SessionStart, etc.)

**What ways cannot do**:
- Be invoked by the user as a slash command
- Restrict which tools Claude can use
- Fork into isolated subagent contexts
- Be distributed via plugin marketplace
- Appear in Claude's context budget for auto-discovery

### Layer 3: Provenance — Institutional Memory ("Because FAA regulation XYZ, which exists because...")

Provenance is the evidence chain from repeated failure → institutional learning → codified standard → policy → way → agent behavior.

**Analogy**: The FAA regulation that requires tool accountability, which exists because in a specific historical incident, a tool was left inside an aircraft and caused a failure. The regulation is the codified institutional memory of "this went wrong enough times that we wrote it down."

**Characteristics**:
- Embedded in way frontmatter as `provenance:` blocks
- Stripped before context injection (zero runtime token cost)
- Queryable via the `governance-cite` skill
- Maps specific way directives to specific control requirements with justifications
- Creates a self-supporting network: multiple controls cross-referencing the same practice

**What a claim provides** *(epistemics corrected by ADR-200 — this section originally read "What provenance provides")*:
- A statement that a way is *designed* to address specific controls — a control-design **claim** (SOC 2 Type I), not proof that it does.
- Traceability of intent: the lookup can surface which controls a way *claims* to address. This is an asserted mapping, not evidence — a citation on demand is a claim awaiting a **finding**, not a satisfied "prove it."
- Explicitness: a way with a claim states its intended control alignment, which makes that alignment *assessable*; substantiating it requires a session-derived finding (ADR-200).

#### The Authorship Model: Provenance as Design-Time Artifact

Provenance is stripped before the way reaches Claude's context — it never sees the control IDs, the justifications, or the regulatory citations at runtime. What Claude sees is the way body: the poster on the wall. This creates a specific authorship dynamic with three key properties:

**Latent space activation.** Claude was almost certainly trained on the full text of NIST 800-53, OWASP, ISO 27001, and other regulatory corpora. That knowledge exists in the model's latent space but isn't precisely retrievable on demand. Provenance metadata serves as the design-time bridge: the way author maps their guidance to specific controls, which ensures the way content contains the right activation cues to surface that latent regulatory knowledge at runtime. The provenance doesn't teach Claude the controls; it ensures the way content is written in a way that *activates* what Claude already knows.

**The author as compiler.** The way author carries the critical responsibility of *compiling* governance into actionable guidance. The provenance block is the author's working notes — "I wrote this bullet point because NIST CM-3 requires change classification" — but the bullet point itself must stand alone as useful, activating guidance. The author bridges the gap between regulatory language and practitioner language. A bad author writes "comply with CM-3"; a good author writes "use conventional commits with type prefixes" and traces it back to CM-3 in the provenance. The way is the compiled thought; the provenance is the source code.

**Epistemic position determines authorship direction.** A developer writing a way compiles from experience upward — "this is how I've seen it done well." A security engineer compiles from controls downward — "this is what the regulation requires, expressed as practitioner guidance." A compliance officer might write the provenance first and derive the way content from it. The direction doesn't matter; what matters is that the compiled output (the way body) lands at the right level of abstraction for the practitioner reading it.

In practice, Claude itself is likely to be the author of most ways — but the human provides the epistemic grounding: which controls matter, what governance posture the framework should carry, what scope to cover. The human sets the intent and the governance coverage; Claude compiles it into effective activation cues. This is a collaborative authorship model where the human's domain knowledge of *what matters* meets Claude's ability to express it in a form that activates its own latent knowledge effectively.

## The Three Types of Ways

Not all ways serve the same function. Forcing governance provenance onto all ways would be dishonest. The three types are:

### Type 1: Kitchen Poster (Compiled Governance)

Practical, actionable guidance that traces directly to regulatory controls. The worker gets "use parameterized queries" on the poster. Corporate policy (OWASP A03, NIST IA-5) is several layers deep behind that poster. The provenance block captures the chain.

**Should have provenance**: Yes — that's their primary purpose.

**Current ways in this category** (all with provenance):
- `softwaredev/commits` → NIST CM-3, SOC 2 CC8.1, ISO 27001 A.8.32
- `softwaredev/security` → OWASP A03, NIST IA-5, CIS v8 16.12, SOC 2 CC6.1
- `softwaredev/quality` → ISO 25010, NIST SA-15, IEEE 730
- `softwaredev/deps` → NIST SA-12, OWASP A06, NIST RA-5
- `softwaredev/testing` → NIST SA-11, IEEE 829, ISO 25010
- `softwaredev/config` → NIST CM-6, CIS v8 4.1, NIST IA-5
- `softwaredev/errors` → OWASP A09, NIST SI-11, NIST AU-3
- `softwaredev/ssh` → NIST AC-17, NIST IA-2, NIST IA-5
- `softwaredev/release` → NIST CM-3, SOC 2 CC8.1, NIST SA-10
- `softwaredev/adr` → NIST CM-3, ISO 27001 A.5.1, NIST PL-2
- `softwaredev/github` → SOC 2 CC8.1, NIST CM-3, ISO 27001 A.8.32
- `meta/knowledge` → ISO 9001 7.5, ISO 27001 5.2, NIST PL-2

### Type 2: Shift Lead Wisdom (Experience-Derived)

Practical wisdom that exists BECAUSE of the governance environment but isn't DIRECTLY traceable to a specific control. An experienced worker knows "when the fryer oil looks like that, change it" — not because a regulation says so, but because working in a regulated kitchen long enough teaches you patterns that prevent problems.

**Should have provenance**: No — forcing it would be dishonest. These are reactions to governance, not implementations of it.

**Graduation path**: Type 2 ways can graduate into Type 1 when experience gets codified. In highly regulated industries, "shift lead wisdom" routinely becomes SOPs and best practice standards after enough incidents. The fryer oil example becomes "change oil every N hours per health code §X" once someone gets sick. The architecture supports this migration naturally — add a provenance block and the way changes type. The classification isn't permanent; it reflects the current state of formalization. A way without provenance today may earn it tomorrow when the pattern it captures gets traced to a control.

**Current ways in this category**:
- `softwaredev/debugging` — experience-based debugging process
- `softwaredev/patches` — "never hand-write patches" is experience, not regulation
- `softwaredev/performance` — performance analysis workflow
- `softwaredev/api` — API design omissions Claude commonly makes
- `softwaredev/design` — design discussion framework
- `softwaredev/migrations` — schema change practices

### Type 3: HQ Policy Manual (Governance About Governance)

Meta-governance: how to write the poster, how to structure policy, how to maintain the traceability matrix.

**Should have provenance**: Special case — self-referential. `meta/knowledge` already maps to ISO 9001 7.5 and NIST PL-2, which is appropriate because the way system itself is a documented information management process.

**Current ways in this category**:
- `meta/knowledge` — how ways work, how to author them
- `meta/skills` — how skills work
- `meta/introspection` — session reflection and learning capture

**Related skills**: `governance-cite` — the query interface into provenance data (a Layer 1 skill that reads Layer 3 metadata)

### Type 4: Plumbing (Operational, Not Governance)

Session lifecycle management. Not governance, not experience, just Claude Code operational mechanics.

**Should have provenance**: No — these don't participate in governance.

**Current ways in this category**:
- `meta/memory` — MEMORY.md management
- `meta/subagents` — delegation guidance
- `meta/teams` — team coordination norms
- `meta/todos` — context-threshold task list enforcement
- `meta/tracking` — cross-session state

## How Ways Work as a System

Ways are a trigger-dispatched, session-gated, context-injection system with a multi-modal retrieval function. They resemble RAG but differ in a fundamental way:

| | Traditional RAG | Ways |
|---|---|---|
| Corpus | Documents | ~30 guidance files + dynamic macro outputs |
| Trigger | Query embedding similarity | Multi-modal matching (regex, semantic, model, state) |
| Injection | Into LLM context | Into Claude's context via hooks |
| Frequency | Every query | Once per session (session-gating) |
| Purpose | Compensate for the model NOT KNOWING | ACTIVATE what the model ALREADY KNOWS |

The critical insight: way content is an activation signal, not a knowledge injection. Claude — and any sufficiently large LLM — has read NIST 800-53, OWASP, ISO 27001, IEEE standards, and other regulatory corpora during training. That knowledge exists in the latent space but isn't precisely retrievable on demand. Ways carry just enough ablated context to prime that existing deep knowledge into active use. This is why ways should be concise — you need "conventional commits, type prefix, atomic changes, rationale in body," and the model's training fills in the depth. More context isn't better; the right context is better.

The provenance layer serves a different audience entirely. The way content primes Claude (the agent). The provenance block (stripped before injection, zero context cost) serves the human — via `governance-cite` — who asks "prove it." Two retrieval paths for two consumers from one source file.

### Multi-Modal Retrieval Function

Ways use four matching strategies instead of embeddings:

1. **Regex** (default, fast): pattern matching against prompts, commands, file paths
2. **Semantic** (BM25): Term-frequency scoring with IDF weighting — no embeddings, no infrastructure dependency
4. **State triggers**: session conditions (context threshold, file existence, session start)

### Session-Gating

Each (way, session) pair has a marker at `/tmp/.claude-way-{domain}-{way}-{session_id}`. Once a way fires, the marker prevents it from firing again until session restart or compaction. This is fundamentally different from skills (always available) and traditional RAG (retrieves every query).

Exception: context-threshold triggers bypass markers and repeat every prompt until a task list is created — enforcement, not education.

### Scope Filtering

Ways differentiate between agent (main session), teammate (team member), and subagent (quick delegate) contexts. This prevents, for example, three teammates simultaneously writing MEMORY.md or subagents receiving delegation guidance about delegation.

## The Complementary Relationship

Skills and ways complement at the edges and overlap in the middle.

### Where the distinction is sharp

**Ways can do, skills cannot**: Fire before `git commit` runs; fire when editing `.env` files; fire at context threshold; fire once then stay silent; differentiate scopes; carry governance provenance.

**Skills can do, ways cannot**: User-invocable slash commands; restrict tools; fork into subagent contexts; distribute via plugins; always-in-context description for auto-discovery.

### Where they overlap

Both can inject contextual guidance when relevant. Both support dynamic context (macros vs `!`command``). Both do semantic matching (BM25 vs description-based). Pure reference-content ways with semantic matching and no macro, no governance, no scope filtering are functionally similar to `user-invocable: false` skills.

### Why the overlap doesn't invalidate ways

The overlap zone is "inject contextual guidance." But the ways that justify the system are the ones skills CAN'T replicate:
- PreToolUse-triggered guidance (commit formatting on `git commit`)
- State-triggered enforcement (context-threshold nag)
- Governance-traced policy (provenance blocks with NIST/OWASP/ISO mappings)
- Scope-filtered team coordination
- Macro-based dynamic context (repo health checks, file scanning)

### First-line formation, not proof

*(Reframed per ADR-200.)* A way carrying a claim knows *which control shape* it steers toward — but citing that claim is an assertion, not proof. In audit terms this is **first-line** work (the Three Lines Model): shaping how risk is managed *in the doing of the work*, distinct from the third-line assurance that independently proves conformance.

- Without claims: guidance is followed because the model was trained on good practice.
- With claims: guidance also *states* the control shape it intends — so a later **finding** can test whether the work actually took that shape.

The claim layer makes intended alignment explicit and assessable. It does not, by itself, turn assertion into evidence — that is precisely the correction ADR-200 exists to make.

## The Governance Control Surface

The user controls their governance posture through `ways.json`:

```json
{
  "disabled": ["itops", "experimental"]
}
```

Enabling/disabling domains determines how much governance the agent carries. This is the control surface — not a one-size-fits-all system, but a configurable governance posture that the framework user chooses.

Ways that are governance-relevant (Type 1) carry provenance. Ways that are experience-derived (Type 2) don't. The presence or absence of provenance in a way file indicates which type it is. No separate classification system needed.

## Decision

### 1. Skills and ways are complementary layers of a professional practice stack

Skills = capability (Layer 1), Ways = policy (Layer 2), Provenance = institutional memory (Layer 3). They are not competing systems — they serve different layers.

### 2. Keep governance provenance commingled in way files (Option A)

The provenance travels with the guidance it justifies. One file, one truth. The `governance.sh` matrix provides the cross-cutting auditor's view derived from the source of truth. No separate governance overlay files.

### 3. Recognize three types of ways (plus plumbing)

- **Type 1 (Kitchen Poster)**: Compiled governance — SHOULD have provenance
- **Type 2 (Shift Lead Wisdom)**: Experience-derived — should NOT have provenance
- **Type 3 (HQ Policy Manual)**: Meta-governance — special case (self-referential)
- **Plumbing**: Operational — doesn't participate in governance

### 4. Fill the provenance gap on Type 1 ways

All active Type 1 ways now carry provenance. The governance matrix covers 12 ways with 37 control claims and 93 justifications across NIST, OWASP, ISO, SOC 2, CIS, and IEEE frameworks. Provenance authoring is a metadata task — the way body (practitioner guidance) already existed; the provenance block traces it back to the controls it implements.

### 5. Way content is an activation cue, not knowledge injection

Ways should be concise — just enough ablated context to activate Claude's latent training knowledge. The provenance serves humans via governance-cite. Two consumers, two paths, one source file.

### 6. Governance is domain-agnostic

The way/provenance architecture works for any governance domain (software engineering, financial compliance, data privacy, operational safety). The current implementation covers software engineering controls. Future domains (enabled via ways.json) can carry their own provenance to their own regulatory bodies.

## Consequences

### Positive
- Clear architectural rationale for why both skills and ways exist
- Framework for deciding which ways need provenance (Type 1) vs which don't (Type 2/3/plumbing)
- Establishes ways as codified policy, not just "contextual guidance"
- The governance-cite skill becomes more valuable as provenance coverage expands
- Domain-agnostic design allows governance expansion without architectural changes

### Negative
- Provenance authoring requires domain expertise (control knowledge + implementation knowledge) — the author must be able to compile in both directions
- More provenance = more to maintain when controls or guidance change
- The compiler metaphor cuts both ways: a good author compiles governance into effective activation cues; a bad author either writes "comply with CM-3" (too abstract, no activation) or writes guidance that activates the wrong latent knowledge. The way system is only as good as its authors.

### Neutral
- Pure reference-content ways (Type 2, semantic match, no macro) could migrate to skills/rules over time as Anthropic adds features, but this isn't urgent
- The `once` field in skill/agent hook frontmatter moves Anthropic incrementally toward session-gating, narrowing one differentiator
- itops domain remains disabled; its ways would follow the same type classification when enabled

## Alternatives Considered

### Organize ways by governance body (Option B)
Rejected: directory structure should match the user's mental model ("what work am I doing") not regulatory taxonomy ("what regulation am I following"). Nobody enables governance by body.

### Separate governance overlay files (Option C)
Rejected: provenance must travel with the guidance it justifies. Separated mappings drift and become stale. One file, one truth.

### Replace ways with official skills/rules
Rejected: skills cannot fire on tool use, cannot session-gate, cannot carry provenance, cannot scope-filter. The unique value of ways is precisely what skills cannot do.

### Replace ways with raw hooks
Possible but rejected: ways provide an abstraction (session-gating, multi-mode matching, macros, governance provenance, scope filtering, domain organization) that would need to be rebuilt in every hook script. The abstraction layer is the value.

## Traceability: a live query, not a frozen table

*(Reframed per ADR-200.)* This ADR originally carried a point-in-time table marking each
way's provenance as "Complete." That is retired on two counts. Coverage is a **live
query** now (`ways governance`, moving to the `ways-audit` binary) — not a snapshot
frozen into a decision record. And under ADR-200 a way with a hand-authored mapping
carries a **claim**, not a completed control; "Complete" was the
coverage-equals-compliance overclaim. A claim is complete only once a session-derived
**finding** substantiates it. Run the tool for current coverage, and read any count it
reports as *claims made*, not *conformance achieved*.

## References

- **Kahneman, D.** (2011). *Thinking, Fast and Slow.* System 1 (fast/intuitive) and System 2 (slow/deliberative) as models of individual cognition. The ways architecture extends this: System 1 maps to Claude's latent training patterns; System 2 to deliberative reasoning when prompted.
- **Beer, S.** (1972). *Brain of the Firm.* The Viable System Model (VSM) describes how organizations maintain viability through recursive system layers including policy (System 3) and identity/ethos (System 3*). The three-layer stack in this ADR — skills (capability), ways (policy), provenance (institutional memory) — parallels VSM's separation of operational, regulatory, and normative functions. System 3 corresponds to institutional governance codified in standards bodies; System 3* corresponds to the ways+provenance mechanism that bridges institutional cognition into an individual agent's decision process.
- **NIST SP 800-53 Rev. 5** — Security and privacy controls referenced throughout provenance blocks
- **OWASP Top 10 (2021)** — Application security risks referenced in security, errors, and dependency provenance
- **ISO/IEC 27001:2022** — Information security management controls referenced in commits, ADR, and GitHub provenance
- **ISO/IEC 25010:2011** — Software quality characteristics referenced in quality and testing provenance
- **SOC 2 (AICPA)** — Trust services criteria referenced in change management provenance
- **CIS Controls v8** — Security implementation guidance referenced in config and security provenance
- **IEEE 730/829** — Software quality assurance and test documentation referenced in quality and testing provenance
