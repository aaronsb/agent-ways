# Body confirmation: 8 embedded sentences vs precomputed section vectors

Measured 2026-10-05 on branch `spike/body-confirm` (from `ways-graph` 0945952a). Question from ADR-701 §6: if late-interaction body confirmation read precomputed heading-section vectors for the whole body, in place of embedding the first 8 prose sentences on every call, would it separate real matches from collisions at least as well, and how much faster would it be?

## Method

- **Corpora.** `OUT=/tmp/confirm-out python3 experiments/content-corpus/run.py <(ways author golden --tsv) tests/routing-golden.tsv`: 332 golden rows, 137 ways, 647 section chunks (4.7 per way). `bin/ways` was built with `make ways-rebuild`.
- **Surfaces.** `confirm.py` joins golden prompts as sentences into 310 surfaces (seed 11): 110 of two targeted prompts (T+T), 60 targeted plus `none` (T+N), 80 T+T+N, and 60 T+T+T. Targeted prompts in one surface come from different areas (the first two path segments) and their expected ways are not ancestors of each other. Each surface stays under the 110-token prompt budget, so `reduce_for_embed` returns it unchanged. Each surface splits into exactly one chunk per prompt.
- **Labels.** An admitted way is *relevant* when it is the expected way of one of the surface's prompts. It is *related* when it is an ancestor or descendant of an expected way, and *irrelevant* otherwise. The main table excludes related ways. A second table counts them as relevant.
- **Pipeline.** A Python port of `late_interaction.rs`: `split_sentences`, `chunk_surface`, `way-embed match --batch --threshold 0.0` against the alias corpus, softmax over each chunk's top 8 at τ 0.08, share = Σmass / n_chunks, admission on share ≥ 0.15 or peak ≥ 0.50, then up to 6 survivors by peak.
- **Confirm (a), today.** One `way-embed similarity --model … --batch` subprocess per survivor, pairing the won chunk with the way's `chunk_body` sentences (frontmatter and fences dropped, leading markdown characters stripped, dedup, cap 8). Score = max. This is the call `body_confirm` makes.
- **Confirm (b), sidecar.** The won chunk's vector, dotted against every section vector of the way. The section matrix is saved as `.npy` and loaded once. Score = max. Variant `(b) sections|alias` uses the way's alias vector when the way has no sections (ADR-701 §6 fallback).
- **Binary.** The scan resolves `~/.cache/agent-ways/user/way-embed` first (`paths::way_embed`). `run.py` uses `~/.claude/bin/way-embed`. The two are different builds, and their cosines differ by about 0.002. `confirm.py` re-embeds the section chunks with the engine-dir copy, so every number in the comparison comes from the binary the scan uses.
- **Verification.** `--verify` runs this checkout's `bin/ways author match` (`run_diagnostic`, the same `aggregate` / `body_confirm` / `chunk_body` code as `run`) on 30 sampled surfaces. It runs under a temporary HOME and XDG tree whose corpus is `$OUT/alias.jsonl` and whose `~/.claude/hooks/ways` links to this checkout. See the wrapper below. All 30 surfaces matched: about 535 rows of peak and share, and 39 body-confirm values, each agreeing within 0.001. WAYS_LI_DEBUG on a live `scan` was not used. The diagnostic exposes the same quantities through the same functions.

## Results

424 admitted candidates: 302 relevant, 16 related, 106 irrelevant. 302 of the 620 expected ways on the surfaces were admitted. Confirmation only judges what admission lets through.

| relevant vs irrelevant (302 / 106) | AUC | keep relevant @ 0.35 | reject irrelevant @ 0.35 |
|---|---|---|---|
| (a) first 8 sentences, per call | 0.659 | 0.828 | 0.406 |
| (b) section sidecar | 0.692 | 0.834 | 0.406 |
| (b) sections, alias when none | 0.697 | 0.841 | 0.406 |

- AUC(b) − AUC(a): +0.033. The 95% surface-bootstrap CI is [−0.019, +0.083]. The direction favours (b), and the difference is not significant.
- The (b) gate that matches (a)'s keep rate of 0.828 is 0.352. At that gate (b) keeps 0.834 and rejects 0.406, the same as (a) at 0.35. The equal reject counts (43 of 106 each) are a coincidence of totals: only 34 of the rejections are the same candidates. The correlation between (a) and (b) is 0.74.
- With related ways counted as relevant (318 / 106): AUC (a) 0.655, (b) 0.690. The CI is [−0.018, +0.087].
- At stricter gates the two trade places. At 0.40, (a) keeps 0.728 and rejects 0.528, while (b) keeps 0.732 and rejects 0.557. At 0.45, (a) keeps 0.599 and rejects 0.660, while (b) keeps 0.543 and rejects 0.651.

**By section count.** In the means columns, rel is relevant and irr is irrelevant.

