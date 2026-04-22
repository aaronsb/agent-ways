---
description: Overview of the ways system — how ways, skills, and hooks relate, domain organization, matching modes
vocabulary: ways way knowledge guidance context inject hook trigger matching semantic vocabulary domain
pattern: (^| )ways?( |$)|knowledge|guidance|context.?inject
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Knowledge Way

## Ways vs Skills

**Skills** = semantically-discovered (Claude decides based on intent)
**Ways** = triggered (patterns, commands, file edits, or state conditions)

| Use Skills for | Use Ways for |
|---------------|--------------|
| Semantic discovery ("explain code") | Tool-triggered (`git commit` → format reminder) |
| Tool restrictions (`allowed-tools`) | File-triggered (edit `.env` → config guidance) |
| Multi-file reference docs | Session-gated (once per session) |
| | Dynamic context (macro queries API) |

They complement: Skills can't detect tool execution. Ways support both regex and semantic matching.

## How Ways Work

Ways are contextual guidance that loads once per session when triggered by:
- **Keywords** in user prompts (UserPromptSubmit)
- **Tool use** - commands, file paths (PreToolUse)
- **State conditions** - context threshold, file existence (UserPromptSubmit)

## State Machine

```
(not_shown)-[:TRIGGER {keyword|command|file|state}]->(shown)  // output + create marker
(shown)-[:TRIGGER]->(shown)  // no-op, idempotent
```

Each (way, session) pair has its own marker. Multiple ways can fire per prompt. Project-local wins over global for same name.

## Locations

- Global: `~/.claude/hooks/ways/{domain}/{wayname}/{wayname}.md`
- Project: `$PROJECT/.claude/ways/{domain}/{wayname}/{wayname}.md`
- Disable domains: `~/.claude/ways.json` → `{"disabled": ["domain"]}`
- Ways can nest: `{domain}/{parent}/{child}/{child}.md` for progressive disclosure
- When a parent way fires, child thresholds are lowered 20% (domain context is established)
- Tree disclosure metrics are tracked per-session (parent, depth, epoch distance, sibling coverage)
- Think strategies are multi-turn ways that steer reasoning across several turns (auto-detected, opt-out)

## See Also

- knowledge/authoring(meta) — how to write and tune ways
- knowledge/optimization(meta) — vocabulary health and scoring calibration
- skills(meta) — skills complement ways with tool-specific bindings
