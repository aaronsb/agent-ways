---
status: Accepted
date: 2026-05-21
deciders:
  - aaronsb
  - claude
related:
  - ADR-105
  - ADR-108
  - ADR-125
  - ADR-127
---

# ADR-130: Sentence-salience input reduction for embed matching

## Context

Way matching today passes hook payloads — user prompts, bash command lines,
subagent dispatch prompts, persisted-output and task-notification blobs
delivered through `UserPromptSubmit` — directly into `way-embed` as the
`--query` argument. The MiniLM embedding models (English `all-MiniLM-L6-v2`,
multilingual `paraphrase-multilingual-MiniLM-L12-v2`) are trained on 128
tokens of position embeddings; inputs past that abort the embedder inside
`ggml_compute_forward_get_rows`.

Three things made this latent bug visible recently:

1. **Auto mode** raised the *rate* of tool dispatches. Each dispatch fires a
   hook; each hook spawns `way-embed` twice (EN + multilingual). With three
   to four concurrent sessions, the per-minute spawn count crosses what the
   single-shot binary architecture was designed for.

2. **The "goals" feature** raised the *size* of dispatches. Top-down goal
   context gets re-packed into structured agent prompts — file paths,
   conventions, smoke tests, return-shape requirements — so a `vue-expert`
   delegation today is routinely 3–5 KB of well-formed prose. The original
   ways scan path was sized for one-sentence intents.

3. **Custom subagents** (`.claude/agents/*.md` files in project, user, and
   plugin locations) became common. These are exactly the dispatches that
   carry the longest prompts.

`way-embed` failed open across all three vectors: the Rust caller
(`run_embed_match` in `tools/ways-cli/src/cmd/scan/scoring.rs`) catches
non-zero exit codes and returns `None`, so the scan appears successful and
emits empty match scores. The underlying SIGABRTs accumulated silently —
600+ crashes between 2026-05-06 and 2026-05-21, peaking at 136/day — and
only surfaced when KDE's `drkonqi` started showing crash-reporter popups.

### What's been shipped as immediate mitigation

PRs #94, #95, #96 closed the three crash paths with the cheapest possible
fixes:

- **#94** — discriminate `subagent_type` against known custom-agent `.md`
  files; skip `ways scan task` for custom agents (their `.md` IS their
  constitution, so ways injection is redundant)
- **#95** — truncate the bash command field at 256 chars before
  `ways scan command` (the longest `commands:` regex in the corpus is 106
  chars, so the cap changes no existing matching behavior)
- **#96** — truncate the combined prompt+response-topics at 1024 chars
  before `ways scan prompt`

These are correct as **safety nets** — they keep the embedder inside its
position-embedding window — but they're lossy. Past the cap, bash heredoc
bodies, persisted-output blobs, and the prose of agent dispatches are
discarded outright. For agent dispatch in particular, that prose IS the
intent signal the matcher exists to read.

### What the alternatives forbid

ADR-125 ("Authored Disclosure Graph and Removal of BM25") established
embedding-only retrieval as the architecture. The multilingual model is
treated as a black box; lexical tiers are explicitly out of scope. *"Stops
the temptation to add lexical patches when the embedding behavior surprises
us."*

ADR-127 ("Full-body embedding corpus for way matching") tested making the
embedded representation denser by ingesting full way bodies instead of
description + vocabulary; **rejected**, because text-matching density is
not the axis of improvement. The deeper reading: routing quality lives in
the graph and the authored aliases, not in throwing more text into the
embedder.

Both ADRs constrain the design space. We cannot:

- Reintroduce BM25 or any other lexical-scoring tier as a parallel matcher
  (ADR-125)
- Make the embedded representation richer in hopes of better discrimination
  (ADR-127)
- Add an LLM-based intent-extraction step (defeats the perf goal — the
  whole problem is that we're embedding too often, not that we lack a
  smarter analyzer)

So the design space reduces to: **how do we shrink an arbitrary-sized hook
input into something the embedder can consume, without discarding the
prose that carries the intent signal, without introducing a lexical
matching tier, and at a cost cheaper than the embed step it precedes?**

## Decision

Add a **sentence-salience reduction step** in front of the embed call. When
a hook input exceeds the embedder's working window, score its constituent
sentences by frequency-distilled salience, keep the top-N highest-salience
sentences (concatenated in document order), and embed only those. When the
input already fits, pass it through unchanged.

### Algorithm

