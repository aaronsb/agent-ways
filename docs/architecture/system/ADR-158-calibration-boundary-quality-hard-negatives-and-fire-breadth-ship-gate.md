---
status: Accepted
date: 2026-07-04
deciders:
  - aaronsb
  - claude
related:
  - ADR-156
  - ADR-155
  - ADR-125
---

# ADR-158: Calibration boundary quality — hard negatives and a fire-breadth ship gate

## Context

Observed in a live session: *nearly every way fired.* Telemetry confirmed it —
104 of 157 ways fired across the session, one scan firing 35 ways. The cause is
**not** the keyword lane (ADR-157) and **not** fail-open (calibration loaded, EN
AUC 0.955). It is the **semantic lane's boundary position and shape**.

The deployed calibration is `g(cos) = σ(26.66·cos − 7.81)` (EN). Two facts fall
out of those coefficients:

1. The fire bar `τ_s = 0.5` maps to **cosine ≥ 0.293** — a low similarity
   threshold. Any way whose alias sits within 0.293 cosine of the prompt fires.
2. The slope (a ≈ 26.66) is near-vertical: past cosine ~0.45 the probability
   **saturates to ~1.0**. A prompt "write an adr documenting this architecture
   decision" fires `delivery/implement` at cos 0.603 → g **0.9997** and
   `architecture/design/prototype` at cos 0.573 → **0.9994** — both scoring
   higher than they should, and the ranking signal at the top is gone.

A purpose-built instrument (`tools/scripts/fire-panel.py`, scoring one prompt
against all aliases) quantifies the breadth against an expectation panel
(`fire-panel.json`):

| bucket | expect | fired (cos 0.293) |
|---|---|---|
| off_topic (France, a haiku, Everest) | ~0 | **0.0** |
| narrow (rename a var, one unit test) | 1–4 | **1.0** |
| adjacent (write an ADR, review a PR) | a few | **8.0** (max 17) |
| broad (a wrap prompt naming every domain) | many | **17** |

So it is not "fires on everything" — genuinely off-topic prompts fire zero. It
is **"fires on everything on-topic, saturated."** Discrimination between
*relevant* and *tangential* has collapsed.

**Root cause.** The calibration is fit at corpus generation from the committed
probe corpus (`calibration_probes.jsonl`, `include_str!` at `corpus.rs:765`) —
**96 probes across 6 ways**, and each probe is scored only against *its own* way's
alias (`corpus.rs:797`). The *noise* probes are **easy** — clearly off-topic, low
cosine — so the logistic fit places the boundary low (cos 0.293) and the slope
steep (a clean gap between easy negatives and intents produces a near-vertical
sigmoid). **High AUC on this probe set does not bound the real false-positive
rate**, because the negatives do not represent the true adversary: *adjacent-domain
real prompts* that sit at moderate-to-high cosine but should not fire.

A compounding factor is self-inflicted: the corpus embeds `description +
vocabulary` (`corpus.rs:380`, `load_aliases` at `:857`). ADR-155 §5's
pattern-hygiene sweep moved common words *out of* `pattern:` and *into*
`vocabulary:` — which **broadens each swept way's alias centroid**, nudging
on-topic cosines up. Pattern hygiene became alias bloat.

The residual after any global threshold move is instructive: even at cos 0.45,
"write an adr" still fires `implement` (0.603) and `prototype` (0.573) — because
those *aliases* are too broad (they carry ADR/architecture vocabulary). A global
threshold cannot separate them from the legitimate `adr` fire (0.735); only
tightening the aliases can.

## Decision

Treat **boundary quality** — not just probe separability — as the calibration's
ship criterion, and fix it along three axes.

1. **Hard negatives in the probe corpus.** `calibration_probes.jsonl` must carry
   *adjacent-domain* hard negatives: prompts with **high** cosine to a way that
   should **not** fire it (label 0). Placing negatives *in the gap* flattens the
   slope (de-saturating the probabilities) and raises the crossover (fewer false
   fires) in one coherent refit — the principled version of moving the boundary,
   as opposed to bending `τ_s`. A rich source is cross-way mining: one way's
   intent probes are hard negatives for an embedding-adjacent way they must not
   trigger.

2. **A fire-breadth ship gate.** `tools/scripts/fire-panel.{py,json}` — a
   committed panel with per-bucket expectations — is a regression asset checked
   alongside `AUC_FLOOR`. A corpus build regresses if `off_topic > 0`, or a
   bucket's fire-breadth rises materially versus the recorded baseline. AUC
   measures probe separability; the panel measures the thing users feel. Both
   gate a refit.

3. **Per-way alias discipline.** A way's alias (`description + vocabulary`) is its
   semantic fingerprint; over-broad vocabulary causes cross-domain bleed. Aliases
   the panel shows bleeding get **tightened** — the counter-discipline to
   ADR-155 §5. Pattern hygiene may not silently become alias bloat: a word moved
   out of `pattern:` belongs in `vocabulary:` only if it is genuinely
   discriminating for *this* way, not merely suggestive.

The `τ_s` config value stays at 0.5 as the calibrated-probability contract
(ADR-156); the boundary moves by fixing the *fit*, not the threshold.

## Consequences

### Positive

- Adjacent-prompt precision rises (fewer tangential ways injected); the context
  window stops filling with 8–35 marginal ways.
- Flattening the slope restores meaningful probabilities across the range, so
  parent-boost and near-miss ranking regain signal.
- The fire-breadth gate makes over-firing a *caught regression*, not a thing a
  user notices in production — and turns a felt symptom into a measured number.
- Establishes the discipline that a lint suppression / vocabulary choice is a
  claim to be measured (shared with the `pattern_keep` governance in #308).

### Negative

- Authoring and maintaining hard-negative probes and the panel is ongoing work.
- Hard negatives can drop AUC below `AUC_FLOOR` if a way's intents and its
  adjacent negatives are truly inseparable by cosine-to-one-alias. When that
  happens the fix is **alias tightening**, not more negatives — the signal, not
  the threshold, is the limit.

### Neutral

- The panel measures raw `τ_s` fires and does not model parent-boost (which only
  *lowers* a child's bar), so it is a sound lower bound and a consistent
  before/after proxy, not an exact production fire count.
- Multilingual calibration is fit from the English probe corpus (ADR-156); the
  hard negatives benefit both lanes.

## Alternatives Considered

- **Raise `τ_s`** (e.g. to 0.9 → cos bar ~0.375). Rejected as the primary fix: a
  weak lever under this slope (0.5→0.99 moves the bar only 0.293→0.465), it
  entangles parent-boost (whose base is `τ_s`), and it papers over the saturation
  rather than fixing it. Retained only as an emergency relief valve.
- **Per-way thresholds.** Rejected — ADR-156 deliberately removed them in favour
  of one global calibrated scale; re-introducing them abandons that model.
- **Do nothing / accept the breadth.** Rejected — 104/157 ways firing wastes the
  context budget the whole system exists to protect (ADR-125), and saturated
  probabilities disable the ranking machinery downstream of the fire decision.
