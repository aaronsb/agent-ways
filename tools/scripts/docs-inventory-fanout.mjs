export const meta = {
  name: 'docs-inventory',
  description: 'Read-only inventory of the ways documentation + authoring/tuning surface: per file, detect staleness vs the ADR-156 calibrated model, assign Diátaxis mode + disposition (keep/rewrite/discard/merge)',
  phases: [{ title: 'Inventory', detail: 'one reader per surface cluster; no edits' }],
}

const REPO = '/home/aaron/Projects/ai/agent-ways'

// ── The CURRENT model the docs must reflect (staleness = deviation from this) ──
const TRUTH = `
CURRENT MATCHING MODEL (the source of truth the docs must reflect):
- Two lanes. KEYWORD lane = regex \`pattern:\` field. SEMANTIC lane = embedding
  cosine s = cos(prompt, way_alias) where alias = "{description} {vocabulary}".
- ADR-156 CALIBRATION: cosine is mapped to a relevance PROBABILITY via a per-model
  logistic g(s)=σ(a·s+b), fit at corpus-generation from a committed probe corpus,
  stored in embed-manifest.json (EN AUC ~0.955, multi ~0.941). Cosine is NOT
  thresholded directly anymore.
- FIRE RULE (probability space): fire ⇔ g(s) ≥ τ_s  ∨  ( keyword_match ∧ g(s) ≥ τ_k ).
  Global thresholds only: τ_s = semantic_fire_probability = 0.5, τ_k =
  keyword_floor_probability = 0.15. τ_s and τ_k are INDEPENDENT. The keyword lane
  only fires when the semantic score already clears the low floor τ_k — a keyword
  can never drag in a totally-unrelated prompt.
- pattern_strict: true bypasses the gate (unconditional keyword fire).
- pattern_keep: <words> — frontmatter list exempting measured common-word keeps from
  the pattern-hygiene lint (ADR-155 §5); the keyword is load-bearing and its noise is
  floor-gated.

RETIRED / STALE — flag any doc that still teaches these as live:
- \`embed_threshold:\` frontmatter field (raw-cosine per-way threshold) — REMOVED.
- \`config.default_embed_threshold\` (0.35 / 0.40) and \`default_multi_embed_threshold\`.
- \`keyword_gate_fraction\` (γ) and the "keyword fires at γ·T" gate — REPLACED by τ_k.
- "raise/lower the threshold on a way", per-way threshold tuning as a remedy — GONE.
  The remedy for a mis-firing way is now a VOCABULARY or PATTERN edit (add/remove
  discriminating terms; remove/anchor/bound alternations) measured through the
  calibration (tools/scripts/probe-measure.py), NOT moving a threshold.
- parent-boost: BOTH \`parent_threshold_multiplier\` (0.8) and \`parent_boost_floor\`
  (0.30) are LIVE. Effective child bar = (τ_s × 0.8).max(0.30) = 0.40 by default
  (scan/mod.rs effective_thresholds). ADR-156 moved this to probability space (the
  operand is τ_s, not a raw per-way cosine threshold); it did NOT remove the multiplier.
  Flag any doc that says the multiplier was retired/replaced.

STILL CURRENT (do NOT flag these as stale):
- \`ways tune\` = LOCALE alias audit (fidelity/discrimination vs the English root
  anchor), ADR-139/125. It explicitly does NOT write thresholds. Correct as-is.
- Salience/signal DECAY: turn-based exponential decay (ADR-121) — a separate model
  from relevance scoring; still valid.
- Progressive disclosure, token-gated re-fire (ADR-104/105/126), three-root runtime
  (ADR-143), sentence-salience input reduction (ADR-130), authored disclosure graph /
  removal of BM25 (ADR-125), two embedding models EN + multilingual (localized mode).
- ADRs themselves are immutable decision records — an ADR describing the pre-156 world
  is correct history, NOT stale. Only flag NON-ADR docs that assert stale behavior as
  current.
`

