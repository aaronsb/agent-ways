export const meta = {
  name: 'docs-enact-pr1',
  description: 'Enact the calibrated-model rewrite of the 3 authoring/tuning surfaces (authoring.md, optimization.md, ways-tests SKILL) against the committed style guide; return full rewritten content (no direct edits)',
  phases: [{ title: 'Rewrite', detail: 'one agent per authoring/tuning file' }],
}

const REPO = '/home/aaron/Projects/ai/harness/agent-ways'
const STYLE = `${REPO}/docs/hooks-and-ways/authoring-docs-style.md`

const FILES = [
  {
    path: 'hooks/ways/meta/knowledge/authoring/authoring.md',
    target_mode: 'how-to (way body — keep the frontmatter + prose structure)',
    remove: [
      "L57 frontmatter-catalog row: `embed_threshold:` — DELETE the field entry entirely.",
      "L30 'Matching is additive — pattern OR semantic ... Use both ... guaranteed activation on specific terms' — the keyword lane is FLOOR-GATED, not an unconditional OR. Rewrite: fire iff g(s) >= tau_s OR (keyword_match AND g(s) >= tau_k). Guaranteed/unconditional keyword firing requires pattern_strict: true (bypasses the gate).",
      "L149 'Child ways get a threshold boost (config.parent_threshold_multiplier, default 0.8)' — KEEP the multiplier; state the full mechanism: effective child bar = (τ_s × parent_threshold_multiplier).max(parent_boost_floor) = max(0.5×0.8, 0.30) = 0.40 by default, in probability space (ADR-156). Both keys are live; the multiplier is the boost, the floor caps the cascade.",
      "L151 whole 'Embedding thresholds' paragraph (embed_threshold, default 0.35, child sets 0.45 to avoid cross-firing) — DELETE. Replace the cross-firing remedy with: sharpen the child's vocabulary/pattern (add discriminating terms, tighten the pattern) and verify with tools/scripts/probe-measure.py; thresholds are global, not per-way.",
      "L211 'the node's English embed_threshold governs all aliases' — locale stubs correctly carry no threshold, but drop the embed_threshold reference; there is no per-node threshold now, firing is the global calibrated gate.",
    ],
    add: [
      "pattern_strict: true — unconditional keyword fire that bypasses the gate (this is the ONLY way to guarantee a keyword fires).",
      "pattern_keep: <words> — frontmatter list exempting a MEASURED common-word keep from the pattern-hygiene lint (ADR-155 §5): keyword load-bearing, off-sense noise floor-gated by tau_k.",
      "The remedy loop: mis-fire -> measure with tools/scripts/probe-measure.py -> edit vocabulary/pattern -> re-measure. Never move a threshold.",
    ],
    preserve: "refire/ADR-126 cadence section, vocabulary-isolation Jaccard guidance, tree token budgets, locale-stub coordinate-alias model (minus the embed_threshold line), `ways tune`/`ways siblings` usage. Do NOT split the frontmatter catalog out to a new file in this pass (note it as a follow-up).",
  },
  {
    path: 'hooks/ways/meta/knowledge/optimization/optimization.md',
    target_mode: 'how-to (way body)',
    remove: [
      "L49 sharpening bullet 'Raise the threshold on the less-specific way' — replace with: remedy overlap by editing vocabulary/pattern (add discriminating terms, remove shared generic terms, anchor/bound alternations) and measure through the calibration (tools/scripts/probe-measure.py).",
      "L56-62 the whole 'Thresholds' reference section (embed_threshold, config.default_embed_threshold 0.35, parent_threshold_multiplier 0.8, 'lowering the base threshold') — rewrite to the calibrated model: cosine -> g(s)=sigma(a*s+b) -> probability; global tau_s=0.5 / tau_k=0.15; parent-boost = (tau_s × parent_threshold_multiplier 0.8).max(parent_boost_floor 0.30) = 0.40 (both keys live). Recall/precision is shaped by vocabulary/pattern content measured through the calibration, not by moving a per-way threshold. The 0-FP hard constraint still holds as a test invariant.",
      "L44 'one way scores well above threshold, others score well below' — reword: one way clears the fire probability (g(s) >= tau_s) while others fall below it.",
    ],
    add: ["A short pointer to pattern_keep and probe-measure.py as the measurement path for the remedy loop."],
    preserve: "the suggest/interpret/apply workflow, sparsity/discrimination principle, 'which ways use semantic matching' note, health indicators, and the ENTIRE `ways tune` locale-audit section (current). Do NOT consolidate locale-audit into tuning.md in this pass (note as a follow-up).",
  },
  {
    path: 'skills/ways-tests/SKILL.md',
    target_mode: 'reference (skill) — score interpretation recentred on probability',
    remove: [
      "L40-44 Engine para 'semantic scoring is the sole retrieval tier ... fires when cosine >= embed_threshold (per-way, default 0.35)' — replace with the TWO-LANE model: keyword regex `pattern:` lane + semantic lane where cosine is mapped through g(s)=sigma(a*s+b) (ADR-156, embed-manifest.json) to a probability; fire iff g(s) >= tau_s(0.5) OR (keyword AND g(s) >= tau_k(0.15)); thresholds global not per-way.",
      "L52-54 '`ways embed` takes no --threshold (that's the per-way embed_threshold field)' — there is no embed_threshold field; relevance is the global probability gate after calibration; a keyword lane exists via pattern:.",
      "L63-68 sample output 'Thr' column showing 0.35 + per-way YES/no — recentre the decision on calibrated g(s) vs global tau_s (and keyword tau_k).",
      "L79-86 'Interpreting cosine scores' table (0.35-0.5 weak / <0.35 no match) — recentre on calibrated probability g(s) vs tau_s=0.5; raw cosine is the INPUT to g(s), not a fire cutoff.",
      "L87-88 'Raise/lower a way's embed_threshold' remedy — replace with vocab/pattern edit measured via tools/scripts/probe-measure.py; for a load-bearing common word use pattern_keep (noise floor-gated by tau_k).",
      "L88-90 'child's effective threshold is x0.8 when an ancestor fired' — the x0.8 is CORRECT (parent_threshold_multiplier); state it fully as (τ_s × 0.8).max(parent_boost_floor 0.30) = 0.40, now in probability space (ADR-156). Progressive disclosure still valid.",
      "L123 / L127-131 tree-health 'threshold progression / per-level thresholds / threshold inversion / flat thresholds' — there are NO per-level thresholds; tree health = vocabulary/pattern isolation, orphans, token budget, depth/breadth; disclosure governed by parent_boost_floor + per-level specificity.",
      "L199-200 'threshold is a second lever' — the two levers are VOCABULARY and PATTERN (regex keyword lane), both measured through the calibration; no per-way threshold lever.",
      "L204 Notes 'embed_threshold (default 0.35) is the per-way cutoff' — the cutoffs are the global tau_s=0.5 / tau_k=0.15 probabilities.",
    ],
    add: [
      "This skill currently has NO mention of the keyword lane, pattern:, pattern_strict, pattern_keep, or the calibration — that is a COVERAGE GAP the rewrite must fill, not just a correction. Add the two-lane model and the new knobs.",
    ],
    preserve: "Suggest/GAPS, Jaccard/siblings, Crowding, Budget, intentional co-fire authoring — vocabulary/structure analyses independent of the threshold model. `ways tune` (locale fidelity/discrimination) is current — keep.",
  },
]

const SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['file', 'rewritten_content', 'change_summary', 'preserved', 'new_concepts_added', 'follow_ups'],
  properties: {
    file: { type: 'string' },
    rewritten_content: { type: 'string', description: 'the COMPLETE new file content, ready to write verbatim (including frontmatter for way bodies / skill headers)' },
    change_summary: { type: 'array', items: { type: 'string' }, description: 'what changed, one line each' },
    preserved: { type: 'array', items: { type: 'string' }, description: 'sections kept intact' },
    new_concepts_added: { type: 'array', items: { type: 'string' } },
    follow_ups: { type: 'array', items: { type: 'string' }, description: 'structural changes deliberately deferred (e.g. splitting the frontmatter catalog)' },
  },
}

function prompt(f) {
  return `You are enacting the calibrated-model documentation rewrite (ADR-156) for ONE authoring/tuning
surface. Produce the COMPLETE rewritten file. This is a real edit that will ship — be precise,
preserve the good content, and match the surrounding voice.

FILE: ${REPO}/${f.path}
TARGET MODE: ${f.target_mode}

STEP 1 — read the STYLE GUIDE first: ${STYLE}
It is the source of truth: the current-model paragraph, the retired-vocabulary replacement table,
the concepts docs must now teach, the Diátaxis rule, and the voice. Everything you write must
conform to it.

STEP 2 — read the current file: ${REPO}/${f.path}

STEP 3 — apply these SPECIFIC changes (measured by the discovery pass):

REMOVE / REPLACE:
${f.remove.map((r) => `  - ${r}`).join('\n')}

ADD (currently missing, load-bearing):
${f.add.map((a) => `  - ${a}`).join('\n')}

PRESERVE (do not disturb): ${f.preserve}

RULES:
- This is a WAY BODY or SKILL — keep its frontmatter/header EXACTLY valid (do not touch
  description/vocabulary/pattern/scope/refire frontmatter unless a field is itself the stale item;
  none here are). If it's a way body, the <!-- epistemic: ... --> marker and heading structure stay.
- Sober, literal voice (this text reads as doctrine, not chat). No hype.
- Do NOT invent behavior. If you're unsure whether a mechanism is current, prefer the style guide's
  "do not disturb" list and leave it. When you reference the remedy path, it is
  tools/scripts/probe-measure.py.
- Keep the file's existing good structure; you are correcting and filling a gap, not writing from
  scratch. Every preserved section should survive verbatim or lightly edited.

Return the COMPLETE rewritten file content in rewritten_content (ready to write verbatim), plus the
change_summary / preserved / new_concepts_added / follow_ups metadata.`
}

phase('Rewrite')
const results = await parallel(
  FILES.map((f) => () =>
    agent(prompt(f), { label: `enact:${f.path.split('/').pop()}`, phase: 'Rewrite', schema: SCHEMA, agentType: 'general-purpose' })
  )
)
const ok = results.filter(Boolean)
log(`${ok.length}/${FILES.length} rewrites returned`)
return { count: ok.length, results: ok }
