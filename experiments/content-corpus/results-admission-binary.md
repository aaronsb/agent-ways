# Chunk-top admission through the binary

Measured 2026-10-06 on branch `adr-701-chunk-admission` (from `ways-graph` 906894e9). Question from ADR-701 increment 6: admitting each chunk's top-ranked way was measured only in `recall.py`'s Python port (ADR-700 §12). Does the built binary reproduce the port, at today's share gate and in the new mode, and what does the mode change when the binary decides?

The corpus, way files, vocabulary, calibration and thresholds are as they are on `ways-graph`. Nothing here tunes them.

## Method

- **Binary.** `tools/target/release/ways` built from this branch. The setting is `matching.admission`: `share` (the default, today's rule) or `chunk_top`. Both keep the peak co-gate at 0.50, take survivors by peak up to 6, and body-confirm them unchanged.
- **Port.** `recall.py`'s `evaluate`. `share` is its shipped point (K 8, share 0.15, peak 0.50, cap 6, share / n_chunks). `chunk_top` is its `top1` pick (each chunk's top-ranked way, or peak ≥ 0.50, cap 6).
- **Surfaces.** `recall.py`'s: 310 main surfaces of two or three golden prompts, one chunk per prompt (seed 11), and 93 auxiliary surfaces built around the 3 golden prompts that split into two chunks (seed 13). Golden files: `golden-synthetic.tsv` and `tests/routing-golden.tsv`, 332 rows.
- **The binary's run.** `admission_binary.py` runs `ways author match --all --project EMPTY --json SURFACE` under a scratch HOME and XDG tree. Its corpus is `$OUT/alias.jsonl`, its ways are this checkout's `hooks/ways`, and its `config.yaml` sets `admission:`. There is no body sidecar, so confirmation is per call, as in the port. `--all` competes every way, as the port does. `--json` lists every candidate with the matcher's admitted and fired decision. The diagnostic now takes its admitted set from the same `admit` function as `late_interaction::run`, cap included.
- **Comparison.** Per surface, the admitted set and the fired set from the binary against the port's, and every body-confirm value the binary printed against the port's, within 0.001.
- **Labels and metrics** as in `results-recall.md`: *relevant* is an expected way, *related* an ancestor or descendant of one, *irrelevant* anything else. Recall after confirm is what reaches the judge. Irrelevant fired per surface is what the judge has to reject.

## The port, side by side with the binary

| Stage | `late_interaction.rs` | `recall.py` / `confirm.py` | Same here? |
|---|---|---|---|
| Surface | `reduce_for_embed` to 110 tokens, then chunks | none | yes: surfaces are built under the budget, so the reducer returns them unchanged |
| Chunks | `split_sentences`, whitespace joined, ≥ 12 chars, dedup, at most 12 | a port of the same | yes |
| Per-chunk scores | `way-embed match --batch --threshold 0.0` on the installed corpus | the same call on `$OUT/alias.jsonl` | yes: the scratch corpus is that file |
| Masking | rows kept only for enabled ways in the lane (toggles, scope, `when:`) | none | with `--all`, yes. In a live prompt scan, `collaboration/teams` (scope teammate) and the `when:`-gated ways compete in the port but not in the binary |
| Peak | max cosine over chunks, first chunk wins a tie | the same | yes |
| Share | Σ softmax mass over each chunk's top 8 at τ 0.08, / n_chunks | the same (`chunks` mode) | yes |
| Rule | `share ≥ 0.15` or `peak ≥ 0.50`; `chunk_top`: a chunk's first row, or `peak ≥ 0.50` | `share ≥ S` or `peak ≥ P`; `top1`: share 1.0 for any chunk's first row | yes |
| Cap | sort by peak, keep 6 | the same | yes; ties in peak order by hash-map order in Rust and by insertion order in Python, never seen to matter |
| Confirm | body sidecar when complete, else won chunk against `chunk_body` sentences, max | won chunk against `chunk_body` sentences, max, one batched call | yes: no sidecar in the scratch tree |
| Diagnostic | before this branch: top 20 by share, admitted without the cap | — | the old text table hid one admit (below) |

## Step 1: today's binary against the port

Before any Rust change, the `ways-graph` binary's text table (`--all`, top 20 by share) against the port at the shipped point: 402 of 403 surfaces had the same admitted and fired sets, and all 508 body-confirm values agreed within 0.001. The one difference was the table, not the matcher: on auxiliary surface 389, `softwaredev/code/supplychain/depscan` is admitted on its peak (0.533) with a share of 0.009 and ranks below 20th by share, so the table never lists it. `--json` lists every candidate. Recall matched `results-recall.md` exactly: 0.487 admitted, 0.403 after confirm, auxiliary 0.307 / 0.281.

## Step 3: both modes through the binary

Binary against port, every surface:

| mode | surfaces with identical admitted and fired sets | confirm values within 0.001 |
|---|---|---|
| share | 403 / 403 | 509 / 509 |
| chunk_top | 403 / 403 | 1122 / 1122 |

Main surfaces (310, 620 expected ways). Binary and port give the same numbers, so one row stands for both:

| mode | rec adm | rec fired | admitted / surface | irr admitted / surface | fired / surface | irr fired / surface |
|---|---|---|---|---|---|---|
| share | 0.487 | 0.403 (250) | 1.37 | 0.34 | 1.05 | 0.20 |
| chunk_top | 0.663 | 0.503 (312) | 2.56 | 1.15 | 1.51 | 0.46 |
| `results-recall.md` top1 (2026-10-05) | 0.656 | 0.503 | 2.56 | 1.16 | 1.53 | 0.47 |

