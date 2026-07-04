# The matching engine — source-cited reference

The authoritative statement of how the ways matching engine behaves: config
defaults, the fire rule, calibration, and parent-boost. **Every fact cites its
source location.** This is the reference other docs are reconciled against — if a
doc and source disagree, source wins and the doc is corrected. If this file and
source ever disagree, re-read source and fix this file, and update the line
citations when the engine changes. This exists because prose summaries drift; a
source-cited sheet does not.

## Config defaults — `tools/ways-core/src/config.rs`

| Key | Default | Line |
|---|---|---|
| `semantic_fire_probability` (τ_s) | **0.5** | 130 |
| `keyword_floor_probability` (τ_k) | **0.15** | 131 |
| `parent_threshold_multiplier` | **0.8** | 128 |
| `parent_boost_floor` | **0.30** | 129 |
| `refire_presets["normal"]` | 0.15 | 120 |

Retired keys, hard-mapped to deprecation warnings (config.rs:258–260):
`default_embed_threshold` → `semantic_fire_probability`;
`default_multi_embed_threshold` → `semantic_fire_probability`;
`keyword_gate_fraction` → `keyword_floor_probability`.
There is **no** `embed_threshold` frontmatter field and **no** per-way threshold.

## Calibration g(s) — `tools/ways-core/src/calibration.rs`

- `probability(cosine) = sigmoid(a·cosine + b)` (L30–31). This is `g(s)`.
- `fit()` filters non-finite cosines (L61) and **rejects** a fit with non-finite or
  `a ≤ 0` slope (L73) — never ships a curve mapping higher cosine to lower probability.
- AUC is computed on the fitted predictor `a·x+b` (L79); a near-flat fit scores ≈ 0.5
  and fails the ship-gate.
- Ship-gate: `const AUC_FLOOR: f64 = 0.70` (`corpus.rs:766`). A fit below 0.70 is not
  written; scan then degrades (keyword fails open, semantic silent) rather than trust it.
- Fit runs at corpus generation (`corpus.rs:191`), stored in `embed-manifest.json`.
  Deployed values: EN `a≈26.7 b≈−7.8 AUC≈0.955`; multi `AUC≈0.941`.

## Fire rule — `tools/ways-cli/src/cmd/scan/`

Two models: EN (`minilm-l6-v2`, 384-dim) and multilingual (768-dim, localized mode)
(`scoring.rs:3`). Each yields a cosine → `g(s)` probability `prob_en` / `prob_multi`
(`scoring.rs:36,41`); `None` when the way isn't embeddable or no calibration is loaded.

Per way, per prompt (`scan/mod.rs` `match_prompt`, ~583–623):

- **Semantic lane** fires if `prob_en ≥ τ_s ∨ prob_multi ≥ τ_s` (mod.rs:621–623), where
  τ_s is `EffectiveThresholds.semantic` (see parent-boost).
- **Keyword lane**: a `pattern:` regex match fires **only if** `prob_en ≥ τ_k ∨
  prob_multi ≥ τ_k` (mod.rs:598–600) — the floor gate. **But** if `pattern_strict` is
  set **or** there is no calibrated signal at all (`no_signal`), the keyword fires
  **unconditionally** (fails open) — the author's explicit trigger stands.
- Lanes are additive-OR; τ_k and τ_s are independent. A gated keyword never shadows a
  semantic fire — the gated verdict is held and the semantic lane is checked first.
- `pattern_strict: true` also bypasses the URL / code-fence mask (mod.rs:145).

Keyword matching is **case-sensitive** against the original-case (masked) prompt — only
code fences and URLs are stripped, no lowercasing (mod.rs:145, `mask_nonlinguistic`).

## Parent-boost — `tools/ways-cli/src/cmd/scan/mod.rs` `effective_thresholds` (753–782)

`base = semantic_fire_probability` (τ_s). If any ancestor way has been shown this
session, the child's effective semantic threshold is:

    (base × parent_threshold_multiplier).max(parent_boost_floor)
    = max(0.5 × 0.8, 0.30) = 0.40        (defaults)

**Both keys are live.** The multiplier (0.8) lowers the child's bar — the boost; the
floor (0.30) stops cascading boosts from reaching the noise band. All in probability
space: ADR-156's change was the *operand* (τ_s, not a raw per-way cosine threshold), not
removal of the multiplier. τ_k (keyword floor) is global and is **not** parent-boosted.

## Still-current mechanisms (a separate concern from relevance scoring)

- `ways tune` — locale alias audit (fidelity / discrimination vs the English root
  anchor), ADR-139/125. Never writes relevance thresholds.
- Salience / signal **decay** — ADR-121 turn-based exponential; a distinct model.
- Progressive disclosure and token-gated re-fire (ADR-104/105/126); `refire` as a
  fraction of the context window (ADR-126); three-root runtime (ADR-143); sentence-
  salience input reduction (ADR-130); authored disclosure graph / no BM25 (ADR-125).

## See also

- `authoring-docs-style.md` — how to write docs about the engine (and what's retired).
- ADR-156 — calibrated relevance scoring. ADR-155 — keyword-channel gating, `pattern_keep`.