const CLUSTERS = [
  { name: 'authoring-tuning-ways', priority: 'HIGH — user-flagged', files: [
    'hooks/ways/meta/knowledge/knowledge.md',
    'hooks/ways/meta/knowledge/authoring/authoring.md',
    'hooks/ways/meta/knowledge/authoring/pii-free/pii-free.md',
    'hooks/ways/meta/knowledge/authoring/tool-agnostic/tool-agnostic.md',
    'hooks/ways/meta/knowledge/optimization/optimization.md',
    'hooks/ways/meta/knowledge/optimization/tuning/tuning.md',
  ]},
  { name: 'ways-skills', priority: 'HIGH — user-flagged', files: [
    'skills/ways-tests/SKILL.md',
    'skills/ways-settings/SKILL.md',
    'skills/ways-localize/SKILL.md',
    'skills/ways-update/SKILL.md',
    'skills/docs/SKILL.md',
  ]},
  { name: 'engine-matching-docs', priority: 'HIGH', files: [
    'docs/hooks-and-ways/matching.md',
    'docs/hooks-and-ways/scoring-and-testing.md',
    'docs/hooks-and-ways/stats.md',
    'docs/hooks-and-ways/rationale.md',
    'docs/hooks-and-ways/observed-behavior.md',
    'docs/hooks-and-ways/ways-vs-rag.md',
  ]},
  { name: 'explanation-how-ways-works', priority: 'HIGH', files: [
    'docs/explanation/how-ways-works/how-ways-works-the-model.md',
    'docs/explanation/how-ways-works/reading-the-session-data.md',
    'docs/explanation/how-ways-works/scenario-a-long-session.md',
    'docs/explanation/attend-messaging/07-the-lane-gate.md',
  ]},
  { name: 'signal-decay-modeling', priority: 'MED', files: [
    'docs/hooks-and-ways/context-decay.md',
    'docs/hooks-and-ways/context-decay-formal-foundations.md',
    'docs/reference/model-context-decay/README.md',
    'docs/signal-analysis/README.md',
  ]},
  { name: 'toplevel-reference-vocab', priority: 'HIGH', files: [
    'README.md',
    'docs/hooks-and-ways.md',
    'docs/hooks-and-ways/README.md',
    'docs/hooks-and-ways/extending.md',
    'docs/vocabulary.md',
    'docs/cognitive-loop.md',
    'docs/reference/ways-cli.md',
  ]},
  { name: 'design-notes', priority: 'MED', files: [
    'docs/design-notes/lexical-gate-as-conditional-threshold.md',
    'docs/design-notes/cognitive-loop-and-awareness-layer.md',
    'docs/design-notes/adopter-localization-lifecycle-and-tuning.md',
    'docs/design-notes/session-introspection-implementation-plan.md',
  ]},
  { name: 'hooks-and-ways-stragglers', priority: 'LOW', files: [
    'docs/hooks-and-ways/languages.md',
    'docs/hooks-and-ways/macros.md',
    'docs/hooks-and-ways/provenance.md',
    'docs/hooks-and-ways/meta.md',
    'docs/hooks-and-ways/teams.md',
    'docs/hooks-and-ways/itops.md',
  ]},
]

