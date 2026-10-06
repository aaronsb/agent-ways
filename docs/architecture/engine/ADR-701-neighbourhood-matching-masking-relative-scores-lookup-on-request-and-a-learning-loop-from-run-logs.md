---
contract: adr/v1
kind: decision
verb: change
capability: [matching, authoring, disclosure]
basis:
  - operator: aaronsb
    level: guided
    said: "let's explore if there's merit in embedding more than just the content of the frontmatter from ways for search"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "or, perhaps, two different embedding passes - one is the frontmatter corpus and the other is the content corpus, chunked by perhaps sentences (not sure of chunking strategy)"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "if this seems to have merit, this would be a great time to pull the other github issues in that we have open related to moving the embedding server to a daemon/fork (like the judge process has)"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "the other aspect of that tradeoff is if we can track what was embedded at the time, and we know that our deactivated state is less the subset of what was embedded, it's cheap to just discard results that match the path of disabled items (since we know they were embedded, we can basically claim that the best match was x, but was disabled, so the next best match possibly could be y, which is not disabled - as long as y met the min threshhold AND the judge later allowed it )"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "the failure modes for the system would be (from most capable to least capable): no failure: all proposed future state items are available: 0: judge llm api, keyword corpus, body corpus. 1: judge llm api unavailable or timing out, keyword corpus and body corpus available. 2: keyword corpus available, body corpus unavailable, judge api unavailable. 3. keyword corpus unavailable, corpus unavailable, judge api unavailable (probably, daemon process not running) - regex match only"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "one of my goals is to give the judge a match score during presentation of evaluation criteria, and low indifferent match scores aren't helpful"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: directed
    said: "ways would have it's own engine unrelated to the vault mcp server, in my opinion. this means that the core goals of ways is to leverage a ways 'graph' to offer an intentiionally designed neighborhood during matching, (the knowledge graph) that is queried and judged as needed. This means that the ways mcp server could eventually expand in capability to offer some ways lookup (directed active  lookup rather than autonomous context injection as it does now), and if we have the jsonl evidence of previous runs (matched scores perhaps, on turns, plus relevancy judge decisions tracked) we have an excellt real world corpus to update the ways graph (terms, content, scoring etc)"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "I am thinking forward to implementation, and I don't think this is a breaking change to use of agent-ways once it eventually is built and published"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: directed
    said: "in the mcp, lookup on request, if requested within the redsclosure supress envelope if it was a normal way injection shouldn't refuse to present the way. it would be helpful to log it as an out of band read though"
    via: chat, session 796c50d0, 2026-10-05
  - evidence: ADR-700
  - precedent: ADR-125
  - precedent: ADR-502
agent:
  name: claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-05
deciders:
  - aaronsb
related:
  - ADR-105
  - ADR-110
  - ADR-126
  - ADR-127
  - ADR-131
  - ADR-143
  - ADR-302
  - ADR-310
  - ADR-156
  - ADR-160
  - ADR-188
  - ADR-196
  - ADR-199
  - ADR-501
  - ADR-502
  - ADR-700
  - ADR-702
---

# ADR-701: Neighbourhood matching: masking, relative scores, lookup on request and a learning loop from run logs

## Summary

- **Decided:** the ways engine moves toward matching within an intentionally designed neighbourhood, queried and judged as needed, with run logs as its tuning corpus. This record decides the parts the measurements in ADR-700 support, on today's identity (path ids), today's parent relation (directories) and today's reference edges (See Also). Disabled ways are masked at scan time before any competition. Every scan logs its top candidates with share and margin, and the event log rotates. Lint checks that See Also targets resolve, refuses symlinks in ways roots and keeps basenames unique. The judge can receive share, margin and a band label, adopted after an A/B evaluation. `ways-mcp` gains lookup on request. A body corpus stored as a binary sidecar serves body confirmation and, if evaluation confirms it, a fused ranking score. The run logs produce reviewed proposals; nothing refits on its own. Stable identity, extra parents and frontmatter edges are proposed separately in ADR-702.
- **Trades away:** more logging per scan, a lint pass that can flag existing user and project ways, a second corpus file once the body sidecar ships, and a judge input that must earn its place in an A/B.
- **One-way?** No. Every element is additive. With the new flags off and the sidecar absent, behaviour matches today apart from masking.
- **Probes:** *Confident (non-breaking):* existing way files, configurations and installs keep working unchanged, and nothing new is required of a way author outside the core corpus. *Not confident (judge-scores):* the judge should see share and margin, not raw cosine, and only if an A/B shows it judges better with them.
- **Inversion:** between scoring a flat list of ways independently and holding the graph in a database the engine owns. The decision keeps files as the only source and builds every vector, parameter and log-derived proposal from them.

## Context

