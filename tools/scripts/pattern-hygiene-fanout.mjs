export const meta = {
  name: 'pattern-hygiene-sweep',
  description: 'ADR-155 §5: measure each flagged pattern keyword through the live ADR-156 calibration and propose remove/keep/anchor/bound (no file edits)',
  phases: [
    { title: 'Propose', detail: 'one proposer per flagged way file; measures probes through g(s)' },
  ],
}

// Session-specific absolute repo root (workflow scripts have no cwd/fs access to
// derive it). Edit this if replaying the methodology from another checkout.
const REPO = '/home/aaron/Projects/ai/harness/agent-ways'
// Findings manifest inlined (see scratchpad/findings-manifest.json) — avoids the
// fragile args-passing path that reached the script as a non-array.
const ways = [
  {"file":"hooks/ways/collaboration/onboarding-share/onboarding-share.md","id":"collaboration/onboarding-share","common":[],"short":[],"greedy":["share .*(guide|link)","bring .* up to speed","get .* started on (the|this) repo"],"needs_measure":false},
  {"file":"hooks/ways/data/documentation/documentation.md","id":"data/documentation","common":[],"short":["erd","dbml"],"greedy":["pg_dump.*(doc|diagram)"],"needs_measure":true},
  {"file":"hooks/ways/data/migrations/idempotent/idempotent.md","id":"data/migrations/idempotent","common":[],"short":[],"greedy":["drop .*if exists"],"needs_measure":false},
  {"file":"hooks/ways/documentation/diataxis/diataxis.md","id":"documentation/diataxis","common":[],"short":[],"greedy":["classif.*(doc|page)"],"needs_measure":false},
  {"file":"hooks/ways/documentation/documentation.md","id":"documentation","common":["documentation","docs"],"short":[],"greedy":["document.*(structure|model|graph|classif)"],"needs_measure":true},
  {"file":"hooks/ways/itops/incident/incident.md","id":"itops/incident","common":[],"short":["mttr"],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/meta/choices/choices.md","id":"meta/choices","common":[],"short":[],"greedy":["present.*(option|choice)","let.*decide","prefer.*(or|over)"],"needs_measure":false},
  {"file":"hooks/ways/meta/introspection/introspection.md","id":"meta/introspection","common":[],"short":[],"greedy":["create.*pr","pr.*create","write.*pr","open.*pr"],"needs_measure":false},
  {"file":"hooks/ways/meta/memory/memory.md","id":"meta/memory","common":["remember"],"short":[],"greedy":["save.*(to|this|that).*memory","note.*(for|this).*(later|next)","keep.*in.*mind"],"needs_measure":true},
  {"file":"hooks/ways/meta/skills/skills.md","id":"meta/skills","common":["skill"],"short":[],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/meta/subagents/subagents.md","id":"meta/subagents","common":[],"short":[],"greedy":["spawn.*agent","review.*pr","plan.*task","organiz.*docs"],"needs_measure":false},
  {"file":"hooks/ways/meta/think/think.md","id":"meta/think","common":[],"short":[],"greedy":["explore.*(option|approach|alternativ)","weigh.*(option|trade|alternativ)"],"needs_measure":false},
  {"file":"hooks/ways/meta/workflows/workflows.md","id":"meta/workflows","common":["workflow"],"short":[],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/code/errors/errors.md","id":"softwaredev/code/errors","common":["exception"],"short":[],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/code/performance/performance.md","id":"softwaredev/code/performance","common":["slow","performance"],"short":[],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/delivery/commits/commits.md","id":"softwaredev/delivery/commits","common":["commit"],"short":[],"greedy":["push.*(remote|origin|upstream)"],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/delivery/release/release.md","id":"softwaredev/delivery/release","common":["release"],"short":[],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/environment/deps/deps.md","id":"softwaredev/environment/deps","common":["package","library"],"short":[],"greedy":["upgrade.*version"],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/environment/ssh/ssh.md","id":"softwaredev/environment/ssh","common":[],"short":["ssh"],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/freshness/groundtruth/groundtruth.md","id":"softwaredev/freshness/groundtruth","common":["audit"],"short":[],"greedy":["is (this|the|that).*(up.?to.?date|still (true|accurate|current))"],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/tooling/tooling.md","id":"softwaredev/tooling","common":["tooling","automate"],"short":[],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/visualization/charts/charts.md","id":"softwaredev/visualization/charts","common":["chart","graph","plot"],"short":[],"greedy":[],"needs_measure":true},
  {"file":"hooks/ways/softwaredev/visualization/diagrams/diagrams.md","id":"softwaredev/visualization/diagrams","common":[],"short":[],"greedy":["sequence.*diagram","state.*diagram","er.*diagram","class.*diagram","visuali[sz]e.*architecture"],"needs_measure":false},
  {"file":"hooks/ways/softwaredev/visualization/visualization.md","id":"softwaredev/visualization","common":[],"short":[],"greedy":["walk.*through","explain.*how","show.*how","describe.*flow","how.*work","overview.*system"],"needs_measure":false},
  {"file":"hooks/ways/workstation/pkghistory/pkghistory.md","id":"workstation/pkghistory","common":[],"short":[],"greedy":["go over .*(stuff|things|packages|what).*(added|install)","(clean up|get rid of|trim|prune|cull).*(cruft|leftover|smelly|packages)","smelly .*leftover"],"needs_measure":false},
]

const SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['way_id', 'way_file', 'current_pattern', 'proposed_pattern', 'findings', 'vocabulary_additions', 'measured', 'notes'],
  properties: {
    way_id: { type: 'string' },
    way_file: { type: 'string' },
    current_pattern: { type: 'string', description: 'the verbatim current pattern: field value' },
    proposed_pattern: { type: 'string', description: 'the full rewritten pattern: field value (unchanged if no edit)' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['flagged', 'kind', 'verdict', 'replacement', 'evidence'],
        properties: {
          flagged: { type: 'string', description: 'the flagged alternation, verbatim from lint' },
          kind: { type: 'string', enum: ['common', 'short', 'greedy'] },
          verdict: { type: 'string', enum: ['remove', 'keep', 'anchor', 'bound'] },
          replacement: { type: 'string', description: 'new alternation text; empty string if removed' },
          evidence: { type: 'string', description: 'measured g_en values for intent/noise probes, or regex rationale' },
        },
      },
    },
    vocabulary_additions: {
      type: 'array', items: { type: 'string' },
      description: 'terms to append to the way vocabulary: field for any keyword removed from pattern:',
    },
    measured: { type: 'boolean', description: 'true if probe-measure.py was run for this way' },
    notes: { type: 'string' },
  },
}

function prompt(w) {
  const findingsBlock = [
    w.common.length ? `  COMMON-WORD (flagged as too generic for pattern:): ${JSON.stringify(w.common)}` : null,
    w.short.length ? `  SHORT-UNANCHORED (under 5 chars, no \\b): ${JSON.stringify(w.short)}` : null,
    w.greedy.length ? `  GREEDY-WILDCARD (unbounded .*): ${JSON.stringify(w.greedy)}` : null,
  ].filter(Boolean).join('\n')

  return `You are a pattern-hygiene proposer for the agent-ways project (ADR-155 §5, on the ADR-156
calibrated ruler). You OWN one way file. PROPOSE fixes for its flagged pattern alternations,
backed by MEASUREMENT. Do NOT edit any file — return a structured proposal only.

WAY: ${w.id}
FILE: ${REPO}/${w.file}

LINT FINDINGS for this way:
${findingsBlock}

BACKGROUND — the matching engine (ADR-156). A prompt fires a way when, for the way's embedded
alias text (= "{description} {vocabulary}"), the calibrated probability g(s)=σ(a·s+b) of the
cosine s clears a threshold:
    fire ⇔ g(s) ≥ τ_s   ∨   ( keyword_matches ∧ g(s) ≥ τ_k )
Global thresholds: τ_s = 0.5 (semantic lane; EN cos ≥ 0.293) and τ_k = 0.15 (keyword floor; EN
cos ≥ 0.228). The keyword lane ONLY fires when the semantic score already clears the low floor
τ_k — so a keyword can NEVER drag in a prompt the embedding finds totally unrelated. There is NO
per-way threshold knob anymore (embed_threshold was removed). Remedies are vocabulary/pattern
edits only: remove / keep / anchor / bound.

STEP 1 — read the file. Read ${REPO}/${w.file} and extract the CURRENT \`pattern:\` field value
verbatim (it's a regex with top-level | alternations; may start with (?i)). Also note the
\`vocabulary:\` field.

STEP 2 — for each COMMON-WORD and SHORT finding, MEASURE. Write 8–12 INTENT probes (realistic
prompts from a user who genuinely wants THIS way — varied phrasings, some that DON'T contain the
flagged word so you can see whether the semantic lane alone catches the intent) and 8–12 NOISE
probes (prompts that CONTAIN the flagged word but where the user does NOT want this way — the
word used in an unrelated sense). Then run the shared instrument:

    cd ${REPO} && python3 tools/scripts/probe-measure.py '${w.id}' \\
      --intent "probe one" "probe two" ... \\
      --noise "noise one" "noise two" ...

It prints per-probe cosine, g_en, whether it fires the semantic lane (g≥τ_s) and clears the floor
(g≥τ_k), plus a separation summary. Read the numbers. Decide per keyword:
  • REMOVE (from pattern:, ADD to vocabulary:) if EITHER
      (a) SEMANTIC-SAFE: the genuine intent already fires the semantic lane on its own — most
          INTENT probes (incl. ones lacking the word) reach g ≥ τ_s. The keyword is redundant.
      (b) INSEPARABLE: intent and noise g-distributions overlap so much the keyword can't
          discriminate (noise leaks past τ_k about as often as intent clears it, or the summary
          says distributions overlap and noise median ≈ intent median). No pattern can save it;
          e.g. meta/memory 'remember'. Removing stops the false fires; the semantic lane keeps
          the true ones.
  • KEEP (in pattern:, unchanged) if the keyword is LOAD-BEARING and CLEAN: genuine intent needs
      the floor (INTENT clears τ_k but often NOT τ_s, so semantic-alone would miss it) AND the
      NOISE is gated (noise g mostly < τ_k — the floor already suppresses the generic-word
      misfires). The calibration is doing its job; the lint flag is a false alarm here. (This is
      the likely verdict for softwaredev/code/performance 'slow' — verify by measuring.)
  • ANCHOR (\\bTERM\\b) for SHORT terms-of-art (erd, dbml, mttr, ssh): keep the keyword but wrap
      in word boundaries so it can't match inside larger words. Measure to confirm the anchored
      term still fires on real intent and the substring-collisions disappear.

STEP 3 — for each GREEDY-WILDCARD finding, the default remedy is BOUND: replace the unbounded
\`.*\` with a bounded, proximity-preserving quantifier (\`.{0,30}\` is a good default; tune 10–40
to the intended phrase gap) or restructure to explicit tokens (e.g. \`push\\s+\\S+\\s+(remote|
origin|upstream)\` → but simplest is bounding). Preserve the ORIGINAL matching intent — the fix
is about killing the unbounded span, not changing what phrases match. If a bounded pattern would
STILL be a generic-phrase leak (e.g. visualization 'how.*work', 'walk.*through' are near-universal
phrasings), MEASURE it like a common word and consider REMOVE if the semantic lane covers the
intent. Otherwise bound and move on.

STEP 4 — assemble the full rewritten \`pattern:\` value: apply every verdict (drop removed
alternations, swap anchored/bounded ones in place, keep the rest). Preserve the leading (?i) if
present and the overall alternation structure. For every keyword you REMOVE, add it (or a better
semantic phrasing of it) to vocabulary_additions so the term still contributes to the embedding.

Return the structured proposal. evidence must cite the actual measured g_en numbers (e.g.
"intent median g=0.71, noise median g=0.04, noise 0% past floor → keep" or "intent g 0.2–0.3 all
below τ_s but clear τ_k; noise g<0.05 → keep, floor-protected"). Be decisive; when genuinely
uncertain, prefer KEEP for load-bearing keywords and BOUND for wildcards, and say so in notes.`
}

phase('Propose')
const proposals = await parallel(
  ways.map((w) => () =>
    agent(prompt(w), {
      label: `propose:${w.id}`,
      phase: 'Propose',
      schema: SCHEMA,
      agentType: 'general-purpose',
    })
  )
)

const ok = proposals.filter(Boolean)
log(`${ok.length}/${ways.length} proposals returned`)

// Roll up a compact decision ledger for central review.
const ledger = ok.map((p) => ({
  way: p.way_id,
  measured: p.measured,
  verdicts: (p.findings || []).map((f) => `${f.flagged} → ${f.verdict}`),
  pattern_changed: p.current_pattern !== p.proposed_pattern,
  vocab_adds: p.vocabulary_additions || [],
}))

return { count: ok.length, of: ways.length, proposals: ok, ledger }