const SCHEMA = {
  type: 'object', additionalProperties: false,
  required: ['cluster', 'entries'],
  properties: {
    cluster: { type: 'string' },
    entries: { type: 'array', items: {
      type: 'object', additionalProperties: false,
      required: ['file', 'what', 'diataxis_mode', 'staleness', 'disposition', 'target_mode', 'priority', 'notes'],
      properties: {
        file: { type: 'string' },
        what: { type: 'string', description: 'one line: what this doc is and what it asserts' },
        diataxis_mode: { type: 'string', enum: ['tutorial', 'how-to', 'reference', 'explanation', 'mixed', 'none'] },
        staleness: { type: 'array', description: 'specific stale/incorrect claims found; empty if clean', items: {
          type: 'object', additionalProperties: false,
          required: ['claim', 'why_stale', 'correct'],
          properties: {
            claim: { type: 'string', description: 'the stale text/claim, quoted or paraphrased, with line if known' },
            why_stale: { type: 'string' },
            correct: { type: 'string', description: 'what it should say under the current model' },
          },
        }},
        disposition: { type: 'string', enum: ['keep', 'rewrite', 'discard', 'merge'] },
        merge_into: { type: 'string', description: 'target file if disposition=merge, else empty' },
        target_mode: { type: 'string', enum: ['tutorial', 'how-to', 'reference', 'explanation', 'unchanged'] },
        priority: { type: 'string', enum: ['high', 'med', 'low'] },
        notes: { type: 'string' },
      },
    }},
  },
}

function prompt(c) {
  return `You are a documentation-inventory reader for the agent-ways project. This is the
PREREQUISITE discovery pass for a documentation overhaul (task #8): produce a precise,
per-file inventory. READ-ONLY — do NOT edit any file.

YOUR CLUSTER: ${c.name} (priority: ${c.priority})
FILES (absolute):
${c.files.map((f) => `  ${REPO}/${f}`).join('\n')}

${TRUTH}

For EACH file: Read it fully. Then record a structured entry:
- what: one line — what the doc is and what it asserts.
- diataxis_mode: which Diátaxis mode it CURRENTLY reads as (tutorial = learning-by-doing;
  how-to = task recipe; reference = information lookup; explanation = understanding/why;
  mixed = serves >1 mode and should split; none = not a Diátaxis doc, e.g. a way body or
  skill — still assess staleness).
- staleness: EVERY concrete claim that deviates from the CURRENT model above. Quote the
  stale text (with line number if you can), say why it's stale, and give the correct
  statement. Be specific — "mentions embed_threshold at L58 as the per-way tuning knob"
  not "seems outdated". If the file is clean, empty array. Do NOT invent staleness;
  ADR history and the still-current list above are NOT stale.
- disposition: keep (accurate, maybe minor touch-ups) | rewrite (stale core, restructure
  to the calibrated model) | discard (obsolete, superseded, delete) | merge (fold into
  another doc — name it in merge_into).
- target_mode: the Diátaxis mode it SHOULD be after rework (or unchanged).
- priority: high (authoring/tuning surface, or asserts the matching model wrongly — a
  reader would be actively misled) | med | low.
- notes: anything the synthesis needs — overlaps with other files, a good section worth
  preserving, a structural suggestion.

Special attention for this cluster: ${c.name.includes('authoring') || c.name.includes('skills')
  ? 'This is the AUTHORING/TUNING surface the user explicitly wants aligned. Scrutinize every claim about HOW to author or tune a way — thresholds, vocabulary, patterns, the remedy for a mis-firing way. The doctrine must teach the calibrated model (vocab/pattern edits + pattern_keep + probe measurement), not per-way threshold tuning.'
  : 'Flag any assertion of the matching/scoring model that contradicts the current model above.'}

Return the structured inventory for your cluster. Ground every staleness finding in text
you actually read.`
}

phase('Inventory')
const results = await parallel(
  CLUSTERS.map((c) => () =>
    agent(prompt(c), { label: `inv:${c.name}`, phase: 'Inventory', schema: SCHEMA, agentType: 'general-purpose' })
  )
)

const ok = results.filter(Boolean)
const entries = ok.flatMap((r) => r.entries || [])
const stale = entries.filter((e) => (e.staleness || []).length > 0)
log(`${ok.length}/${CLUSTERS.length} clusters; ${entries.length} files inventoried; ${stale.length} with staleness`)

const byDisp = {}
for (const e of entries) byDisp[e.disposition] = (byDisp[e.disposition] || 0) + 1
log(`dispositions: ${JSON.stringify(byDisp)}`)

return { clusters: ok.length, files: entries.length, dispositions: byDisp, entries }
