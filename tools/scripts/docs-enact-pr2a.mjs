export const meta = {
  name: 'docs-enact-pr2a',
  description: 'Source-grounded rewrite of the 5 engine/scoring docs to the ADR-156 calibrated model + a provenance banner on the lexical-gate design note; every engine claim verified against source with file:line citations',
  phases: [{ title: 'Rewrite', detail: 'one agent per engine doc; claims verified against source' }],
}

const REPO = '/home/aaron/Projects/ai/harness/agent-ways'
const STYLE = `${REPO}/docs/hooks-and-ways/authoring-docs-style.md`
const REF = `${REPO}/docs/hooks-and-ways/engine-reference.md`

const FILES = [
  { path: 'docs/hooks-and-ways/matching.md', mode: 'rewrite', target: 'reference',
    preserve: "way-graph section (L5-54), alias/max-over-aliases, localized-mode multilingual lane (ADR-139), regex design (L131-153), the IR-lineage essay (L194-239), state triggers (L241-264). Only the routing/scoring/progressive-disclosure core (L56-129, L162-168) is stale." },
  { path: 'docs/hooks-and-ways.md', mode: 'rewrite', target: 'reference',
    preserve: "hook-lifecycle / scope / marker / state-trigger sections (L1-126, L223-329, L410-476) are accurate. Rework: the matching-modes mermaid (L131-158), the Semantic Matching section (L172-221), Telemetry (L397-408). Calibration/probability is ENTIRELY MISSING and must be added. `ways tune-curves`/`tune-precision` are fine except embed_threshold remedies." },
  { path: 'docs/hooks-and-ways/scoring-and-testing.md', mode: 'rewrite', target: 'how-to',
    preserve: "self-validating-loop framing (L5-26), 'how test prompts get written' (L34-55), the vocabulary-authoring trap table (L178-184), sparsity/anti-overfitting + intentional-co-fire (L186-211, minus the threshold-lever bullet), tools table (L213-229; ways tune/tune-curves/tune-precision are current). The BM25-era worked example (L72-157) must be regenerated on the calibrated model." },
  { path: 'docs/signal-analysis/README.md', mode: 'rewrite', target: 'how-to',
    preserve: "the signal-vs-noise separation concept; scripts/signal-report.py exists. Re-anchor on ADR-156 calibration + global τ_s/τ_k + embed-manifest.json + probe-measure.py. CRITICAL: the two `ways tune` references (L45-46) are STILL CURRENT (locale audit, ADR-139/125) — do NOT remove them." },
  { path: 'docs/hooks-and-ways/stats.md', mode: 'rewrite', target: 'reference',
    preserve: "near_miss_margin (0.05, ADR-134), tail-compaction, scopes/teams, stats.sh flags, JSONL-append-only framing, data-locations table — all current. Staleness is confined to threshold-bearing example fields + config-name asides. CONFIRM the real events.jsonl field names from source (ADR-153/156 telemetry: does way_nearmiss carry thr_en/thr_multi or prob_en/prob_multi/tau_s? check scan/mod.rs log_keyword_gated / near-miss logging) — DO NOT GUESS." },
  { path: 'docs/design-notes/lexical-gate-as-conditional-threshold.md', mode: 'banner', target: 'explanation',
    preserve: "the ENTIRE body — this is timestamped reasoning that led to ADR-156. Do NOT rewrite it. Only PREPEND a dated provenance banner per style-guide §5." },
]

const SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['file', 'rewritten_content', 'claims_ledger', 'change_summary', 'preserved', 'follow_ups'],
  properties: {
    file: { type: 'string' },
    rewritten_content: { type: 'string', description: 'the COMPLETE new file content, ready to write verbatim' },
    claims_ledger: {
      type: 'array',
      description: 'EVERY factual claim about the engine you wrote (numbers, mechanisms, config keys, fire rule), each with the source location that backs it. This is how the claim is verified.',
      items: { type: 'object', additionalProperties: false, required: ['claim', 'source'],
        properties: {
          claim: { type: 'string', description: 'the engine fact as stated in your rewrite' },
          source: { type: 'string', description: 'file:line in source or engine-reference.md that backs it (e.g. config.rs:130, scan/mod.rs:777, engine-reference.md#parent-boost)' },
        } },
    },
    change_summary: { type: 'array', items: { type: 'string' } },
    preserved: { type: 'array', items: { type: 'string' } },
    follow_ups: { type: 'array', items: { type: 'string' } },
  },
}

