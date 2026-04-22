---
files: \.claude/ways/.*\.md$
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Authoring Ways

## Way File Format

Each way lives in `{domain}/{wayname}/{wayname}.md` with YAML frontmatter.

## Matching Strategy

**Use semantic matching.** This is the primary matching strategy for prompt-triggered ways. The engine uses embeddings (cosine similarity) — this is the sole retrieval tier (per ADR-125).

```markdown
---
description: what this way covers, in natural language
vocabulary: domain specific keywords users would say
embed_threshold: 0.35     # cosine similarity threshold (optional, per-way tuning)
refire: 0.15              # firing cadence; see "Firing cadence" section below
scope: agent
---
```

Regex-only matching (`pattern:` without `description:`/`vocabulary:`) will miss any phrasing you didn't predict. Users don't say "code.?quality" — they say "clean this up" or "this function is a mess." Semantic matching with a good description and vocabulary handles the variation. Regex doesn't.

If you still want regex-only, that's your choice, but expect poor recall on natural language prompts.

**When to add `pattern:` alongside semantic:** As a supplementary trigger for exact matches you never want to miss. Matching is additive — pattern OR semantic, either fires the way. Use both when you need guaranteed activation on specific terms AND broad natural language coverage.

**Other trigger types** (not prompt-based, semantic doesn't apply):
- `files:` — regex matched against file paths (Edit/Write hooks)
- `commands:` — regex matched against bash commands
- `trigger:` — state-based (context-threshold, file-exists, session-start)

**All values must be single-line.** Do not use YAML folded (`>`) or literal (`|`) scalars — the trigger pipeline parsers only read the first line, silently returning `>` as the value. Use `ways lint` to catch this.

For state-based triggers:
```markdown
---
trigger: context-threshold
threshold: 90             # percentage (0-100)
---
```

### Frontmatter Fields

**Pattern-based:**
- `pattern:` - Regex matched against user prompts
- `files:` - Regex matched against file paths (Edit/Write)
- `commands:` - Regex matched against bash commands

**Semantic:**
- `description:` - Natural language reference text for what this way covers
- `vocabulary:` - Space-separated domain keywords users would say
- `embed_threshold:` - Cosine similarity threshold (optional, per-way tuning; default applied if absent)
- Engine: embedding-only (per ADR-125). Explicit `pattern:` / `commands:` regex still fire independently.

**State-based:**
- `trigger:` - State condition type (`context-threshold`, `file-exists`, `session-start`)
- `threshold:` - For context-threshold: percentage (0-100)
- `path:` - For file-exists: glob pattern relative to project

**Preconditions (`when:` block):**
- `when:` - Deterministic gate checked before any matching. If unmet, way is skipped entirely.
  - `project:` - Only fire in this project directory (e.g., `~/.claude`). Path is resolved for comparison.

```yaml
when:
  project: ~/.claude    # only fire when working in claude-code-config
```

Ways without a `when:` block fire everywhere (the default). Use `when:` sparingly — only for self-referential ways that are meaningless outside their home project.

**Firing cadence (`refire:`):**

Fire-bearing ways (ways with description + vocabulary that participate in semantic matching) should carry a `refire:` field. This controls re-disclosure — how quickly the way becomes eligible to fire again after a fire. Per ADR-126 the value is a fraction of the session's context window, resolved at fire time against the model's actual window (so way files stay portable across model generations and frameworks).

Two forms are accepted:

```yaml
refire: 0.15         # direct: half-life = 15% of session window
```
```yaml
refire: normal       # preset: resolved via config.refire_presets
```

- **Numeric form** (`0.0 – 1.0+`) pins the cadence to today's model. Use when you want precise control or when the intent is model-specific.
- **Preset form** (string name) looks up the project's `refire_presets` config section. Built-in defaults: `once` (1.0), `rare` (0.4), `normal` (0.15), `frequent` (0.05). Use for portability — re-tuning happens globally via one config edit.

Common choices (numeric ↔ preset, matching the built-in defaults):

| Intent | Numeric | Preset |
|---|---|---|
| Static-heavy payloads (heuristic tables, long checklists) | `0.4` | `rare` |
| Load-bearing guidance (typical case, ~3 fires per session) | `0.15` | `normal` |
| Procedural event handlers (fires often relative to session) | `0.05` | `frequent` |
| Disclose once per session | `1.0` | `once` |

Numeric values between these presets are fine — for example, the 14 ways migrated from ADR-127's 1M-Opus hack sit at `refire: 0.2` (between `normal` and `rare`), deliberately pinned to today's model.

Missing `refire:` on a fire-bearing way means the way fires once and never re-discloses — valid but uncommon, and `ways lint` warns on it. Check files and `trigger: attend` handlers are exempt (checks ride on parent way firing; attend handlers are signal-triggered).

The legacy `curve:` block (ADR-123) is no longer part of the schema. Writing `curve:` in new ways will trigger a lint UNKNOWN/foreign-field warning.

**Other:**
- `macro:` - `prepend` or `append` to run `macro.sh` for dynamic context
- `scope:` - `agent`, `subagent`, `teammate` (comma-separated, default: agent)

## Creating a New Way

Use `ways template` to scaffold the way file and locale stubs in one step:

```bash
# Project-local (default)
ways template softwaredev/code/newway \
  --description "what this way covers" \
  --vocabulary "domain keywords users would say"

# Global
ways template meta/newway \
  --description "what this way covers" \
  --global
```

This creates:
- `{wayname}/{wayname}.md` — frontmatter + body template with guidance placeholders
- `{wayname}/{wayname}.locales.jsonl` — locale entries for all covered languages (needs translation)

Then: translate locale descriptions, run `ways corpus && ways tune --apply`, and `ways lint`.

**Manual creation** also works: create `{domain}/{wayname}/{wayname}.md` with frontmatter + guidance. No config files to update. Project ways override global ways with the same path. Ways can nest arbitrarily: `{domain}/{parent}/{child}/{child}.md`.

## Writing Ways Well

Write as a collaborator, not an authority. Include the *why* — an agent that understands the reason applies better judgment at the edges. Write for a reader with no prior context.

For state transitions and process flows, prefer Cypher-style notation over ASCII diagrams — it's compact, the model parses it natively, and it saves tokens:
```
(state_a)-[:EVENT {context}]->(state_b)  // what happens
```

## Progressive Disclosure Trees

When a way covers multiple distinct concerns (>80 lines, >2 sub-topics, language/tool-specific variants), decompose into a tree. The supply chain tree (`softwaredev/code/supplychain/`) is the reference implementation.

**How disclosure works now** (ADR-125): ways are nodes in a DAG. When a parent fires, a session marker is set. Child ways get a threshold boost (`config.parent_threshold_multiplier`, default 0.8) whenever any ancestor has a marker — so children fire more easily once their domain is active. This is the mechanism behind "progressive disclosure": children are always candidates, but the boost makes in-domain children fire on weaker signal. Full model in [hooks-and-ways/matching.md](../../../../docs/hooks-and-ways/matching.md).

**Embedding thresholds** — only English frontmatter carries `embed_threshold:`. Broader ways at the root typically leave it unset (use the default 0.35); more specific children may set a higher value (e.g., 0.45) to avoid cross-firing with the root. Locale stubs don't carry thresholds.

**Vocabulary isolation** — sibling ways MUST NOT share vocabulary:
- Target Jaccard similarity < 0.15 between siblings
- Each child owns its own keyword space
- Use `ways siblings <path>` to verify; use `ways tune --way <path>` to surface cross-way confusers in multilingual space

**Token awareness** — aim for:
- Realistic path (root→leaf): ~1200 tokens
- Worst case (all fire): ~4000 tokens
- Use `/ways-tests budget <tree>` to measure

**When NOT to tree**: Leave flat if <80 lines, single cohesive concern, or all content is needed together.

## Anti-Rationalization Patterns

For high-stakes ways where the agent is tempted to skip steps (testing, security, supply chain), add a "Common Rationalizations" table:

```markdown
## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "This is simple, tests aren't needed" | If it's simple, the test is trivial. Write it. |
| "I'll add tests later" | Later never comes. Tests verify understanding NOW. |
```

**Placement**: In the specific leaf/mid-tier node, not the root. The table should only appear when the agent is actively doing the thing it might skip.

**Tone**: Direct, not preachy. State the fact. 5-7 rows max.

## Testing Your Way

Use `/ways-tests` and the `ways` CLI to validate matching quality. **Use the built-in tools — do not write ad-hoc scripts** for scoring, Jaccard, or vocabulary analysis.

- `/ways-tests score <way> "sample prompt"` — test a specific way
- `/ways-tests score-all "sample prompt"` — rank all ways against a prompt
- `/ways-tests suggest <way>` — analyze vocabulary gaps
- `/ways-tests lint <way>` — validate frontmatter
- `ways siblings <path>` — vocabulary overlap between siblings (Jaccard)
- `way-embed match --corpus ... --query "..."` — embedding similarity scores

**Tree validation**:
- `/ways-tests tree <path>` — structural analysis (depth, breadth, threshold progression)
- `/ways-tests budget <path>` — token cost per way, per path, worst-case
- `/ways-tests crowding "prompt"` — vocabulary overlap detection
- `/ways-tests metrics` — session disclosure tracking (after live use)

For vocabulary tuning workflows, see the optimization sub-way (triggers on vocabulary/optimization discussion).

Full authoring guide: `docs/hooks-and-ways/extending.md`

## Locale Stubs

Ways can have native-language matching stubs stored in `{wayname}.locales.jsonl` alongside the way file. These are **coordinate aliases** on the way's graph node (ADR-125): one line per language with `description` and `vocabulary` in the target language. The way body stays English. Every alias must carry the objective match words of the way's intent in local form — translations must actually translate, not just share a name.

```jsonl
{"lang":"ja","description":"セキュリティ脆弱性スキャン","vocabulary":"セキュリティ 脆弱性 CVE"}
```

No per-locale threshold field: the node's English `embed_threshold` governs all aliases.

**Audit your stubs** with `ways tune` — it measures fidelity (do sibling translations agree?) and discrimination (does another way's alias outrank yours?). Entries where a non-sibling confuser wins need the stub re-authored with sharper vocabulary. See `knowledge/optimization/tuning(meta)` for the full workflow and failure-mode categories. Full guide: `docs/hooks-and-ways/languages.md`.

## See Also

- knowledge/authoring/tool-agnostic(meta) — ways describe intent, not tool calls
- knowledge/authoring/pii-free(meta) — privacy constraint on way content
- knowledge/optimization(meta) — vocabulary tuning, sparsity, discrimination
- knowledge/optimization/tuning(meta) — locale alias audit, failure modes, re-authoring guidance
