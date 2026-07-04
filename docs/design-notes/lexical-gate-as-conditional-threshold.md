# The Lexical Gate as a Conditional Threshold

A reading of what the ADR-155 keyword gate *is*, underneath the implementation:
each way's keyword pattern is a one-dimensional detector, the gate is that
detector's operating point, and the point is placed — today — by a single
per-way number that also governs the semantic lane. Writing the mechanism down
in these terms explains why the pattern-hygiene remedies (remove a keyword,
raise a threshold, or leave it alone) are the *only* moves the current control
surface affords, and names the one move it cannot yet express.

## The two lanes, as one acceptance rule

A semantic way carries an alias — its `description` plus `vocabulary`, embedded
once into a unit vector `a_w` (ADR-125). An incoming prompt `q` embeds to `q`,
and the way's relatedness to the prompt is the cosine

    s = cos(q, a_w)          (way-embed emits s ≥ 0; a missing row means s < 0)

The way also carries a keyword pattern `P_w`. Let `k = 1` when `P_w` matches the
prompt and `0` otherwise. With a per-way semantic threshold `T_w` (frontmatter
`embed_threshold`, default 0.40) and the global gate fraction `γ`
(`keyword_gate_fraction`, default 0.4), ADR-155's additive rule is

    fire  ⇔  ( k ∧ s ≥ γ·T_w )  ∨  ( s ≥ T_w )

Because `γ ≤ 1`, the two disjuncts collapse into a single threshold on `s` whose
height depends on the keyword:

    fire  ⇔  s ≥ θ_w(k),      θ_w(k) = γ·T_w   if k = 1
                                        T_w     if k = 0

This is the whole idea in one line. **The keyword does not contribute to the
score. It selects which threshold the score must clear** — lowering the bar from
`T_w` to `γ·T_w` for prompts that carry the lexical anchor. The gate is not a
second signal added to the first; it is a *conditional threshold* on the first.

## Each keyword is a detector with an operating point

Restrict attention to the prompts that match `P_w` (the only ones the keyword
lane touches). Among them, some are on-topic ("author a new skill") and some are
lexical coincidence ("improve your communication skills"). Call their relatedness
distributions `S⁺` and `S⁻`. The keyword lane fires a matched prompt exactly when
`s ≥ b`, with `b = γ·T_w`. That is a binary classifier with a single scalar
boundary, and its behaviour is the textbook receiver-operating-characteristic:

    TPR(b) = Pr[S⁺ ≥ b]        FPR(b) = Pr[S⁻ ≥ b]

Sweeping `b` traces the keyword's ROC curve; `b = γ·T_w` picks one operating
point on it. Two facts about that curve decide everything downstream:

- **Separability.** A boundary that cleanly divides intent from noise exists iff
  the distributions do not overlap — informally `max S⁻ < min S⁺`, a *gap*. Any
  `b` in the gap is a perfect operating point. If the distributions overlap
  (AUC ≈ ½), *no* threshold separates them: the keyword carries no discriminative
  signal at any operating point.
- **Placement.** When a gap exists, precision and recall are traded by where in
  the gap `b` sits. Too low and noise leaks (false positives); too high and
  sub-threshold intent goes silent (false negatives).

This is why a bare common word like `remember` cannot be salvaged by tuning: its
`S⁻` (a coincidental "remember when the server crashed", cos ≈ 0.32) sits *above*
its `S⁺` (a genuine "remember this for next time", cos ≈ 0.22). The
distributions are inverted, the ROC hugs the diagonal, and there is no `b` that
admits the intent without admitting more of the noise. The right move is to
delete the detector, not to move its threshold.

## The control surface, and its one coupling

The engine exposes two knobs that touch `b`: the per-way `T_w` and the global
`γ`. The operating point is `b = γ·T_w`. But `T_w` is not free — it *also* sets
the semantic lane's own threshold in the second disjunct. Raising `T_w` to lift
the keyword's operating point simultaneously raises the bar at which the way
fires on relatedness alone:

    keyword operating point:  b       = γ · T_w
    semantic-lane threshold:  T_w     = b / γ        (2.5× b at γ = 0.4)

For a way whose intent language reliably carries its own keyword — a
*keyword-anchored* way — the semantic lane is vacuous (no prompt reaches `b/γ`),
so spending `T_w` to place `b` costs nothing. We confirmed this directly: pushing
`meta/skills` to `embed_threshold: 0.90` moved `b` from ≈0.16 to 0.36, flipped a
0.25 coincidence from *fired* to *gated*, and left a 0.61 intent prompt firing
through the gated keyword — no corpus rebuild, since `T_w` is read from
frontmatter at scan time.