Auxiliary surfaces (93, 228 expected ways):

| mode | rec adm | rec fired | fired / surface | irr fired / surface |
|---|---|---|---|---|
| share | 0.307 | 0.281 (64) | 0.80 | 0.09 |
| chunk_top | 0.658 | 0.557 (127) | 2.33 | 0.94 |

- `recall.py` run today on the same corpus gives these numbers too: top1 0.663 / 0.503, 2.56 admitted and 1.51 fired per surface, 0.46 irrelevant fired. Its paired bootstrap gives +0.100 [+0.076, +0.125] recall after confirm and +0.25 [+0.19, +0.31] irrelevant fired per surface. The 2026-10-05 run gave +0.27; the corpus has moved a little since `0bdd6619`.
- On the auxiliary surfaces the irrelevant fired count rises more, 0.09 to 0.94 per surface. Those surfaces repeat 3 prompts about 31 times each, so that figure is about three prompts.

**Per surface, as the binary decides (all 403).** chunk_top changed the fired set on 227 surfaces. Ways gained: 126 relevant, 6 related, 162 irrelevant. Ways lost: 1 relevant, 2 related, 4 irrelevant.

Gained, relevant:

- `documentation` on "Where should the stripe api key live, and how do i hash user passwords properly. How should we organize our docs…"
- `meta/governance` on "Auditor asked what standard justifies requiring conventional commit messages and dependency scanning…"
- `softwaredev/delivery/branching` on "What should I name this branch. We're shipping next week…"
- `meta/skills` on "Write a new skill for our release checklist…"

Gained, irrelevant:

- `softwaredev/code/security/auth` on "This change is huge and touches the core auth path, how careful should the automated checking be…" (expected `delivery/merge`, `code/quality/versioning`)
- `softwaredev/environment/hostparity` on "A fresh ci run pulled a newer transitive version than my laptop had and the build behaved differently…"
- `collaboration/onboarding-share` on "I've read my onboarding guide so many times i can't see its holes anymore…" (expected `documentation/validate`)

Several of these are close to the prompt's topic, which the synthetic labels count as irrelevant.

Lost. chunk_top drops a way that passed the share gate without winning a chunk and without a 0.50 peak:

- `meta/subagents` (relevant) on "Spawn parallel agents to explore these three approaches. Set up my ssh agent…". Another way wins its chunk.
- `softwaredev/code/testing/gates/assertions` (related) on two surfaces with "What should i assert in these tests…"
- `documentation/adr` and `ea/intelligence` (irrelevant), twice each.

## Gates

- **Routing golden** (`tests/test-routing-golden.sh`): 40/40 top-1, 3/3 `none` rows below 0.30, the same in both modes. The test scores single prompts with `way-embed match` and never runs late interaction, so admission cannot change it.
- **Share-mode output.** The new binary's `author match` text against the `ways-graph` binary's on all 403 surfaces: identical on 398 with no key and on 398 with `admission: share`. Every difference was two rows with the same printed share in swapped order. The `ways-graph` binary run twice against itself differed on 6 surfaces in the same way: `aggregate` sorts by share and breaks ties in hash-map order, which changes on each run. The decisions were identical on every surface.

## Limits

- **Synthetic surfaces and labels**, as in `results-recall.md`: joined golden prompts, not real multi-sentence prompts with response context. An "irrelevant" way can be useful.
- **Confirm per call only.** An install with a complete body sidecar confirms against section vectors (ADR-701 §7). `results-confirm.md` found it keeps and rejects at about the same rates at 0.35. It was not run in either mode here.
- **No masking.** `--all` competes every way, as the port does. A live scan masks the lane's disabled, out-of-scope and `when:`-gated ways first, and chunk_top reads chunk winners after that masking.
- **The judge was not run.** Irrelevant fired per surface is what it would see.
- **Multi-chunk prompts rest on 3 golden prompts.**

## Conclusion

The binary reproduces the port in both modes on every surface: the same admitted and fired sets on all 403 surfaces and the same body-confirm values. With `matching.admission: chunk_top`, recall after confirm on the main surfaces rises from 0.403 to 0.503, the figure `recall.py` reported. It rises from 0.281 to 0.557 on surfaces with a multi-sentence prompt. The judge sees 1.51 candidates per surface in place of 1.05, of which 0.46 are irrelevant in place of 0.20. The mode gains 126 relevant ways and loses one, a share-admitted way that won no chunk. Share mode decides as before on every surface.

## Reproduce

```sh
cargo build --release --manifest-path tools/Cargo.toml -p ways
mkdir -p bin && ln -sf "$PWD/tools/target/release/ways" bin/ways   # run.py builds the corpus with bin/ways
cd experiments/content-corpus
OUT=/tmp/adm-out python3 -c 'import run; run.OUT.mkdir(parents=True, exist_ok=True); run.build_alias_corpus()'
OUT=/tmp/adm-out python3 admission_binary.py --ways ../../tools/target/release/ways --mode both \
    golden-synthetic.tsv ../../tests/routing-golden.tsv
```

The first run scores confirm pairs into `$OUT/recall-confirm-cache.json`, shared with `recall.py`. Later runs take about two minutes, all of it the binary's per-surface calls. `--text` reads the table of a binary without `--json`, as Step 1 did with the `ways-graph` build. The summary is written to `$OUT/admission-binary.json`.