function rewritePrompt(f) {
  return `You are enacting the SOURCE-GROUNDED rewrite of one engine/scoring doc to the ADR-156
calibrated matching model. A prior pass introduced a factual error (claimed a live config key
was retired) by trusting a prose summary instead of source. You will NOT repeat that: every
engine claim you write must be verified against SOURCE and carry a citation.

FILE: ${REPO}/${f.path}
TARGET DIÁTAXIS MODE: ${f.target}

STEP 1 — read, in order:
  1. ${REF}  — the SOURCE-CITED engine reference. This is the source of truth for every number,
     the fire rule, calibration, and parent-boost. Its facts cite config.rs / scan/mod.rs /
     calibration.rs line numbers. Treat it as authoritative.
  2. ${STYLE} — the style guide: retired-vocabulary table, Diátaxis rule, voice.
  3. ${REPO}/${f.path} — the file you are rewriting.
If anything in the file, the reference, or your own knowledge seems in tension, GO READ THE
SOURCE (tools/ways-core/src/config.rs, tools/ways-cli/src/cmd/scan/mod.rs,
tools/ways-core/src/calibration.rs) and let source decide. You have Read/Grep/Bash — use them.

STEP 2 — rewrite. Purge every retired-model claim (embed_threshold, config.default_embed_threshold,
keyword_gate_fraction / γ, raw-cosine thresholding s≥T, per-way threshold tuning, "raise/lower the
threshold"). State the calibrated model to match the reference EXACTLY:
  - g(s)=σ(a·s+b); fire iff g(s)≥τ_s(0.5) ∨ (keyword ∧ g(s)≥τ_k(0.15)); τ_s/τ_k independent globals.
  - keyword lane floor-gated, BUT fails open with no calibrated signal; pattern_strict bypasses by design.
  - parent-boost = (τ_s × parent_threshold_multiplier 0.8).max(parent_boost_floor 0.30) = 0.40 by
    default. BOTH keys live — do NOT say the multiplier was retired.
DO NOT re-derive the full fire rule in longhand if another doc owns it — state it tersely and link
to engine-reference.md (single source of truth; avoid drift across docs).
For numbers you can't find in the reference (e.g. exact events.jsonl telemetry field names for
stats.md), READ THE SOURCE and cite it — never guess field names.

PRESERVE (do not disturb): ${f.preserve}

STEP 3 — Diátaxis: shape the doc toward ${f.target} mode. If a large block is a different mode
(e.g. an explanatory essay inside a reference doc), note it in follow_ups rather than deleting it —
splitting files is a later pass.

Return the COMPLETE rewritten file in rewritten_content, and a claims_ledger listing EVERY engine
fact you asserted with its source citation. Sober, literal voice. Do not touch the still-current
mechanisms (ways tune locale audit, salience decay ADR-121, progressive disclosure).`
}

function bannerPrompt(f) {
  return `Add a provenance banner to a timestamped design note (style-guide §5). Do NOT rewrite the body.

FILE: ${REPO}/${f.path}
Read ${STYLE} §5 (timestamped artifacts) and ${REF} for the correct current model, then read the file.

This note was written DURING the ADR-156 exploration (pre-ship). Its body treats the γ·T gate on
raw cosine, per-way embed_threshold, and keyword_gate_fraction as the LIVE control surface, and
frames ADR-156's calibration as forthcoming. That is correct history, not a bug — the ROC /
log-odds reasoning is the note's value. Do NOT rewrite it to present tense.

PREPEND a short dated provenance banner (a blockquote, right after the H1 title) that: says it was
written during the ADR-156 exploration pre-ship; states that the body's live-control-surface framing
(γ·T, embed_threshold, keyword_gate_fraction) was superseded by the shipped calibrated g(s) with
independent global τ_s/τ_k; and points the reader to ADR-156 and engine-reference.md for the shipped
model. Keep the ENTIRE existing body verbatim below it.

Return the complete file (banner + original body) in rewritten_content. claims_ledger: just the
banner's factual claims with citations. Note in follow_ups that the body is preserved as history.`
}

phase('Rewrite')
const results = await parallel(
  FILES.map((f) => () =>
    agent(f.mode === 'banner' ? bannerPrompt(f) : rewritePrompt(f),
      { label: `${f.mode}:${f.path.split('/').pop()}`, phase: 'Rewrite', schema: SCHEMA, agentType: 'general-purpose' })
  )
)
const ok = results.filter(Boolean)
log(`${ok.length}/${FILES.length} returned; ${ok.reduce((n,r)=>n+(r.claims_ledger||[]).length,0)} source-cited claims to verify`)
return { count: ok.length, results: ok }
