---
description: PR creation as a reflection point — pause and consider what was learned this session
vocabulary: pull request create pr open pr ship merge review reflect session learning introspection
pattern: (?i)pull.?request|create.{0,20}\bpr\b|\bpr\b.{0,20}create|write.{0,20}\bpr\b|open.{0,20}\bpr\b
commands: gh\ pr\ create
macro: prepend
scope: agent
requires: ["Bash(jq:*)", "Bash(ways:*)"]
refire: 0.15
---
<!-- epistemic: heuristic -->
# Introspection Way

A pull request is a natural boundary of work — a moment to pause and reflect before closing the loop. Regardless of what the PR contains (code, config, docs, process), this is the right time to ask: did we learn something this session that should become a **way, a skill, or a workflow**?

## The Surprise Test

Intelligence is managing surprise. If this session went as expected — no corrections, no unexpected behavior, no "actually we do it this way" moments — then there's nothing to capture and you should skip this entirely. A routine session that followed existing patterns doesn't need introspection. Move on and create the PR.

The threshold is surprise: something the next session would also get wrong without guidance.

## Two-Part Flow

If something *did* surprise, this splits between you (the main agent) and a subagent. You hold the session history — only you can identify what the human taught. The subagent gets a fresh context window to review existing ways, skills, and workflows and draft proposals without burning your remaining tokens.

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

### Part 2: Subagent Reviews Existing Artifacts and Proposes

Spawn a subagent (`subagent_type: "general-purpose"`) with:
1. Your summary of human signals from Part 1
2. The project path so it can find `$PROJECT/.claude/ways/`
3. Instructions to follow the review process below

**Subagent prompt template:**

> Review project-local ways, skills, and workflows and propose new artifacts based on session learnings.
>
> **Project path:** [path]
>
> **Session signals from human:**
> [your summary from Part 1]
>
> **Your tasks:**
>
> 1. **Enumerate existing project-local artifacts** — ways (`$PROJECT/.claude/ways/`), skills (`$PROJECT/.claude/skills/`), and workflows (`$PROJECT/.claude/workflows/`). Note what's absent.
>
> 2. **Check for overlap** — do any existing ways already cover the signals above? If so, note whether they need updating or are sufficient.
>
> 3. **Propose new artifacts** for uncovered signals, each classified to the right type (ADR-138):
>    - **way** — recurring behavior/context (the 5W), disclosed just-in-time. Path `$PROJECT/.claude/ways/{domain}/{topic}/{topic}.md`; give trigger type + pattern.
>    - **skill** — a concrete procedure to *run* on demand. Path `$PROJECT/.claude/skills/{name}/SKILL.md`; give a tight description + a "Not for" lane.
>    - **workflow** — deterministic multi-agent orchestration (fan-out / verify / synthesize). Path `$PROJECT/.claude/workflows/{name}`; describe the stages.
>
>    Rule of thumb: recurring *guidance* → way; a repeated *procedure* → skill; a multi-stage *orchestration* → workflow. Draft each in collaborative voice.
>
> 4. **Skip anything that's:**
>    - A one-off decision that won't recur
>    - Already covered by an existing way (global or project-local)
>    - So specific it applies to exactly one file
>
> Follow the Knowledge Way format for ways (YAML frontmatter + collaborative guidance) and the Skills Way for skills (tight description, scoped allowed-tools, a "Not for" lane). Place all artifacts in project scope.
>
> Return: a summary of existing ways, and any proposed new ways with their full content. Do NOT create the files — just return the proposals.

### Part 3: Present to the Human

Take the subagent's proposals and present them. Don't silently create artifacts.

> "During this session, you [corrected/explained/guided] me about [X]. I had a subagent review our project ways, skills, and workflows — here's what it found and proposes:
>
> **Existing artifacts:** [list or "none yet"]
>
> **Proposed additions (ways / skills / workflows):**
> - [way] `project/domain/topic/{topic}.md` — triggered by [pattern], covering [what]
> - [skill] `skills/{name}/SKILL.md` — invoked for [task]; not for [lane]
> - ...
>
> Want me to create any of these?"

Let the human decide what's worth keeping. Their judgment about what's a real convention vs. a one-time choice is better than yours.

## Why This Matters

Every session starts cold. The agent that arrives next has no memory of corrections made today. If a learning lives only in the conversation history, it dies when the session ends. Ways, skills, and workflows are how we carry forward what the human teaches us.