The matcher scores each prompt against one alias vector per way (`description + vocabulary`) and thresholds a calibrated cosine (ADR-156), with late interaction and body confirmation on multi-sentence surfaces (ADR-160) and a relevance judge on the result (ADR-196). ADR-127 rejected body text in place of the alias and named graph-aware routing as the forward path. ADR-125 named the corpus an authored disclosure graph.

ADR-700 measures the current matcher on a 332-row golden set. Body content in place of the alias loses 19 points of top-1; a light fusion beside the alias gains about 2.5, short of significance. Raw cosine is weak in its middle range, while margin and softmax share separate right from wrong cleanly. Routing loses 6 to 7 points of top-1 per doubling of the corpus. Disabled ways still compete in the late-interaction pass. The event log records fires, near-misses and judge verdicts, but not the ranked candidates.

agent-ways is moving toward knowledge work beyond software, which means more domains and a larger corpus. A review of an earlier draft of this record found its identity and multi-parent machinery speculative at today's corpus size and move rate, and found two defects: masks computed at build, though project toggles are only known at scan time, and per-way toggles in user scope, which ADR-131 rejected. That machinery moved to ADR-702.

## Decision

### 1. Masking and toggles

- Disabled ways are removed before any scoring competition, single-vector or late-interaction. In late interaction, rows for ways outside the enabled set are dropped before softmax share is computed and before survivors are chosen for confirmation.
- The enabled set is computed at scan time from `disabled_domains` (user scope) and per-way toggles (project scope, ADR-131).
- The project `ways:` map also accepts a path prefix, such as `softwaredev/code/supplychain/*: false`, which disables every way under it. A toggle on a way itself overrides a prefix.
- Both corpora embed every way, disabled or not, so enabling one again needs no rebuild.

### 2. Logging

- Every scan logs its top 5 candidates with cosine, share and margin, and whether the body sidecar was used.
- The event log rotates by age, keeping enough history for session introspection (ADR-153) and tuning. Retention is configurable.

### 3. Lint

- See Also targets resolve to a way, looked up across every root a session reads (project, user, core), so a project or user way may point at a core way. Linting a subtree or a single file resolves against the ways root that contains it.
- No symlinks inside a ways root: the scanner would read a linked way as a second way.
- Basenames are unique within a root (ADR-110 §7); the two colliding basenames are renamed.
- These checks are errors for the core corpus and warnings for user and project ways, so an upgrade alone turns no one's lint red.

### 4. Scores for ranking and the judge

- Whether a way fires stays decided by calibrated cosine (ADR-156), with one calibration until a fused score ships.
- Share is a softmax at τ 0.08 over the top 8 enabled candidates of the scan's lane, and margin is the gap to the next enabled candidate.
- Behind a flag, the judge receives for each candidate its share, margin, a band label (strong, uncertain, weak, with fixed bounds from ADR-700 §4) and the route already in its input. The flag turns on by default only after an A/B on the golden set and on logged verdicts shows the judge rejects more irrelevant fires without losing relevant ones.

### 5. Lookup on request

`ways-mcp` (ADR-501) gains, running in-process until the daemon's search service exists:

- `ways_search(query)`: candidates with route, share, margin and matched section
- `ways_read(id)`
- `ways_neighbors(id)`: the parent, children, See Also edges and nearest semantic neighbours from `ways author siblings`, each labelled with its kind

A pull always returns the way, including inside its re-disclosure suppression window (ADR-126), where injection would hold it back. A pull needs no judge, because the agent chose it. A pulled way stamps disclosure, so injection does not repeat it on the next turn. Each pull is logged as `way_pulled`. A pull inside the suppression window is logged as an out-of-band read, with `out_of_band: true` and the epoch distance since the last disclosure.

### 6. Body corpus

- Each way's prose split at headings, with frontmatter, code blocks, tables, HTML comments and the See Also section removed, and sections longer than 120 words split. About 4.7 vectors per way.
- Stored as a binary sidecar beside the alias corpus, with a manifest recording each way's content hash, so the in-process path can load it. At today's size it is about 1 MB.
- Body confirmation reads it in place of embedding the first 8 sentences on every prompt, once a measurement shows confirmation is no worse with it.
- **Body score in ranking** (`alias + 0.25 × best section`) is adopted only after an evaluation on held-out real prompts shows a gain with no loss in separating relevant prompts from unrelated ones. Once adopted, confirmation uses a section other than the one that contributed to the ranking score, and a way with one section or none is confirmed against its alias.

### 7. Degradation

| State | Matching | Body confirmation | Precision filter |
|---|---|---|---|
| Embedder, sidecar complete, judge reachable | keyword lane and alias matcher, with the fused score once adopted | sidecar | judge |
| Judge unreachable or past its deadline | as above | sidecar | matcher gates |
| Sidecar missing or incomplete | keyword lane and alias matcher | embedded per call | judge if reachable |
| No embedder | keyword lane only | none | none |

