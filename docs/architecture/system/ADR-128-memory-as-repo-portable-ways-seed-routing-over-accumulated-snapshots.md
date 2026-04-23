---
status: Draft
date: 2026-04-22
deciders:
  - aaronsb
  - claude
related:
  - ADR-125
---

# ADR-128: Memory as repo-portable ways — seed routing over accumulated snapshots

## Context

Claude Code ships an official auto-memory feature (v2.1.59+, browsable via the `/memory` slash command). It stores `MEMORY.md` at `~/.claude/projects/<project-normalized>/memory/MEMORY.md`, loads the first 200 lines or 25 KB unconditionally at every session start, and encourages the session to review and extend the file. Topic files as `.md` siblings in the same directory load on demand via pointer lines in `MEMORY.md`. Reference: [Claude Code Memory docs](https://code.claude.com/docs/en/memory).

Anthropic has also shipped, without public announcement, a **periodic memory-compaction cycle**. After roughly 24 hours + 5 sessions of accumulation, a 4-phase pass runs — orient, gather from session transcripts, consolidate, prune — rewriting `MEMORY.md` and its topic files to stay under the 200-line load window. Anthropic's internal/branded name for this is "Auto-Dream" (toggleable via `/memory`; manual trigger `/dream` on gradual rollout). Third-party reference: [claudefa.st on auto-dream](https://claudefa.st/blog/guide/mechanics/auto-dream). The mechanism is memory compaction — it makes accumulated memory tidier; it does not send consolidated knowledge anywhere other than back into memory.

Observed across recent instances, the capture surface this produces is elaborate, persistent, and frictionless — Claude readily writes substantial entries without external prompting. The compaction cycle's quiet ship signals that Anthropic is doubling down on memory as a capture medium with self-organizing behavior.

Three failure modes follow from accepting the default.

**1. Staleness overrides reality.** `MEMORY.md` loads unconditionally into every session. A memory entry capturing yesterday's state silently outranks today's source of truth. 2026-04-22 provides a concrete case: a design note in this repo (`docs/design-notes/post-adr-126-simplification.md`) claimed certain `Curve` variants were "unused in the tree" — accurate for way frontmatter files but wrong for Rust code, where `ActionPotential` is `default_sensor_curve()` (`tools/sensor-trait/src/lib.rs:220`) and attend's engagement curve (`tools/attend/src/cmd/run.rs:177`). A memory entry pointed at the note. A session following the memory would have deleted live code. `grep` would have caught it in 10 seconds; the memory didn't.

**2. Short-circuiting discipline.** Memory's low friction is not a feature — it is the problem. Every elaborate memory entry is an ADR, way, design note, GitHub issue, PR description, or commit message that *didn't get written* because the model satisfied its capture instinct cheaply. The friction enforced by those artifacts — naming, frontmatter, lint, review — exists precisely because that friction forces thinking memory would skip.

**3. Per-instance cage.** Auto-memory is scoped to a Claude instance and the project-path normalization. It does not travel with the repo. Teammates, CI, other Claude runs on the same repo from a different path — none see it. The project's accumulated "memory" is invisible to everyone who wasn't the instance that wrote it. (This is orthogonal to `CLAUDE.md`, which is human-authored, repo-committed, and loaded separately and in full at session start. `CLAUDE.md` is not auto-memory.)

These failure modes are *disproportionately acute* for projects with their own capture discipline (ADRs, ways, enforced PR review, commit-message conventions). For a bare Claude Code session with no such discipline, memory's competing capture surface is unobtrusive — a few scribbled notes, no downstream cost. For a repo that already has friction-enforcing artifacts, memory's low-friction shortcut actively erodes the discipline those artifacts encode. The memory system also exposes **no programmatic integration hooks** — no `memoryWrite` intercept, no API, no per-project steering beyond the binary `/memory` toggle. A harness-level response must therefore work by *observing and rewriting* `MEMORY.md` rather than by hooking memory events. This ADR's mechanism takes that constraint as given.

## Decision

**Redirect Claude Code's official auto-memory slot rather than fight it.** Seed `MEMORY.md` with routing guidance that treats project-scoped knowledge as belonging in repo artifacts (ways, ADRs, design notes, GitHub issues, PR descriptions, commit messages, and other friction-enforcing artifact classes a project may adopt). Auto-memory remains writable (we do not disable it or modify the feature), but the *first bytes Claude reads at session start* — within the official 200-line / 25 KB load window — frame memory as narrow: short cross-project user facts, nothing that has a better home.

Mechanism:

**a. Seed template** shipped with the ways tooling. Opens with the "Memory short-circuits discipline" thesis, names the friction-enforcing artifacts, includes an anti-rationalization table pre-empting common capture-skip patterns.

**b. Frontmatter-bearing identification.** The seed begins with YAML frontmatter:

```yaml
---
seed: claude-code-memory
seed-version: 1
---
```

**c. Byte-equality integrity check against an embedded canonical.** The canonical seeded body is baked into the ways binary at build time via Rust's `include_str!`. The binary physically carries the canonical bytes, so verification is pure byte comparison — no hash, no runtime crypto dependency, no "chasing" a moving hash value. Scoping the compared region to the body between frontmatter and the `## User Context` heading means user-added entries below that heading are orthogonal to the integrity check and preserved across re-seeds.

**d. SessionStart integration via `ways init`** runs on every Claude invocation. The existing `ways init` subcommand (already registered in `SessionStart` settings.json hooks) now also verifies the seed:

- `MEMORY.md` missing → write current seed.
- Frontmatter parses + `seed == claude-code-memory` + `seed-version` matches + extracted body bytes equal `canonical_body()` → no-op.
- Any check fails → save unified diff of current content to `MEMORY.diff.YYYY-MM-DD.NNN.md` (serial increment), write fresh seed preserving everything from `## User Context` onward. If the `## User Context` marker is structurally missing, diff the whole file and rewrite in full.

**e. Memory-seed files** are a new artifact class — frontmatter-bearing, byte-compared against an embedded canonical, managed by ways tooling, destination the harness's memory slot rather than the ways tree. The vocabulary matters: a future reader sees `seed: claude-code-memory` and has a word for what they're looking at.

**f. Defense-in-depth via the memory way.** `hooks/ways/meta/memory/memory.md` carries the same routing table and anti-rationalization block. Surfaces when the way fires mid-session on "remember this" prompts, after the seed has already framed the session at startup.

**g. Drift surfaces as session-time review via hook stdout.** When `ways init` detects drift and writes a diff file, it emits a review prompt to stdout. The `SessionStart` hook pipeline captures that stdout into the session's initial context, giving Claude an actionable triage instruction: apply the routing table (see point `f`) to each diff entry — convert repo-relevant content to ways/ADRs/design notes/issues, discard what doesn't warrant preservation, re-add only genuine cross-project user facts under `## User Context`. Compaction output is no longer a silent silo — it's a conversion queue the session actively triages. No separate state-trigger way is needed; the init hook's own stdout is the surfacing mechanism.

Claude remains free to write memory — no permission changes, no harness modification. The intervention is purely framing: the memory attractor gets redirected toward the repo-portable artifacts the project already maintains, and any output that accumulates despite the framing gets actively reviewed and re-routed at the next session.

## Consequences

### Positive

- **Repo-portability** — project knowledge captured as ways travels with `git clone`. CI sees it, teammates see it, other Claude runs see it. The per-instance cage is broken for everything that belongs in a repo artifact.
- **Preserved discipline** — ADR/way/issue/PR/commit friction is protected from memory's shortcut. Elaborate memory entries get reframed as "discipline bypassed" *before* they're written, not after.
- **Staleness detection** — ways and ADRs are lint-validated in CI; memory isn't. Routing project facts through ways means drift gets caught mechanically rather than on accidental human re-read.
- **Idempotent, structurally versioned seeding** — the hook is integrity-verified (byte-equality against the binary's embedded canonical) and carries `seed-version` in frontmatter. Ships at v1; template improvements are designed to ship as version bumps with diff preservation on migration (path present but unexercised until a v2 lands).
- **Auditable drift** — when a seed is edited (intentionally by the user, or by Claude mid-session), the unified diff is preserved with a serial-numbered filename. Review is a concrete artifact, not a memory lookup.
- **Compaction becomes a re-routing trigger** — the periodic memory-compaction cycle rewrites `MEMORY.md` between sessions; our byte-equality check detects it, the hook re-seeds + writes a diff, and emits a review prompt to stdout which SessionStart injects into Claude's context at the next session. Compaction output gets *converted* into repo artifacts (ways, ADRs, issues) or explicitly discarded — not silently re-consolidated back into memory. The harness's memory-tidying work becomes an input to our discipline, not a competitor.

### Negative

- **New-project onboarding cost** — every new project needs the hook installed. `project-init` should handle this automatically; existing projects need a one-time install step.
- **Non-preserving for seeded-portion edits** — if a user or Claude edits the seeded body (above `## User Context`), the next session's hook detects drift, diffs it, and rewrites. User edits to the seed itself are treated as drift, not customization. Intentional: opinionated framing is the whole point.
- **Harness coupling** — mechanism depends on the harness loading `MEMORY.md` directly at session start. Anthropic's in-flight memory-compaction work (see Context) is visibly ongoing; a future release replacing direct load with summarization or compacted injection would change the delivery path. The *framing* (project knowledge belongs in repo artifacts, not per-instance memory) survives any such change; only the force-feed mechanism is coupled.
- **Review cost per compaction cycle** — when compaction runs, Claude is prompted at next session-start to triage the diff. If the user just wants a quick task, this pulls attention into memory review first. Mitigation: users who want no compaction at all can toggle it off via `/memory`, and the diff file can simply be ignored until the user is ready to triage (the hook does not block the session or gate future work on review).
- **Gated on project scaffolding** — seeding is piggybacked on `ways init`, which early-returns when the CWD has neither `.claude/` nor `.git/`. Scratch-directory sessions (running Claude Code in an arbitrary folder with no project shape) skip seeding and get the harness's default memory behavior. This inherits `ways init`'s existing gating and is correct for its scope; the consequence is that the routing guidance reaches only project-shaped sessions.

### Neutral

- **Memory remains writable** — no permissions change. The seed steers, it doesn't prohibit. Claude can still save memory when genuinely warranted for cross-project user facts.
- **Scope is harness-specific** — the hook exists for this harness shape. The underlying principle generalizes; the specific hook does not.

## Alternatives Considered

- **Do nothing; accept the harness's memory defaults.** Rejected — the three failure modes in Context are observed and recurring. Inaction means continued drift and continued short-circuiting.
- **Disable or suppress memory writes entirely.** Rejected. Fights the harness rather than collaborating; breaks on harness updates; removes legitimate cross-project user memory use cases; hard to enforce without permission hacks.
- **Put routing guidance only in `hooks/ways/meta/memory/memory.md` (no seed).** Rejected as sole mechanism. The way fires on triggers; the seed loads unconditionally. To redirect the memory-writing instinct, the guidance must reach the model *before* it forms an intent to save — that requires the force-fed slot. Both channels (seed at startup, way on trigger) compose for defense-in-depth.
- **Hash the whole file including `## User Context`.** Rejected — any legitimate user addition would trigger re-seed. The narrower hash (seeded portion only) distinguishes template integrity from user content.
- **Feature-flag the seeding behind an opt-in.** Rejected. The framing should be the default, not opt-in. Also: a flag is exactly the kind of unrequested preservation scaffold the project has explicitly deprecated.
- **Put project knowledge in `~/.claude/projects/<hash>/memory/` as topic files (current practice).** Rejected — this is the per-instance cage this ADR exists to leave. Topic files in that directory don't travel with the repo, aren't lint-validated, aren't reviewable, and compete with ways for the project's accumulated learning.
- **Rely on Anthropic's periodic memory-compaction to consolidate instead.** Rejected. Compaction consolidates memory *into* memory — it tidies the silo without addressing the two structural problems this ADR targets (per-instance cage, short-circuited discipline). A tidier silo is still a silo.
