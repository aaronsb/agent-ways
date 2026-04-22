---
description: Claude Code skills — SKILL.md format, creation, discovery, slash commands, frontmatter
vocabulary: skill slash command SKILL.md create author invoke user-invocable plugin
pattern: skill|SKILL\.md|skill.?(creation|author|write)|claude.?code.?skill|~\/\.claude\/skills
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Skills Way

## What is a Skill?

A markdown file that teaches Claude how to do something specific. Claude automatically applies Skills when your request matches their description.

## SKILL.md Structure

```yaml
---
name: explaining-code
description: Explains code with visual diagrams and analogies. Use when explaining how code works or when asked "how does this work?"
allowed-tools: Read, Grep, Glob
model: claude-sonnet-4-20250514
---

# Instructions

1. Start with an analogy
2. Draw ASCII diagram
3. Walk through step-by-step
```

## Frontmatter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Lowercase, hyphenated, max 64 chars, matches directory |
| `description` | Yes | What + when (max 1024 chars) - Claude uses this to decide |
| `allowed-tools` | No | Tools Claude can use without permission |
| `model` | No | Override model for this Skill |

## Where Skills Live

| Location | Path | Scope |
|----------|------|-------|
| Personal | `~/.claude/skills/` | All your projects |
| Project | `.claude/skills/` | Team (in repo) |
| Plugin | Bundled in plugin | Plugin users |
| Enterprise | Managed settings | Organization |

Priority: Enterprise > Personal > Project > Plugin

## Writing Good Descriptions

The description is how Claude decides when to use your Skill.

**Bad**: "Helps with documents"
**Good**: "Extract text and tables from PDF files, fill forms, merge documents. Use when working with PDF files or when the user mentions PDFs, forms, or document extraction."

Include:
1. What the Skill does (specific actions)
2. When to use it (trigger keywords users would say)

## Progressive Disclosure

Keep `SKILL.md` under 500 lines. Link to supporting files:

```
my-skill/
├── SKILL.md        # Overview (required)
├── reference.md    # Detailed docs (loaded when needed)
├── examples.md     # Usage examples
└── scripts/
    └── helper.py   # Executed, not loaded into context
```

## Skills vs Other Options

| Use | When | Activation |
|-----|------|------------|
| **Skills** | Specialized knowledge | Claude chooses automatically |
| **Slash commands** | Reusable prompts | You type `/command` |
| **CLAUDE.md** | Project-wide rules | Every conversation |
| **Ways** | Tool/file-triggered guidance | Hook events |

## Testing

```
# Restart Claude Code after creating
claude

# Ask Claude what's available
"What Skills are available?"

# Test with matching request
"How does this code work?"
```