The sidecar is used only when its manifest covers every enabled way at the alias corpus's content hashes; otherwise the scan uses alias scores alone. Fused and alias-only scores are never mixed in one ranking. Each scan logs which state it ran in, and `ways status` reports the current state and the reason for any drop. The daemon's search service (ADR-502 §2, #668) holds the same files when it lands; until then the hook loads them.

### 8. Run logs as the learning corpus

- Pulled and injected disclosures are told apart. A pull of a way injection did not fire on that turn is a recall miss. An out-of-band read is evidence that the way's `refire:` fraction is longer than the work needs.
- Logs produce proposals that land as pull requests: vocabulary and body changes from repeated near-misses, boundary or edge changes from repeated confusions and from semantic neighbours with no See Also between them, `refire:` changes from out-of-band reads, and changes to the calibration probe set.
- Nothing refits on its own. Calibration stays fitted from the reviewed probe set at corpus build, so the fire decision does not drift per install.
- Prompt text stays out of the event log. When the operator opts in, a local consumer of the ways sensor (ADR-199) carries it to an evaluation store on the machine.
- Judge verdicts are weak labels. Fitting and evaluation hold out by session or date.

### 9. Evaluation travels with the corpus

Each new core way ships with two golden prompts, one direct and one situational, and lint checks that they exist. The harness in `experiments/content-corpus/` becomes the standing evaluation, run against the golden set and, when available, the real-prompt store.

### 10. Compatibility

No field becomes required. Configuration keys keep their meaning; the prefix form in `ways:` is new and optional. The sidecar is a new file that a binary without support ignores. With the judge flag off and no sidecar, an upgraded install behaves as today apart from masking, which can let an enabled way fire where a disabled one crowded it out.

### Increments

1. Masking at scan time and prefix toggles; top-5 logging with share and margin; event-log rotation; the three lint checks and the basename renames.
2. Share, margin and band to the judge behind a flag; the A/B that decides the flag's default.
3. `ways-mcp` lookup in-process, `way_pulled` and out-of-band reads.
4. Body sidecar build; body confirmation reads it after measurement.
5. Proposals from logs through `ways tune` and `ways suggest`; the opt-in real-prompt store through the ways sensor.
6. Gated on evaluation: body score in ranking; a hubness penalty fitted at build against the probe set; competition within a subtree first.
7. When #668 lands: the daemon's search service holds the corpora and the sidecar.

## Consequences

### Positive

- Disabling unrelated domains narrows the competition, which by ADR-700 §6 raises accuracy and margins.
- The judge can see scores that separate right from wrong, once an A/B shows they help.
- Body confirmation stops embedding on every prompt and covers the whole body.
- Lookup on request gives the agent a path to ways injection missed, and records each miss as a label.
- Tuning moves from hand estimates to proposals drawn from logged use, still reviewed.

### Negative

- Per-scan logging grows by the candidate list, held in check by rotation.
- New lint warnings can appear on existing user and project ways.
- The sidecar must stay complete or scans fall back to alias scores.

### Neutral

- Files remain the only source of truth.
- ADR-702, if accepted, changes identity and edges without changing this record's mechanisms.
- ADR-188's move of tool-lane matching to PostToolUse leaves prompt and task surfaces as the main users of the body corpus.

## Alternatives Considered

- **Body text in place of the alias.** Rejected by ADR-127 and ADR-700 §1.
- **Maximum or reciprocal-rank fusion of alias and body.** Rejected by ADR-700 §1: both lose to the alias alone.
- **Dot product or Euclidean distance.** Rejected: with unit vectors both rank identically to cosine (ADR-700 §5).
- **Centred scores for the fire decision.** Rejected: centring weakens the separation from unrelated prompts (ADR-700 §5).
- **Raw cosine to the judge.** Rejected: it is right about half the time in its middle range (ADR-700 §4).
- **Excluding disabled ways at build time.** Rejected: enabling one again would need a rebuild, and masking costs little.
- **Masks computed at corpus build.** Rejected: project toggles are only known at scan time.
- **Automatic refit of calibration or bands from logs.** Rejected: the fire decision would drift silently and differ between installs, and the logs are biased toward what the matcher already surfaced.
- **The body corpus only in the daemon.** Rejected: as a binary sidecar it is smaller than the alias JSON the hook already parses.
- **Hold the graph in a database.** Rejected: the corpus is small, authored and reviewed, and writes arrive through pull requests. ADR-702 records how its integrity is enforced on files.
- **UUID identity, `parents:` and `related:` in this record.** Moved to ADR-702: nothing here depends on them, and at the measured move rate (12 renames since April, in four commits) they are not needed yet.
