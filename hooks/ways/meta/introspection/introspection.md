---
description: PR creation as a reflection point — pause and consider what was learned this session
vocabulary: pull request create pr open pr ship merge review reflect session learning introspection
pattern: pull.?request|create.*pr|pr.*create|write.*pr|open.*pr
commands: gh\ pr\ create
macro: prepend
scope: agent
requires: ["Bash(jq:*)", "Bash(ways:*)"]
refire: 0.15
---
<!-- epistemic: heuristic -->
# Introspection Way

A pull request is a natural boundary of work — a moment to pause and reflect before closing the loop. Regardless of what the PR contains (code, config, docs, process), this is the right time to ask: did we learn something this session that should become a way?

## The Surprise Test

Intelligence is managing surprise. If this session went as expected — no corrections, no unexpected behavior, no "actually we do it this way" moments — then there's nothing to capture and you should skip this entirely. A routine session that followed existing patterns doesn't need introspection. Move on and create the PR.

The threshold is surprise: something the next session would also get wrong without guidance.

## Two-Part Flow

If something *did* surprise, this splits between you (the main agent) and a subagent. You hold the session history — only you can identify what the human taught. The subagent gets a fresh context window to review existing ways and draft proposals without burning your remaining tokens.

### Part 1: You Summarize (main agent)

Before creating the PR, look back through this conversation for moments where the human:
- **Corrected** something — "No, we do it this way..."
- **Explained** a convention — "The reason we X is because Y..."
- **Guided** a choice — "We prefer A over B here because..."
- **Pushed back** — "That's not how this project works..."
- **Repeated** a preference — if they said it twice, it's a pattern

If none of these happened, say "Nothing surprising this session" and proceed with the PR. No subagent, no proposals, no ceremony.

If something did stand out, compile a concise summary. For each signal, capture:
- **What** the human said or corrected
- **Why** they said it (if they gave a reason)
- **When** it would apply again (what kind of work would hit this)

### Part 2: Subagent Reviews Ways and Proposes

Spawn a subagent (`subagent_type: "general-purpose"`) with:
1. Your summary of human signals from Part 1
2. The project path so it can find `$PROJECT/.claude/ways/`
3. Instructions to follow the review process below

**Subagent prompt template:**

> Review project-local ways and propose new ones based on session learnings.
>
> **Project path:** [path]
>
> **Session signals from human:**
> [your summary from Part 1]
>
> **Your tasks:**
>
> 1. **Enumerate existing project-local ways** — list what's in `$PROJECT/.claude/ways/`. Note if none exist.
>
> 2. **Check for overlap** — do any existing ways already cover the signals above? If so, note whether they need updating or are sufficient.
>
> 3. **Propose new ways** for uncovered signals. For each proposal, specify:
>    - File path: `$PROJECT/.claude/ways/{domain}/{topic}/{topic}.md`
>    - Trigger type and pattern (keyword, command, or file pattern)
>    - A draft of the way content in collaborative voice
>
> 4. **Skip anything that's:**
>    - A one-off decision that won't recur
>    - Already covered by an existing way (global or project-local)
>    - So specific it applies to exactly one file
>
> Follow the Knowledge Way format: YAML frontmatter with match/pattern/files/commands, then concise guidance written as a collaborator, not a directive. Place ways in project scope.
>
> Return: a summary of existing ways, and any proposed new ways with their full content. Do NOT create the files — just return the proposals.

### Part 3: Present to the Human

Take the subagent's proposals and present them. Don't silently create ways.

> "During this session, you [corrected/explained/guided] me about [X]. I had a subagent review our project ways — here's what it found and proposes:
>
> **Existing ways:** [list or "none yet"]
>
> **Proposed new ways:**
> - `project/domain/topic/{topic}.md` — triggered by [pattern], covering [what]
> - ...
>
> Want me to create any of these?"

Let the human decide what's worth keeping. Their judgment about what's a real convention vs. a one-time choice is better than yours.

## Why This Matters

Every session starts cold. The agent that arrives next has no memory of corrections made today. If a convention lives only in the conversation history, it dies when the session ends. Ways are how we carry forward what the human teaches us.