```
fn reduce_for_embed(input: &str, budget_tokens: usize) -> String {
    let token_count = approx_tokens(input);
    if token_count <= budget_tokens {
        return input.to_string();
    }

    let sentences = split_sentences(input);
    let token_counts = count_tokens(&input);  // bag of words over whole input
    let weights = softmax(token_counts);      // distribution with spread

    let scored: Vec<(usize, &str, f64)> = sentences
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let tokens = tokenize(s);
            let salience = tokens.iter().map(|t| weights[t]).sum::<f64>()
                          / (tokens.len() as f64).max(1.0);
            (i, s, salience)
        })
        .collect();

    let mut selected: Vec<&(usize, &str, f64)> = scored.iter()
        .collect();
    selected.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    let mut accumulated = 0;
    let mut keep: Vec<&(usize, &str, f64)> = Vec::new();
    for s in selected {
        let s_tokens = approx_tokens(s.1);
        if accumulated + s_tokens > budget_tokens { break; }
        keep.push(s);
        accumulated += s_tokens;
    }

    // Re-order kept sentences in their original document order
    keep.sort_by_key(|s| s.0);
    keep.iter().map(|s| s.1).collect::<Vec<_>>().join(" ")
}
```

The shape: sentences are the unit of selection (preserves phrasing within
each sentence); each sentence's salience is the average softmax-weight of
its tokens against the whole input (so sentences carrying terms the input
emphasizes score higher than sentences full of one-off mentions);
document order is preserved so the embedder sees coherent prose.

### Why these specific choices

- **Sentence unit, not token unit.** Bag-of-words over tokens (Shape A in
  the design conversation) is cheaper but throws away phrasing —
  "rollback the deployment" and "deployment the rollback" become
  identical. Sentence unit preserves the local word order that the
  embedder is trained to use. If empirical testing shows sentence-level
  salience adds no recall over token-level, we shave down to Shape A
  later; the path from B → A is mechanical.

- **Softmax over frequencies, not raw counts.** Counts emphasize the
  most-repeated tokens, which in long structured prompts are often
  scaffold words ("the", "agent", "context"). Softmax compresses the
  high tail and lifts the middle, which is where the discriminative
  vocabulary lives. The user's intuition here matches the IR convention:
  softmax-normalized weights give better distributive spread than raw TF
  or TF-IDF when the corpus comparison is intentionally absent.

- **No stemming.** The MiniLM tokenizer is WordPiece; it already
  segments `agent`/`agents`/`agentic` into shared subword units before
  the position embeddings ever look at them. An English-stemmer pass on
  top is redundant for prose and harmful for code (where `Agent` the
  type is distinct from `agent` the noun). Stopword filtering at the
  salience-scoring stage is fine; the stopword list is small and
  language-aware via the input itself, not via a per-locale dictionary.

- **No corpus involvement.** The reducer never reads the ways corpus.
  This is deliberate: it preserves ADR-125's separation between input
  preparation and corpus-side matching, and it means the reducer's
  output is the same regardless of which subset of ways is enabled.

### Where this lives

Inside `tools/ways-cli/src/cmd/scan/`, as a function called from
`scan::command`, `scan::task`, `scan::prompt`, and `scan::file` immediately
before `batch_embed_score(query)`. The hook scripts continue to pass the
full payload via the existing CLI flags — the reduction happens once, in
Rust, after both EN and multilingual `--query` arguments have been
identified.

The `budget_tokens` constant is calibrated to leave headroom under the
MiniLM 128-position-embedding window:

| Hook | Budget (tokens) | Reasoning |
|---|---|---|
| `scan command` | ~60 | Command shape is short; preserves room for description |
| `scan task` | ~100 | Agent dispatch prompts carry the most intent; max budget |
| `scan prompt` | ~100 | User prompts can be discursive; needs more headroom than commands |
| `scan file` | ~30 | Filepath alone — rarely needs reduction |

Approximate tokenization (whitespace + punctuation split, ratio ~4 chars/token)
is sufficient for budgeting; precise tokenization is the embedder's job.

### What stays as-is

- The interim truncation caps in PRs #94–96 stay in the hook scripts as a
  belt-and-suspenders safety net, in case the reducer is bypassed,
  errors, or undercounts. Their character limits (1024 for prompt, 256 for
  bash) are well above what the reducer should ever emit, so they only
  fire on logic faults.
