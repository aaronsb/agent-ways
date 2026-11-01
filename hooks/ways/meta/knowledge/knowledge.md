---
description: Overview of the ways system — how ways, skills, and hooks relate, domain organization, matching modes
vocabulary: ways way knowledge guidance context inject hook trigger matching semantic vocabulary domain userpromptsubmit pretooluse re-injection project-local epoch
pattern: (^| )ways?( |$)|context.?inject
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
| Multi-file reference docs | Refire-gated, re-injects after its `refire:` fraction |
| | Dynamic context (macro queries API) |

The two complement each other: a skill cannot detect that a tool ran, and a way can. Ways match by regex and by meaning.

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

A way can be shown more than once in a session. Each agent keeps its own record of when a way fired (main at the session root, each subagent under `agents/<id>/`); after a fire the way waits out its `refire:` fraction of the context window, then becomes eligible to re-inject when it matches again. Showing a way again corrects drift in a long session. Several ways can fire on one prompt. A project way overrides a global way with the same name.

## Locations

Ways live in three roots (ADR-143). You can read ways in all three, and you write durable ways only in the last two:

- **Framework ways (read-only projection):** `~/.claude/hooks/ways/{domain}/{wayname}/{wayname}.md` — a symlink into the app source at `$XDG_DATA_HOME/agent-ways`. Readable here, but **don't author durably in it**: `ways manage update --commit` pulls the app dir with an autostash, so edits there conflict with upstream. Change these in a dev checkout and reproject (see `docs/development.md`).
- **Your own ways (user scope, survives updates):** `$XDG_CONFIG_HOME/agent-ways/ways/{domain}/{wayname}/{wayname}.md`
- **Project ways:** `$PROJECT/.claude/ways/{domain}/{wayname}/{wayname}.md` — override global ways at the same path
- Disable domains: `$XDG_CONFIG_HOME/agent-ways/config.yaml` → `disabled_domains: [domain]`
- Ways can nest: `{domain}/{parent}/{child}/{child}.md` for progressive disclosure
- When a parent way fires, its in-domain children become eligible on weaker signal: the child's semantic bar drops from `τ_s` to `(τ_s × parent_threshold_multiplier).max(parent_boost_floor)` = `max(0.5×0.8, 0.30) = 0.40` by default. The multiplier lowers the bar, and the floor stops it from dropping below 0.30.
- Tree disclosure metrics are tracked per agent (parent, depth, epoch distance, sibling coverage)
- Think strategies are ways that guide reasoning over several turns. The matcher detects them on its own, and the user can opt out.

## See Also

- knowledge/authoring(meta) — how to write and tune ways
- knowledge/optimization(meta) — vocabulary health and scoring calibration
- skills(meta) — skills complement ways with tool-specific bindings
