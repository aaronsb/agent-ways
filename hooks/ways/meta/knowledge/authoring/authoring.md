---
files: (^|/)(\.claude/ways|hooks/ways|agent-ways/ways)/.*\.md$
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
refire: 0.15              # firing cadence; see knowledge/authoring/refire(meta)
scope: agent
---
```

Regex-only matching (`pattern:` without `description:`/`vocabulary:`) will miss any phrasing you didn't predict. Users don't say "code.?quality" — they say "clean this up" or "this function is a mess." Semantic matching with a good description and vocabulary handles the variation. Regex doesn't.

If you still want regex-only, that's your choice, but expect poor recall on natural language prompts.

**When to add `pattern:` alongside semantic:** As a supplementary trigger for exact terms you never want to miss on the strong-signal path. The keyword lane is floor-gated by the semantic signal rather than an unconditional OR, and it is reserved for specific, term-of-art triggers; suggestive or common words belong in `vocabulary:`. The fire rule, `pattern_strict:`, `pattern_keep:`, and the pattern-hygiene lint are in knowledge/authoring/keyword-lane(meta).

**Other trigger types** (not prompt-based, semantic doesn't apply):
- `files:` — regex matched against file paths (Edit/Write hooks)
- `commands:` — regex matched against bash commands
- `trigger:` — state-based (context-threshold, file-exists, session-start)

**All values must be single-line.** Do not use YAML folded (`>`) or literal (`|`) scalars — the trigger pipeline parsers only read the first line, silently returning `>` as the value. Use `ways author lint` to catch this.

For state-based triggers:
```markdown
---
trigger: context-threshold
threshold: 90             # percentage (0-100)
---
```

### Frontmatter Fields

The full field reference (pattern, semantic, state-based, `when:` preconditions, `macro:`, `scope:`) is in knowledge/authoring/frontmatter(meta). `ways author lint` validates every field against `frontmatter-schema.yaml`.

## Creating a New Way

Use `ways author template` to scaffold the way file in one step:

```bash
# Project-local (default)
ways author template softwaredev/code/newway \
  --description "what this way covers" \
  --vocabulary "domain keywords users would say"

# Your own ways, every project ($XDG_CONFIG_HOME/agent-ways/ways/)
ways author template meta/newway \
  --description "what this way covers" \
  --global
```

This creates:
- `{wayname}/{wayname}.md` — frontmatter + body template with guidance placeholders

Ways are authored **English-only** (ADR-139): localization is adopter-run, not authored per-way — there is no translation step here. Then: `ways author lint <path>`, `ways corpus`, and `ways author match "<prompt>"`. A fire-bearing way needs `refire:`; without it the firing gate refuses the way, so it never reaches the agent (only a Task dispatch to a subagent bypasses the gate).

**Manual creation** also works: create `{domain}/{wayname}/{wayname}.md` with frontmatter + guidance. No config files to update. A project way overrides your own way with the same path, and yours overrides the shipped one. Ways can nest arbitrarily: `{domain}/{parent}/{child}/{child}.md`.

## Writing Ways Well

Write as a collaborator, not an authority. Include the *why* — an agent that understands the reason applies better judgment at the edges. Write for a reader with no prior context.

For state transitions and process flows, prefer Cypher-style notation over ASCII diagrams — it's compact, the model parses it natively, and it saves tokens:
```
(state_a)-[:EVENT {context}]->(state_b)  // what happens
```

## Progressive Disclosure Trees

When a way covers multiple distinct concerns (>80 lines, >2 sub-topics, language/tool-specific variants), decompose it into a tree of parent and child ways (ADR-105). A way whose delivered body is over the 10,000-character hook context cap must be split: `ways author lint` reports it as an error. The parent boost, vocabulary isolation, token budgets, and anti-rationalization tables are in knowledge/authoring/trees(meta).

## Testing Your Way

Use the `ways` CLI and `/ways-tests` to validate matching quality. **Use the built-in tools — do not write ad-hoc scripts** for scoring, Jaccard, or vocabulary analysis.

- `ways corpus` — rebuild the corpus after editing `description` or `vocabulary`, so scores reflect the edit
- `ways author match "sample prompt"` — the live late-interaction matcher (ADR-160): peak, share, body-confirm, and whether each candidate would fire
- `ways author lint <path>` — validate frontmatter and the delivered-size cap
- `ways author suggest <way-file>` — analyze vocabulary gaps
- `ways author siblings <way-id>` — way-vs-way cosine similarity, to find confusers
- `/ways-tests score <way> "sample prompt"`, `/ways-tests score-all "sample prompt"` — the skill's scoring views

For vocabulary tuning workflows, see the optimization sub-way (triggers on vocabulary/optimization discussion).

Full authoring guide: `docs/hooks-and-ways/extending.md`

## Locale Stubs

Native-language matching aliases live in `{wayname}.locales.jsonl` beside the way file; the way body stays English. The format and audit are in knowledge/authoring/locale-stubs(meta).

## See Also

- knowledge/authoring/frontmatter(meta) — every frontmatter field and what it does
- knowledge/authoring/keyword-lane(meta) — the `pattern:` lane, its floor gate, and pattern hygiene
- knowledge/authoring/refire(meta) — firing cadence forms and presets
- knowledge/authoring/trees(meta) — progressive disclosure trees and anti-rationalization tables
- knowledge/authoring/locale-stubs(meta) — per-language matching aliases
- knowledge/authoring/tool-agnostic(meta) — ways describe intent, not tool calls
- knowledge/authoring/pii-free(meta) — privacy constraint on way content
- knowledge/optimization(meta) — vocabulary tuning, sparsity, discrimination
