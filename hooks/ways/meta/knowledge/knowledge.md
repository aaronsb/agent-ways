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
| Multi-file reference docs | Session-gated, re-injects on a decay curve |
| | Dynamic context (macro queries API) |

They complement: Skills can't detect tool execution. Ways support both regex and semantic matching.

## How Ways Work

Ways are contextual guidance that discloses when triggered by:
- **Keywords** in user prompts (UserPromptSubmit)
- **Tool use** - commands, file paths (PreToolUse)
- **State conditions** - context threshold, file existence (UserPromptSubmit)

## State Machine

```
(not_shown)-[:TRIGGER {keyword|command|file|state|embed}]->(shown)  // output + stamp epoch
(shown)-[:TRIGGER, suppressed]->(shown)                             // hold — re-disclosure threshold not met
(shown)-[:TRIGGER, threshold met]->(shown)                          // re-inject to course-correct
```

Disclosure isn't once-and-done. Each (way, session) pair stamps the epoch it fired; a per-way decay curve lowers its suppression threshold as the session grows (context size, epoch distance), so a way that fired early becomes eligible to re-inject when it matches again. Re-disclosure course-corrects drift over long sessions — it isn't verbatim repetition. Multiple ways can fire per prompt. Project-local wins over global for same name.

## Locations

Three way roots (ADR-143) — don't confuse *reading* one with *authoring* in it:

- **Framework ways (read-only projection):** `~/.claude/hooks/ways/{domain}/{wayname}/{wayname}.md`
  — a symlink into the app source at `$XDG_DATA_HOME/agent-ways`. Readable here, but
  **don't author durably in it**: the app dir is replaced wholesale on update, so edits are
  lost. Change these in a dev checkout and reproject (see `docs/development.md`).
- **Your own ways (user scope, survives updates):** `$XDG_CONFIG_HOME/agent-ways/ways/{domain}/{wayname}/{wayname}.md`
- **Project ways:** `$PROJECT/.claude/ways/{domain}/{wayname}/{wayname}.md` — override global ways at the same path
- Disable domains: `$XDG_CONFIG_HOME/agent-ways/config.yaml` → `disabled_domains: [domain]` (legacy `$XDG_CONFIG_HOME/ways/config.yaml` and `~/.claude/ways.json` → `{"disabled": [...]}` still honored)
- Ways can nest: `{domain}/{parent}/{child}/{child}.md` for progressive disclosure
- When a parent way fires, child thresholds are lowered 20% (domain context is established)
- Tree disclosure metrics are tracked per-session (parent, depth, epoch distance, sibling coverage)
- Think strategies are multi-turn ways that steer reasoning across several turns (auto-detected, opt-out)

## See Also

- knowledge/authoring(meta) — how to write and tune ways
- knowledge/optimization(meta) — vocabulary health and scoring calibration
- skills(meta) — skills complement ways with tool-specific bindings