- Custom-agent skip (PR #94's discriminator) stays — those dispatches
  shouldn't reach the embed path at all, reducer or not.
- The embedding-only matching architecture from ADR-125 is unchanged.
  The corpus side, the alias model, and the per-node thresholds are
  untouched.

## Consequences

### Positive

- **Crash class eliminated structurally, not just clamped.** With the
  reducer in place, no hook payload can drive `way-embed` past its
  position-embedding window regardless of source. The truncation caps
  become dead code that never fires.
- **Agent-dispatch prose is preserved, not discarded.** The current
  1024-char prompt cap drops the back half of any 3+ KB delegation; the
  reducer keeps the high-salience sentences distributed across the whole
  document. That's the prose the matcher exists to read.
- **Cost stays well below the embed step.** Tokenize + frequency-count +
  sentence-split + softmax + sort on a 5 KB input runs in single-digit
  milliseconds in Rust. The embed step it precedes costs hundreds of
  milliseconds (model load + tokenization + cosine against the corpus
  rows). Reducer cost is in the noise.
- **Cheaper, not just safer.** Smaller `--query` means smaller embed
  input means slightly faster per-call embedding. The reducer pays for
  itself even when truncation wouldn't have been triggered.
- **Foundation for shaving down to Shape A later.** Empirical testing
  against real prompts will reveal whether sentence-level structure is
  doing real work. If not, simplification to token-level (Shape A) is a
  ~20-line diff inside the same function.

### Negative

- **Bag-of-words within sentences still loses phrasing nuance.** The
  reducer keeps whole sentences in document order, but it scores them
  using a token-frequency bag. A sentence with an unusual phrasing of a
  central concept scores the same as one with a common phrasing of the
  same concept. Mitigation: the embedder sees the full sentence text,
  so the eventual match still benefits from the unusual phrasing — only
  the *selection* step uses the bag.
- **Heavily-templated prompts may collapse onto scaffold content.** Agent
  dispatch prompts often include boilerplate like "Return: a summary of
  ..." or "Files you'll touch: ...". Repetition makes these tokens
  high-weight, which means their sentences may dominate the selection.
  This is the "lost in the middle" risk in miniature. Mitigation lives
  in the stopword list (which can grow to include scaffold tokens
  empirically observed to dominate) and in the budget — keeping enough
  sentences that scaffold dominance still leaves room for substance.
- **New code path to maintain.** Roughly 80 lines of Rust + tests. Small
  but real ongoing surface.

### Neutral

- **Match tuner's role is unchanged.** ADR-125's tuner still measures
  alias fidelity in embedding space. The reducer prepares the *input*
  before embedding; the tuner audits the *corpus* aliases. Different
  axes, no interaction.
- **Multilingual handling is implicit.** The reducer operates on whatever
  language the input is in. Sentence-splitting and tokenization both
  work reasonably across the languages the corpus supports
  (whitespace-segmented; languages without whitespace word boundaries
  like Japanese degrade to single-sentence behavior, which is the
  current state for `scan prompt` already).
- **The interim truncation caps (#94 / #95 / #96) become dead code.**
  Worth keeping for one cycle as redundancy; can be removed in a
  follow-up once the reducer is proven in practice.

## Alternatives Considered

### Shape A — Token-frequency bag-of-words

Same algorithm as the chosen direction, but selecting individual tokens
rather than sentences. Top-K tokens by softmax weight, concatenated as
the query. Cheaper (~30 lines), but throws away phrasing entirely. Picked
as the **simplification target** if Shape B's sentence-level scoring
doesn't earn its complexity empirically.

### Shape C — Multi-chunk parallel embedding

Split the input into 128-token chunks; embed and match each independently;
union the matched ways. Preserves every byte of prose but multiplies the
per-call embed cost by chunk count, which makes the spawn-storm worse,
not better. Also makes "best match" semantics across chunks ambiguous —
which chunk's score does a way inherit? Rejected on the perf axis.

### BM25 against the corpus vocabulary

The original sketch in the design conversation. Rejected by ADR-125 and
by the user's clarifying note in this thread: the corpus's `vocabulary:`
fields are tuned for embedding-space matching, not lexical-scoring
matching. Re-introducing BM25 against them would be the regression
ADR-125 was specifically trying to prevent.

### LLM-based intent extraction (LLMLingua-style)

Use a small LLM to compress the input to its intent. Highest quality
in the literature; **rejected** here because it adds a model invocation
ahead of the embed invocation — the exact "embedding repeatedly" failure
mode this work is meant to eliminate. Cost outranks quality for this
problem.

### Plain truncation (status quo via PRs #94–96)

Keep the character caps; do nothing more. Works for crash prevention,
fails for signal preservation. Acceptable as immediate mitigation
(already shipped); insufficient as the long-term answer for agent
dispatch prose specifically. The PRs are the safety net under this ADR,
not its replacement.

## Open Questions

- **Budget calibration.** The token budgets above are eyeballed from the
  128-token model window. They should be validated empirically — too
  large and the embedder degrades inside its window; too small and the
  reducer over-prunes. A small benchmark using existing way-matching
  ground truth (the same fixtures ADR-127 used) would set defaults.
- **Sentence splitter robustness.** Bash commands, JSON payloads, and
  code snippets don't have natural sentence boundaries. The reducer
  needs a fall-through: if `split_sentences` returns one (or zero)
  sentence on a non-prose input, degrade to Shape A on tokens. This is
  a few extra lines but worth deciding before implementation.
- **Stopword list.** Start with a minimal set (`the`, `a`, `is`, `of`,
  `and`, …) and grow empirically based on what scaffold tokens dominate
  agent prompts in practice. Out of scope to enumerate here; settling
  the list belongs in the implementation PR.
- **Observability.** The fail-open silence that hid the original crashes
  for two weeks suggests the embed pipeline needs a place where failures
  surface. Not in scope for this ADR (it's a separate observability
  concern that applies regardless of the reducer), but worth tracking
  as a follow-up.
