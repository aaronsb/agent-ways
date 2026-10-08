# The matching engine — source-cited reference

The authoritative statement of how the ways matching engine decides a fire: config defaults, the fire rule, calibration, parent-boost, and where the relevance gate sits. Every fact cites its source by symbol and file. If a doc and source disagree, source wins and the doc is corrected. If this file and source disagree, re-read source and fix this file. Citations name functions and constants rather than line numbers, so they survive edits around them.

## Config defaults — `tools/ways-core/src/config.rs` `impl Default for Config`

| Key | Default |
|---|---|
| `semantic_fire_probability` (τ_s) | **0.5** |
| `keyword_floor_probability` (τ_k) | **0.15** |
| `parent_threshold_multiplier` | **0.8** |
| `parent_boost_floor` | **0.30** |
| `near_miss_margin` | 0.05 |
| `admission` | `share` (or `chunk_top`; see the late-interaction step 4) |
| `body_rank` | `off` (or `on`; adds `0.25 × best body section` to each way's cosine while the body sidecar is complete, on both the late-interaction and single-vector paths, and confirms against a section other than the one that contributed; under evaluation as ADR-701 §6, results in `experiments/content-corpus/results-body-rank.md`) |
| `refire_presets` | `once` 1.0, `rare` 0.4, `normal` 0.15, `frequent` 0.05 |

Retired keys warn and are ignored (`tools/ways-core/src/settings.rs` `RETIRED`): `default_embed_threshold` and `default_multi_embed_threshold` (use `semantic_fire_probability`), `keyword_gate_fraction` (use `keyword_floor_probability`). There is no `embed_threshold` frontmatter field and no per-way threshold.

## Calibration g(s) — `tools/ways-core/src/calibration.rs`

- `ModelCalibration::probability(cosine) = sigmoid(a·cosine + b)`. This is `g(s)`.
- `fit()` drops non-finite samples and rejects a fit whose slope is non-finite or `a ≤ 0`, so no curve maps a higher cosine to a lower probability.
- AUC is computed on the fitted predictor `a·x + b`. A near-flat fit scores about 0.5 and fails the ship gate.
- Ship gate: `AUC_FLOOR = 0.70` in `fit_calibration` (`tools/ways-cli/src/cmd/corpus.rs`). A lane below it is left uncalibrated, and scan then degrades (keyword fails open, single-vector semantic silent) rather than trust it.
- The fit runs at corpus generation from the committed probe corpus, `include_str!`-baked from `calibration_probes.jsonl` in `fit_calibration`, never from runtime telemetry. It is stored in `embed-manifest.json`. Deployed values: EN `a≈26.7 b≈−7.8 AUC≈0.955`, multi `AUC≈0.941`.

## Fire rule — `tools/ways-cli/src/cmd/scan/mod.rs` `match_prompt`

`match_prompt` decides one way on the prompt, queued and task surfaces. The input to the semantic channel is `reduce_for_embed` of the surface (for the prompt, the prompt plus Claude's last response, ADR-130 and ADR-155). Two signals are computed once per scan:

- **Single-vector probabilities.** `batch_embed_score` embeds the reduced surface once per model: EN (`minilm-l6-v2`, 384-dim), plus multilingual (768-dim) in localized mode. Each way's cosine is mapped through `g(s)` to `prob_en` and `prob_multi` (`scoring.rs`). `None` means the way is not embeddable or no calibration is loaded.
- **Late-interaction verdicts.** `late_interaction::run` (ADR-160) returns `Some(Verdicts)` or `None`.

### Channel 1 — keyword

A `pattern:` regex hit fires when `prob_en ≥ τ_k ∨ prob_multi ≥ τ_k`, the keyword floor. It fires unconditionally when `pattern_strict` is set or when there is no calibrated signal on either lane (`no_signal`, fails open). Otherwise the veto is held while the semantic channel is checked, and reported as `way_keyword_gated` only if nothing fired. `pattern_strict: true` also bypasses the URL and code-fence mask (`scan_prompt_surface`, `mask_nonlinguistic`).

### Channel 2 — semantic, in two tiers

**Primary: late-interaction** (`scan/late_interaction.rs`). When `run` returns `Some`, it alone decides the semantic channel. A way fires when `Verdicts::fired_score` returns a score, on channel `semantic:late-interaction:en`, and the single-vector branch is not consulted. The matcher:

1. splits the reduced surface into sentence chunks (at most `MAX_SURFACE_CHUNKS` 12, each at least `MIN_CHUNK_CHARS` 12) and embeds them in one `way-embed` batch against `ways-corpus-en.jsonl`;
2. ranks each way by its peak cosine over the chunks;
3. within each chunk runs a softmax over the top `TOP_K_PER_CHUNK` (8) ways at temperature `SOFTMAX_TAU` (0.08), and sums each way's share over the chunks divided by their number;
4. admits a way whose peak is at least `PEAK_GATE` (0.50), or that passes the `admission` rule (`admit`): under `share`, the default, a share of at least `SHARE_GATE` (0.15); under `chunk_top`, being the top-ranked way of some chunk (ADR-700 §12, under evaluation as ADR-701 increment 6);
5. confirms at most `MAX_WINNERS_TO_CONFIRM` (6) admitted ways, highest peak first, by the cosine between the chunk each one won and its own body chunks (at most `MAX_BODY_CHUNKS` 8), which must reach `CONFIRM_GATE` (0.35).

These operating points are hand-set and uncalibrated. The fired score, logged as `fire_score`, is the summed share.

**Fallback: single vector.** `run` returns `None` when the surface yields fewer than 2 chunks, when the EN binary, corpus or model is missing, or when a `way-embed` call fails during chunk matching or body confirmation. The way then fires when `prob_en ≥ τ_s` (channel `semantic:embedding:en`) or `prob_multi ≥ τ_s` (`semantic:embedding:multi`), where τ_s is `EffectiveThresholds.semantic` after parent-boost. The fired score is `g(s)`.

Late-interaction reads only the EN corpus. On a localized install the multilingual lane therefore decides only on fallback surfaces.

### What reads the single-vector probabilities on both tiers

- The keyword floor τ_k (channel 1).
- Near-miss logging: when nothing fired and `τ_s − near_miss_margin ≤ p < τ_s` on a lane, `log_near_miss` emits `way_nearmiss` with `prob_en`, `prob_multi`, `tau_s`, `margin`.

### Other surfaces

- **Bash** (`scan::command`): `commands:` on the command, then `pattern:` on the tool description, fire on the regex alone. Its semantic lane is single-vector only (`semantic:bash:en|multi`, `p ≥ τ_s` after parent-boost) over the command, the description and Claude's prose since the last human turn (`lookbehind::intent`). State-triggered ways are excluded, and nothing is logged as a near-miss.
- **File** (`scan::file`): `files:` regex only.

Keyword matching is case-insensitive (ADR-157): every trigger regex is compiled by `compile_trigger` with `case_insensitive(true)`, so a lowercase pattern matches the uppercase acronyms users type (`\bssh\b` matches `SSH`). The text keeps its original case, with only code fences and URLs stripped (`mask_nonlinguistic`). A pattern may opt back into case sensitivity with an inline `(?-i)`. Authoring rule: write patterns in lowercase and mean the concept.

## Parent-boost — `scan/mod.rs` `effective_thresholds_in_scan`

`base = semantic_fire_probability` (τ_s). If an ancestor way has a marker for the current agent, or fired earlier in the same scan, the child's effective semantic threshold is:

    (base × parent_threshold_multiplier).max(parent_boost_floor)
    = max(0.5 × 0.8, 0.30) = 0.40        (defaults)

The multiplier lowers the child's bar. The floor stops cascading boosts from reaching the noise band. τ_k is not boosted.

Parent-boost changes τ_s, so it applies where τ_s decides: the single-vector fallback and the Bash semantic lane. It has no effect when late-interaction decides. A child that fired only on a boost from a parent fired in the same scan is shown only if that parent is shown (`withheld_for_parent`, `has_shown_ancestor`). Candidates are walked in tree order (`collect_candidates`), so a parent is always decided before its children.

## From fire to injection

After matching, every lane orders its hits (`scan/order.rs` `order_hits`) and shows them through `show::way_scored`, which checks the domain and per-way switches (already applied once in `collect_candidates`), the way's refire window (ADR-126), and, on the matching and post-tool lanes, the hook's 10,000-character budget (`HOOK_CONTEXT_CAP`). The state lane runs without a budget (`scan::state`). On the prompt and queued surfaces the relevance gate sits between ordering and showing.

## Relevance gate — `scan/gate.rs` `apply`

On the prompt and queued surfaces, `scan_prompt_surface` sends the hits that `show::would_fire` says would reach the agent to the ways agent in one request (ADR-196, ADR-502). In `enforce` mode a blocked way is skipped before its fire is recorded, so it leaves no marker and keeps its refire budget. The task, Bash, file, state and post-tool lanes are not judged. The settings are `gate.mode` (`enforce`, `shadow`, `off`) and `gate.engine`, and only a key file turns the gate on.

What the judge sees, its threshold, candidate cap, timeout, failure behaviour and cost are explained in [the relevance judge](../explanation/relevance-judge/relevance-judge-the-model.md), which this page does not restate.

## Related mechanisms

- `ways tune locale`: locale alias audit (fidelity and discrimination against the English root), ADR-139 and ADR-125. Never writes relevance thresholds.
- Re-disclosure cadence: `refire` as a fraction of the context window (ADR-126), kept per agent (`session::agent_state_dir`). The ADR-123 `curve:` block was removed by ADR-159.
- Way roots: project, user, core, with id shadowing (ADR-143, `collect_candidates`).
- Input reduction by sentence salience (ADR-130), and the authored disclosure graph with no BM25 (ADR-125).

## See also

- `authoring-docs-style.md`: how to write docs about the engine, and what is retired.
- ADR-156 (calibrated relevance scoring), ADR-155 (keyword-channel gating, `pattern_keep`), ADR-160 (late-interaction), ADR-196 (relevance gate).