The coupling only bites when a way needs a *high keyword floor and a live
semantic lane at once* — separable keyword noise to gate out, plus genuine
prompts that fire on relatedness without the keyword. There the single `T_w`
cannot serve both: raising it to gate the noise silences the semantic-only
intent in the band `[T_w^old, T_w^new)`. Expressing that case needs an operating
point decoupled from the semantic threshold — a per-way `b_w` (equivalently a
per-way `γ_w = b_w / T_w`) — which the current schema does not have. That is the
one move the control surface cannot make, and the seed of a follow-on decision.

## The remedies fall out of the geometry

The pattern-hygiene rework (ADR-155 §5) is, in these terms, choosing an
operating point per flagged keyword from what its ROC allows. Let `q⁻` be a high
quantile of `S⁻` and `q⁺` a low quantile of `S⁺`:

| Keyword's score geometry | Remedy | Why |
|---|---|---|
| `q⁺ ≥ T_w` — intent clears the semantic lane alone | **Remove** keyword | Semantic lane preserves recall; the keyword's only remaining effect is the false positives it admits |
| Gap (`q⁻ < q⁺`), way keyword-anchored | **Raise `T_w`** to seat `b ∈ (q⁻, q⁺]` | Keeps the keyword; gates the leak; semantic lane was vacuous anyway |
| Gap, but semantic lane is relied upon | **Decouple** `b_w` from `T_w` (not yet expressible) | Single knob can't place the floor without moving the semantic bar |
| Overlap (`q⁻ ≥ q⁺`, AUC ≈ ½) | **Remove** keyword | No separating boundary exists at any operating point |
| Short term-of-art token | **Anchor** (`\b…\b`) | Restores precision without touching the score lane |

The first four rows are a decision procedure the sweep can run per keyword from a
handful of probe measurements; the third is the only one that escalates to a
human, because it is the only one the tooling cannot satisfy.

## Where this sits in the literature

The pieces are individually well-trodden; the specific composition and its
visible coupling are what we arrived at independently.

- **Signal detection / ROC.** Treating each keyword as a scalar-thresholded
  binary detector, reading separability as AUC, and choosing `b` as an operating
  point is standard detection theory — the same framing used for keyword-spotting
  systems. Nothing new; it is the right lens.
- **Hybrid lexical–semantic retrieval.** Combining a lexical signal with a dense
  one is a mature area (COIL, Dense Lexical Representations, gated inner product,
  BM25+dense hybrid search). The mainstream combiner is *score-level fusion* — a
  weighted sum or reciprocal-rank fusion of two continuous scores, thresholded
  once.
- **Bayesian log-odds fusion.** The principled version calibrates each signal to
  a relevance probability and adds them in log-odds space (e.g. Bayesian BM25):
  `logit Pr[R | k, s] = c + β·k + g(s)`, fire iff `≥ logit τ`. For a *binary*
  lexical feature `k` this rearranges to `g(s) ≥ logit τ − c − β·k`, i.e. a
  keyword-conditional two-threshold rule — the score threshold drops by `β` when
  the keyword matches. **Our gate is exactly this decision boundary**, hard-coded
  rather than computed from a fused score, with the particular parameterization
  `b₁ = γ·T`, `b₀ = T`.

So we reinvented the binary-feature special case of log-odds evidence fusion. The
one thing the hard-gated form makes visible that the additive-score form hides:
because a single `T` scales *both* thresholds by a fixed `γ`, rather than an
independent lexical log-odds bump `β`, the keyword's operating point and the
semantic lane's threshold cannot be moved independently. The coupling is an
artifact of the parameterization, and naming it is the contribution — it is
precisely the knob a future decoupling would add.

## See also

- ADR-155 — the semantic gate on the keyword channel (the mechanism read here)
- ADR-125 — the canonical way alias (`description` + `vocabulary`) that `a_w` embeds
- ADR-134 — telemetry-driven threshold tuning; the near-miss / gated streams that
  measure `S⁺`/`S⁻` in production rather than from probes
- Robertson, *The Probability Ranking Principle in IR* — the ranking-as-probability
  frame the log-odds fusion rests on
- Bayesian BM25 (github.com/cognica-io/bayesian-bm25) — log-odds lexical–semantic
  fusion; the continuous-score sibling of this gate
