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
    level: guided
    said: "the nesting of ways offers an acyclic graph relationship. additionally, relative symlinks could make it cyclic, if it was warranted. we could essentially have a graph of 'progressive disclosure' content that looks just like files in directories, but treat it as a knowledge graph"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "using a filesystem seems like an ideal tradeoff"
    via: chat, session 796c50d0, 2026-10-05
  - operator: aaronsb
    level: guided
    said: "the other option is an obsidian approach where the frontmatter has links. this would be where orienting them on disk doesn't really matter except for ease of authoring, but the actual graph is represented in computing all the frontmatter"
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
amends: [ADR-125#1]
related:
  - ADR-105
  - ADR-110
  - ADR-126
  - ADR-127
  - ADR-156
  - ADR-160
  - ADR-188
  - ADR-196
  - ADR-199
  - ADR-501
  - ADR-502
  - ADR-700
---

# ADR-701: Ways as an authored graph: neighbourhood matching, lookup on request, and a learning loop from run logs

## Summary

- **Decided:** the ways engine treats the corpus as an authored graph held in files. Directory nesting stays the primary parent edge, an optional `parents:` frontmatter field adds further parents, and See Also stays the reference edge in the body. A precomputed, section-chunked body corpus sits beside the alias corpus. Disabled ways are masked before any competition. Calibrated cosine keeps deciding whether a way fires, while ranking and the judge see relative scores: share, margin and a hubness-corrected score. A four-level degradation contract fixes what runs when parts are missing. The daemon's search service holds both corpora. `ways-mcp` gains lookup on request. Every scan logs its ranked candidates, so run logs become the evaluation set and the source of tuning: derived parameters refit automatically, and authored changes arrive as pull requests. Fusing the body score into ranking waits on an evaluation with real prompts.
- **Trades away:** a second corpus to build and keep complete (about 4.7 vectors per way), more state in the daemon, one more frontmatter field, and more logging per scan. A small routing gain from body fusion stays unused until real prompts confirm it.
- **One-way?** No. Every element is additive and optional. A way without `parents:` behaves as today, and with the daemon absent the hook runs today's matcher.
- **Probes:** *Confident (non-breaking):* existing way files, configurations and installs keep working unchanged, and nothing new is required of a way author. *Not confident (multi-parent-disable):* a way that sits under both an enabled and a disabled domain stays enabled.
- **Inversion:** between scoring a flat list of ways independently and holding the graph in a database the engine owns. The decision keeps files as the only source of truth and builds every graph structure, vector and parameter as a derived index that can be rebuilt.

## Context

The matcher scores each prompt against one vector per way and thresholds a calibrated cosine (ADR-156), with late interaction and body confirmation on multi-sentence surfaces (ADR-160) and a relevance judge on the result (ADR-196). ADR-127 rejected body text in place of the alias and named graph-aware routing as the forward path. ADR-125 named the corpus an authored disclosure graph. Neither built routing that uses the graph.

ADR-700 measures the current matcher on a 332-row golden set. Body content in place of the alias loses 19 points of top-1. A light fusion beside the alias gains about 2.5 points, short of significance. Raw cosine is a weak signal in its middle range, while margin and softmax share separate right from wrong cleanly. Routing loses 6 to 7 points of top-1 per doubling of the corpus, and margins shrink with it. Ways outside software development route 16 points worse. Disabled ways still compete in the late-interaction pass. The event log records fires, near-misses and judge verdicts, but not the ranked candidate list.

agent-ways is moving toward knowledge work beyond software, which means more domains and a larger corpus. Systems that carry knowledge and its relationships often keep them in a database. This corpus is small, authored by people and changed through review, so files under git keep history, review and recovery with no service to run.

## Decision

### 1. The graph is authored in files

- A node is a way file. Its identity stays its path id.
- **Parent edges.** The directory is the default parent. An optional `parents:` frontmatter field lists further parents by way id as a single-line list. The parent boost (ADR-105) reads it, which keeps frontmatter to fields the matcher reads (ADR-110 §3). This section amends ADR-125 §1 by adding authored extra parents to its edge set.
- **Reference edges** stay the body See Also section (ADR-110 §2). They are visible to the model and need no frontmatter.
- **Node text.** A directory may carry a short description of the class of ways under it (#664). The route from the domain root to a way, each node described, is part of how a way is presented.
- **Parent edges stay acyclic,** so ancestors stay defined for the boost and for disabling. Reference edges may form cycles. Lint refuses a parent cycle.
- **Symlinks are not edges.** Lint refuses a symlink inside a ways root, because the scanner would read a linked way as a second node and symlinks are not portable across platforms and git settings. `parents:` covers the same need.
- **Disabling with several parents.** A way is enabled when any of its parent paths is enabled.
- **Basenames stay unique** across a ways root, as ADR-110 §7 requires, and lint enforces it.
- The graph export, ancestor sets and masks are derived at corpus build. None is authoritative.

### 2. Two corpora

- The **alias corpus** (`description + vocabulary`) is unchanged and stays the primary signal.
- A **body corpus** holds each way's prose split at headings, with frontmatter, code blocks, tables, HTML comments and the See Also section removed, and sections longer than 120 words split. It is built with the alias corpus.
- Both corpora embed every way, disabled or not. The manifest records which ways were embedded and the content hash of each, so enabling a way again needs no rebuild.
- Body confirmation reads the precomputed body vectors and covers the whole body, replacing the per-call embedding of the first 8 sentences.
- **Body score in ranking** (`alias + 0.25 × best section`) is adopted only after an evaluation on held-out real prompts shows a gain with no loss in separating relevant prompts from unrelated ones. Until then the body corpus serves confirmation only. Once the body score joins ranking, confirmation must use a section other than the one that contributed to the ranking score.

### 3. Competition

- Disabled ways are removed before any scoring competition, single-vector or late-interaction, so a disabled best match cannot suppress the next enabled way.
- The graph defines the neighbourhood a prompt competes in. Restricting competition to a subtree chosen first is the intended next step and is adopted when the evaluation harness shows a gain over the full-corpus competition.

### 4. Scores

- **Whether a way fires** is decided by calibrated cosine (ADR-156), fitted per degradation level.
- **Ranking and presentation** use relative scores: softmax share and the margin over the next candidate, computed on a hubness-corrected score. The hubness penalty for each way is fitted from logged prompts without labels and is a derived parameter.
- **The judge receives,** per candidate: the described route, share and margin, a band label (strong, uncertain, weak), and the body section that matched. The band bounds are fitted on real prompts.

### 5. Degradation contract

| Level | Available | Matching | Body confirmation | Precision filter |
|---|---|---|---|---|
| 0 | daemon, judge, alias corpus, body corpus | keyword lane and the full scoring above | precomputed vectors | judge |
| 1 | as 0, judge unavailable or past its deadline | as 0 | precomputed vectors | matcher gates |
| 2 | alias corpus only: daemon absent, or body corpus missing or incomplete | keyword lane and today's alias matcher | embedded per call | judge when the daemon runs, else matcher gates |
| 3 | no embedder | keyword lane only | none | none |

- The body corpus is used only when its manifest covers every enabled way at the alias corpus's content hashes. Otherwise the scan runs at level 2. Fused and alias-only scores are never mixed in one ranking.
- Each level uses the calibration fitted for its own score.
- Every scan logs its level, and `ways status` reports the current level and the reason for any drop.

### 6. Where it runs

The daemon's search service (ADR-502 §2, #668) holds the alias corpus, the body corpus, the masks, the hubness penalties and the calibrations, and reloads them when the corpus manifest changes. The hook's own path is level 2.

### 7. Lookup on request

`ways-mcp` (ADR-501) gains tools served by the daemon:

- `ways_search(query)`: candidates with route, share, margin and matched section
- `ways_read(id)`
- `ways_neighbors(id, edge types)`

A pull always returns the way, including inside its re-disclosure suppression window (ADR-126), where injection would hold it back. A pull needs no judge, because the agent chose it. A pulled way stamps disclosure, so injection does not repeat it on the next turn. Each pull is logged as `way_pulled`. A pull inside the suppression window is logged as an out-of-band read, with `out_of_band: true` and the epoch distance since the last disclosure.

### 8. Run logs as the learning corpus

- Every scan logs its top 5 candidates with alias score, body score, share, margin and level.
- Pulled and injected disclosures are told apart. A pull of a way injection did not fire on that turn is a recall miss. An out-of-band read is evidence that the way's `refire:` fraction is longer than the work needs.
- Prompt text stays out of the event log. When the operator opts in, a local consumer of the ways sensor (ADR-199) carries it to an evaluation store on the machine.
- **Derived parameters refit automatically:** calibration, hubness penalties and band bounds.
- **Authored content changes arrive as pull requests:** vocabulary, body text, `parents:` and See Also edges, drafted by `ways tune` and `ways suggest` from repeated near-misses and confusions.
- Judge verdicts are weak labels. Fitting and evaluation hold out by session or date.

### 9. Evaluation travels with the corpus

Each new way ships with two golden prompts, one direct and one situational. Lint checks that they exist. The harness in `experiments/content-corpus/` becomes the standing evaluation, run against the golden set and, when available, the real-prompt set.

### 10. Compatibility

No field becomes required. `parents:` and directory descriptions are optional. Configuration keys keep their meaning. The body corpus is a new file beside the existing ones, and a binary that does not know it ignores it. Installs without the daemon run at level 2, which is today's behaviour.

### Increments

1. Mask disabled ways before competition; log the top 5 candidates and the level on every scan.
2. Lint: unique basenames, no symlinks in ways roots; rename the two colliding basenames.
3. `parents:` in the schema, the graph build, the parent boost and the disable rule.
4. Build the body corpus and its manifest; body confirmation reads it.
5. Daemon search service holding both corpora (#668), with level reporting.
6. Hubness penalty, share and margin; judge presentation with route, band and matched section.
7. `ways-mcp` lookup tools and `way_pulled`.
8. Real-prompt evaluation store through the ways sensor; refit loop for derived parameters; proposals for authored changes.
9. Gated on evaluation: body score in ranking; subtree-first competition.

## Consequences

### Positive

- Disabling unrelated domains narrows the competition, which by ADR-700 §6 raises accuracy and margins.
- The judge sees scores that separate right from wrong, with the route and matched section as evidence.
- Body confirmation stops embedding on every prompt and covers the whole body.
- Lookup on request gives the agent a path to ways that injection missed, and records each miss as a label.
- Tuning moves from hand estimates to parameters fitted on logged use, with authored changes still reviewed.

### Negative

- The corpus build does more work, and the body corpus must stay complete or the scan drops a level.
- The daemon holds more state and needs a reload path.
- Per-scan logging grows by the candidate list.
- Two scores coexist, one deciding fires and one ranking and presenting, and each needs its own calibration.

### Neutral

- Files remain the only source of truth. The graph export, masks, vectors and parameters are all rebuildable.
- Real-prompt evaluation depends on the operator opting in to the local sensor consumer.
- ADR-188's move of tool-lane matching to PostToolUse leaves prompt and task surfaces as the main users of the body corpus and neighbourhood competition.

## Alternatives Considered

- **Hold the graph in a database.** Rejected: the corpus is small, authored and reviewed, and a database adds a service, migrations and a second source of truth.
- **A graph defined only by frontmatter links, independent of directories.** Rejected for now: way ids, project overrides, domain disabling, the parent boost and telemetry all key on the path, and moving them is a migration across the CLI, telemetry and configuration. `parents:` gives multiple parents without it.
- **Relative symlinks as edges.** Rejected: the scanner reads a linked way as a second node, and symlinks break under some platforms and git settings.
- **Body text in place of the alias.** Rejected by ADR-127 and ADR-700 §1.
- **Maximum or reciprocal-rank fusion of alias and body.** Rejected by ADR-700 §1: both lose to the alias alone.
- **Dot product or Euclidean distance.** Rejected: with unit vectors both rank identically to cosine (ADR-700 §5).
- **Centred scores for the fire decision.** Rejected: centring weakens the separation from unrelated prompts (ADR-700 §5).
- **Excluding disabled ways at build time.** Rejected: enabling one again would need a rebuild, and masking costs little.
- **Building the body corpus for the in-process path.** Rejected: re-parsing several megabytes of vectors on every hook call costs more than it gains; the in-process path stays at level 2.