| ways with | admitted | rel / irr | mean (a) rel / irr | mean (b) rel / irr | AUC (a) | AUC (b) |
|---|---|---|---|---|---|---|
| 0 sections | 2 | 2 / 0 | 0.503 / – | 0.000 / – | – | – |
| 1 section | 34 | 24 / 7 | 0.532 / 0.524 | 0.461 / 0.406 | 0.589 | 0.690 |
| 2+ sections | 388 | 276 / 99 | 0.479 / 0.400 | 0.475 / 0.376 | 0.669 | 0.696 |

- Two ways have no prose sections: `itops/policy` and `softwaredev/code/security/injection`. Twelve ways have one section.
- Under raw (b), both admitted zero-section candidates were relevant and were rejected with a score of 0. Today's chunk_body confirms them, because it keeps tables and the HTML comment that run.py's section chunking strips.
- The alias fallback recovers them. With it, both are kept, which is why `(b) sections|alias` keeps more relevant ways.
- One-section ways separate better under (b) than under (a), but the sample is small (24 / 7).

**Latency of the confirm stage per surface**, on this machine with the model warm in page cache:

| | mean | p50 | p95 |
|---|---|---|---|
| (a) `similarity --batch` subprocess per survivor | 108 ms | 86 ms | 239 ms |
| (b) numpy dot over preloaded sections | 0.06 ms | – | 0.13 ms |

- Each `similarity` call costs 79 ms. Almost all of that is process start and model load: 424 calls over 1.37 survivors per surface.
- Loading the sidecar takes 0.5 ms for 647 × 384 float32.
- Embedding the sections takes 8 s at build time.

## Limits

- **The won chunk needs a vector.** Path (b) uses it, and today's `way-embed match` subprocess does not return chunk vectors. Under the subprocess architecture, the sidecar saves the body embeddings only if the chunk vectors come from the match pass, which means extending `match` or using the in-process embedder ADR-701 assumes. Otherwise one embedding call per surface is still needed, about 80 ms, in place of one per survivor. The microsecond figure holds only once chunk vectors are in hand.
- **The labels are synthetic.** An "irrelevant" admitted way can be useful (a parent domain, say), and a relevant way can win a chunk from the other prompt. Related ways are reported both ways. There are only 106 negatives, so the CI is wide.
- **Admission recall is 49%.** Confirmation was measured only on what admission passes. A sidecar would not change admission.
- **The surfaces are concatenated golden prompts.** They are not real multi-sentence prompts with response context. the golden sidecars (`ways author golden --tsv`) has only 18 `none` rows, so T+N and T+T+N reuse them heavily.
- **(a) and (b) read different body text.** (a) reads the first 8 sentences, including tables, HTML comments and See Also. (b) reads all prose sections without those. The comparison is between the two designs as specified, not between chunkings of the same text.
- Timing is single-machine and wall-clock, with way-embed run as production runs it (fresh process per call).

## Conclusion

Confirming against precomputed section vectors separates relevant survivors from collisions at least as well as embedding the first 8 body sentences per call: AUC 0.692 against 0.659. The bootstrap CI on the difference, [−0.019, +0.083], includes zero, so this is "no worse", not "better". At the shipped 0.35 gate both keep about 83% of relevant ways and reject about 41% of collisions, so the gate can stay at 0.35. Ways with no prose sections must fall back to their alias vector, as ADR-701 §6 already says, or they lose real matches. The confirm stage drops from about 108 ms per surface (one 79 ms subprocess per survivor) to well under a millisecond of dot products. That saving needs the won chunks' vectors from the match pass or an in-process embedder. Otherwise one embed call per surface remains, about 80 ms.

## Reproduce

```sh
make ways-rebuild
OUT=/tmp/confirm-out python3 experiments/content-corpus/run.py <(ways author golden --tsv) tests/routing-golden.tsv
OUT=/tmp/confirm-out python3 experiments/content-corpus/confirm.py <(ways author golden --tsv) tests/routing-golden.tsv
```

For `--verify WRAPPER`, build a scratch HOME (`.claude/hooks/ways` → this checkout's `hooks/ways`, `.claude/bin/way-embed` → the real one) and a scratch `XDG_CACHE_HOME` (`agent-ways/user/` holding links to `minilm-l6-v2.gguf` and `way-embed`, and a copy of `$OUT/alias.jsonl` as `ways-corpus-en.jsonl`). Create an empty `/tmp/confirm-empty`. The wrapper is then:

```sh
#!/bin/sh
cd /tmp/confirm-empty
HOME=/tmp/confirm-home XDG_CACHE_HOME=/tmp/confirm-cache XDG_DATA_HOME=/tmp/confirm-xdg/data \
XDG_CONFIG_HOME=/tmp/confirm-xdg/config XDG_STATE_HOME=/tmp/confirm-xdg/state \
exec /path/to/checkout/bin/ways "$@"
```
