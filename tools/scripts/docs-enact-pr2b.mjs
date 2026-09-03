export const meta = {
  name: 'docs-enact-pr2b',
  description: 'Source-grounded single-claim touch-ups across 13 docs + 1 provenance banner; fix flagged stale claims against engine-reference.md, return claims ledger',
  phases: [{ title: 'Touchup', detail: 'one agent per file; fix only the flagged claims' }],
}

const REPO = '/home/aaron/Projects/ai/harness/agent-ways'
const STYLE = `${REPO}/docs/hooks-and-ways/authoring-docs-style.md`
const REF = `${REPO}/docs/hooks-and-ways/engine-reference.md`

const ANTI_ERROR = `
CRITICAL corrections (these exact mistakes were shipped and caught in prior passes — do NOT repeat):
- The g(s) calibration is fit at corpus-generation from the COMMITTED calibration_probes.jsonl
  (include_str!, ways-cli/src/cmd/corpus.rs:765). It is NOT fit from the runtime fire_score
  telemetry stream. fire_score feeds only the DEFERRED ADR-134 auto-tune. Never say the
  calibration/g(s) is fit from fire_score / events / telemetry.
- parent_threshold_multiplier (0.8) is LIVE, not retired. Parent-boost = (τ_s ×
  parent_threshold_multiplier).max(parent_boost_floor 0.30) = 0.40 by default. Both keys live.
- Telemetry event fields (source scan/mod.rs): way_nearmiss carries prob_en, prob_multi, tau_s,
  margin (NOT score_en/score_multi/thr_en/thr_multi). way_keyword_gated carries matched_span,
  prob_en, prob_multi, floor. way_fired carries fire_score. Use these EXACT names.
- events.jsonl lives at $XDG_STATE/agent-ways/events.jsonl (~/.claude/stats is legacy).
- There is NO per-way / per-locale / per-stub embed_threshold. Firing is global τ_s / τ_k on g(s).
- Keyword lane is floor-gated (fires only if g(s) ≥ τ_k) EXCEPT it fails open with no calibrated
  signal, and pattern_strict bypasses by design. "additive OR" needs this nuance.
- config keys: semantic_fire_probability (τ_s 0.5), keyword_floor_probability (τ_k 0.15),
  near_miss_margin (0.05). The default_embed_threshold / default_multi_embed_threshold keys are
  RETIRED (deprecation-mapped).
`

const FILES = [
  {
    "path": "hooks/ways/meta/knowledge/optimization/tuning/tuning.md",
    "mode": "touchup",
    "locators": [
      "L13-14: 'does not write thresholds \u2014 per ADR-125 thresholds are per-node (the English frontmatter), and stub quality is fixed by re-authoring, not by moving gates.'"
    ]
  },
  {
    "path": "docs/hooks-and-ways/ways-vs-rag.md",
    "mode": "touchup",
    "locators": [
      "L52: 'Ways have progressive disclosure trees: when a parent way fires, its children\\'s triggering thresholds drop by 20%.'"
    ]
  },
  {
    "path": "docs/explanation/how-ways-works/how-ways-works-the-model.md",
    "mode": "touchup",
    "locators": [
      "L114-118: 'When a way scores within a small margin of its effective threshold but doesn't clear it, the matcher records the would-be fire \u2014 its English and multilingual scores, both thresholds, and the margin by which it missed'",
      "L120: 'a near-miss right before a mistake is a threshold set too high.'",
      "L91-93: 'A way fires the first time its trigger matches: a keyword in the prompt, a semantic embedding score over threshold...'",
      "L176-178: 'The fire_score on a first-fire is the exact embedding score that cleared the threshold; the near-miss scores are the exact scores that didn't.'"
    ]
  },
  {
    "path": "docs/explanation/how-ways-works/reading-the-session-data.md",
    "mode": "touchup",
    "locators": [
      "L88: near_misses field '\u2014 with its English and multilingual scores, both thresholds, the margin, and the epoch it occurred in'",
      "L133-136: 'A near-miss with a tiny margin right before a mistake is a threshold set too high. Both feed the empirical tuning loop (ADR-134); `ways tune-curves` and `ways tune-precision` read the same log to suggest the adjustments.'"
    ]
  },
  {
    "path": "docs/explanation/how-ways-works/scenario-a-long-session.md",
    "mode": "touchup",
    "locators": [
      "L160-161: 'at epoch 26, `architecture/adr/migration` scored 0.4203 against its multilingual threshold of 0.44 \u2014 a margin of 0.0197.'",
      "L162: 'If that miss sat right before a migration mistake, it's evidence the threshold is a touch too high'"
    ]
  },
  {
    "path": "docs/hooks-and-ways/context-decay-formal-foundations.md",
    "mode": "touchup",
    "locators": [
      "L437: 'The once-per-session gating of ways serves as anti-windup: it prevents the injection system from accumulating redundant corrections...' \u2014 presents once-per-session gating as the live mechanism."
    ]
  },
  {
    "path": "docs/reference/model-context-decay/README.md",
    "mode": "touchup",
    "locators": [
      "L42: 'The current \"disclose once per session\" rule was designed for 200K context windows...' and L62-74 recommend a single global '25% of context window' re-disclosure interval.",
      "L82-89: 'The current epoch counter tracks events (hook firings)... Epoch distance drives check scoring (ADR-103). Token distance drives way re-disclosure.'"
    ]
  },
  {
    "path": "README.md",
    "mode": "touchup",
    "locators": [
      "L149: `embed_threshold: 0.35         # cosine similarity threshold (optional per-way tuning)` shown as a live frontmatter field in the canonical 'Creating Ways' example.",
      "L158 (and echoed L44/L277): \"Matching is **additive** \u2014 regex and semantic are OR'd. A way with both can fire from either channel.\""
    ]
  },
  {
    "path": "docs/hooks-and-ways/extending.md",
    "mode": "touchup",
    "locators": [
      "L22: \"Matching is additive \u2014 pattern and semantic are OR'd. A way can have both a `pattern:` and `description:` + `vocabulary:`; either channel can fire it.\""
    ]
  },
  {
    "path": "docs/cognitive-loop.md",
    "mode": "touchup",
    "locators": [
      "L95: example way frontmatter contains `embed_threshold: 0.55`.",
      "L135: \"`fire_score` on `way_fired` events ... This is the population a future `embed_threshold` raise would tune against.\"",
      "L140: mis-targeted remedy \"raise `embed_threshold`, narrow vocabulary, or change trigger channel\".",
      "L142: \"the threshold is read alongside `default_embed_threshold` and `default_multi_embed_threshold` in the ways config, which is also where `near_miss_margin` lives.\"",
      "L134: `way_nearmiss` fields \"`score_en`, `score_multi`, `thr_en`, `thr_multi`, `margin`\" framed as scoring within near_miss_margin of a per-way 'effective threshold'."
    ]
  },
  {
    "path": "docs/reference/ways-cli.md",
    "mode": "touchup",
    "locators": [
      "L123: `ways match` \u2014 \"The firing threshold is typically ~0.4\u20130.5 depending on the way's configuration.\"",
      "L92: `ways rethink --json` near-miss output described as \"each with its EN/multilingual scores, thresholds, and margin\" (also L123's implied per-way thresholds surface here)."
    ]
  },
  {
    "path": "docs/design-notes/session-introspection-implementation-plan.md",
    "mode": "banner",
    "locators": [
      "L61-62 'Present semantic fires as way-level cosine (`fire_score` >= `embed_threshold`), never a highlighted term.'"
    ]
  },
  {
    "path": "docs/hooks-and-ways/languages.md",
    "mode": "touchup",
    "locators": [
      "L89: 'Just lang, description, vocabulary \u2014 no per-stub threshold (per ADR-125, thresholds are per-node on the English frontmatter, not per locale).'",
      "L19: 'Both modes match by embedding cosine \u2014 the difference is the model, not the method.'"
    ]
  },
  {
    "path": "docs/hooks-and-ways/meta.md",
    "mode": "touchup",
    "locators": [
      "L15: knowledge way 'Covers: ... Matching modes (regex, semantic, model)'"
    ]
  }
]

const SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['file', 'rewritten_content', 'claims_ledger', 'change_summary', 'follow_ups'],
  properties: {
    file: { type: 'string' },
    rewritten_content: { type: 'string', description: 'COMPLETE new file content, ready to write verbatim' },
    claims_ledger: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['claim','source'],
      properties: { claim: { type: 'string' }, source: { type: 'string', description: 'file:line in source or engine-reference.md' } } } },
    change_summary: { type: 'array', items: { type: 'string' } },
    follow_ups: { type: 'array', items: { type: 'string' } },
  },
}

function touchupPrompt(f) {
  return `You are making SURGICAL single-claim corrections to one doc — fix ONLY the flagged stale claims,
leave everything else byte-for-byte identical. Do NOT rewrite or restructure the file.

FILE: ${REPO}/${f.path}

Read ${REF} (the source-cited engine truth) and ${STYLE} first, then the file.
${ANTI_ERROR}
FLAGGED STALE CLAIMS to fix in this file (these are LOCATORS — find each and correct it against
engine-reference.md / source; do NOT trust any remembered "correction", verify against the reference):
${f.locators.map((l, i) => `  ${i + 1}. ${l}`).join('\n')}

For each: replace the stale statement with the calibrated truth from engine-reference.md. Where the
claim is an example frontmatter with embed_threshold, DELETE that line. Where it states the fire
rule, state it correctly and link to engine-reference.md rather than re-deriving at length. For any
"verify the actual schema / JSON output" claim, use Read/Grep on the source to confirm the real
field names before writing. Keep the file's voice and structure; this is a touch-up, not a rewrite.

Return the COMPLETE file (with only the targeted fixes applied) and a claims_ledger of every engine
fact you touched with its source citation.`
}

function bannerPrompt(f) {
  return `Add a dated provenance banner to a timestamped implementation PLAN (style-guide §5). Do NOT
rewrite the body. FILE: ${REPO}/${f.path}. Read ${STYLE} §5 and ${REF} first.
This plan was written pre-ADR-156 and its body (L61-62) treats semantic fire as cosine ≥ embed_threshold
— the retired model. Prepend a short dated blockquote banner right after the H1: note it was written
before ADR-156 shipped, that semantic fire is now g(s) ≥ τ_s in probability space (no embed_threshold),
and point to ADR-156 + engine-reference.md. Preserve the ENTIRE body verbatim below. Return the full
file (banner + body). claims_ledger = the banner's facts with citations.`
}

phase('Touchup')
const results = await parallel(
  FILES.map((f) => () =>
    agent(f.mode === 'banner' ? bannerPrompt(f) : touchupPrompt(f),
      { label: `${f.mode}:${f.path.split('/').pop()}`, phase: 'Touchup', schema: SCHEMA, agentType: 'general-purpose' })
  )
)
const ok = results.filter(Boolean)
log(`${ok.length}/${FILES.length} returned; ${ok.reduce((n,r)=>n+(r.claims_ledger||[]).length,0)} claims to verify`)
return { count: ok.length, results: ok }
