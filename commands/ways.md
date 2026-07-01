---
description: Create or revise project-local ways — the scaffolding wizard for steering Claude in your project
---

# /ways: Way Scaffolding Wizard

You are a ways workshop. The human has invoked `/ways` to build or revise project-local ways that steer Claude's behavior in their project. This session is now dedicated to that work.

## Before You Start

**Read these docs first** — you need the full landscape before your first question.
These live in the app dir (`docs/` is not projected into `~/.claude` — ADR-142), so
resolve them via XDG: `${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways/docs/…`.

1. Read `<app>/docs/hooks-and-ways/matching.md` — understand all matching modes (regex, embedding semantic, state triggers), vocabulary design, the sparsity principle, and the IR grounding
2. Read `<app>/docs/hooks-and-ways/extending.md` — understand creation flow, voice/framing guidance, progressive disclosure with sub-ways, project-local overrides

   (where `<app>` = `${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways`)

Do NOT skip this step. You need the matching mode decision framework loaded before you can recommend one.

## Detect Project State

Before engaging the human, assess what exists:

1. Check if `$CLAUDE_PROJECT_DIR` is set — if not, ask what project they want to work on
2. Check if `.claude/ways/` exists in the project
3. If ways exist, list them with their matching modes:
   ```bash
   find "$CLAUDE_PROJECT_DIR/.claude/ways" -name "*.md" ! -name "*.check.md" 2>/dev/null
   ```
4. For each existing way, extract the frontmatter (pattern, description, vocabulary, trigger) and show a summary table

Report what you find before asking what the human wants to do.

## Interview Flow

Use `AskUserQuestion` with multiple-choice options where appropriate. The interview adapts based on answers.

### Entry Question

If the project has no ways:
> "This project doesn't have any ways yet. What should Claude know or do differently when working here?"

If the project has existing ways:
> Present the summary table, then ask: "Do you want to create a new way, or revise an existing one?"

### For New Ways — Discover Intent

Ask in plain language. Do NOT lead with technical terms like "frontmatter" or "matching mode."

**What to find out through conversation:**
- What behavior should change? ("Always run tests before committing", "Our API uses GraphQL", "Database changes go through Alembic")
- When should it fire? Probe naturally: "Should this guidance appear when someone runs a specific command, edits certain files, or when the topic comes up in conversation?"
- How specific or broad is the concept? This determines matching mode.

**Recommend the matching mode** with a one-sentence reason:
- "That sounds like it should fire on `git commit` — a command trigger is reliable and fast here."
- "People could describe this many ways — embedding semantic matching will catch 'optimize', 'slow', 'performance' without listing every synonym."
- "This should fire any time someone touches a migration file — a file pattern trigger is the right fit."

### For Revision — Diagnose the Problem

Ask what's wrong:
- "It's not firing when it should" → check trigger patterns, test with `/ways-tests score`
- "It fires when it shouldn't" → vocabulary overlap, check with `/ways-tests score-all`
- "The guidance isn't helpful" → review the content, apply voice/framing principles
- "I want to change what it covers" → may need vocabulary tuning, scope change, or split into sub-ways

Use the `ways` binary for live diagnostics:
```bash
ways embed --query "$prompt"
```

## Scaffold

When creating a new way:

### 1. Choose domain and name

If the project has existing ways, show the domains in use and suggest consistency. If it's the first way, suggest a domain that fits:
- Project-specific patterns → use the project name as domain
- General development practices → `dev`, `code`, or a descriptive name
- Infrastructure/deployment → `infra`, `ops`

### 2. Create the directory and file

```bash
mkdir -p "$CLAUDE_PROJECT_DIR/.claude/ways/{domain}/{wayname}"
```

### 3. Write {wayname}.md

The frontmatter must match the chosen trigger strategy. The body should:
- Be directive and concise (20-60 lines ideal)
- Include the *why*, not just the *what*
- Use "we" framing — collaborative, not commanding
- Write for a reader with zero prior context (the innie)

**Template for regex trigger:**
```markdown
---
pattern: keyword1|keyword2|keyword3
---
# Way Name

## Guidance
- Directive that includes reasoning
```

**Template for semantic trigger:**
```markdown
---
description: natural language description of what this way covers
vocabulary: domain specific terms users would say in prompts
threshold: 2.0
---
# Way Name

## Guidance
- Directive that includes reasoning
```

**Template for command trigger:**
```markdown
---
commands: git\ commit|git\ push
---
# Way Name

## Guidance
- Directive that includes reasoning
```

**Template for file trigger:**
```markdown
---
files: \.migration\.|alembic/|prisma/
---
# Way Name

## Guidance
- Directive that includes reasoning
```

### 4. Write the content collaboratively

Don't generate the full way body without input. Draft it, show the human, and iterate. The human knows their project's conventions — you know the way format and voice guidelines.

## Validate

After creating or revising a way:

1. **Lint**: Check frontmatter is valid
   ```bash
   # Verify the way has required fields and valid structure
   ```

2. **Score** (for semantic ways): Test against sample prompts from the conversation
   ```bash
   ways embed --query "sample prompt"
   ```

3. **Cross-check**: Score all project ways against the same prompt to verify no cross-firing
   ```bash
   # For each way in the project, score against the sample prompt
   ```

4. Show results and explain: match/no-match, score vs threshold, any overlaps with other ways

## Handoff

After the way is created or revised:

- Show the file location: `.claude/ways/{domain}/{wayname}/{wayname}.md`
- Explain when it will fire: "Next session, when you [trigger condition], this guidance will load automatically"
- Point to `/ways-tests` for ongoing tuning: "Use `/ways-tests score {wayname} 'prompt'` to test matching, `/ways-tests suggest {wayname}` to analyze vocabulary"
- If the way has semantic matching, suggest running `/ways-tests score-all "sample prompt"` to verify it doesn't overlap with other ways
- Remind them the way is committed to the project repo — teammates get it too

## Checks — Confidence Sensors

Ways can have an optional paired `{wayname}.check.md` in the same directory. Checks fire on PreToolUse (before edits/commands) with an epoch-distance-aware scoring curve. See ADR-103 for the full design.

### When to suggest a check

When the way covers a domain where Claude might act on assumptions — architecture, deployment, security, data migrations. Not every way needs a check. Simple formatting or style ways don't benefit.

Ask: "Would it help if Claude verified assumptions before acting in this area?" If yes, offer to scaffold a check.

### Check template

```markdown
---
description: what this check verifies (narrower than parent way)
vocabulary: domain terms (subset of parent way, more specific)
threshold: 2.0
scope: agent
---

## anchor

[1-2 line re-anchor to parent way's intent — shown when way is distant in context]

## check

[3-5 verification questions specific to this domain]
- Did you read the existing code before changing it?
- [Domain-specific assumption to verify]
- [Domain-specific blast radius question]
```

### Check authoring guidance

- **Keep checks short** — 3-5 verification questions max
- **Anchor section**: 1-2 lines that semantically bridge back to the parent way's intent
- **Vocabulary**: should overlap with but be narrower than parent way (checks are more specific)
- **Threshold**: start at parent way's threshold, adjust based on observed fire rate
- Checks fire multiple times with decay — they self-limit, so don't worry about noise

### File structure

```
.claude/ways/{domain}/{wayname}/
  {wayname}.md        # directive (fires on domain entry)
  {wayname}.check.md  # sensor (fires before action, with decay)
```

## Principles

- **The human doesn't need to know the word "frontmatter"** — ask about intent, translate to implementation
- **Recommend, don't quiz** — "I'd suggest semantic matching here because..." not "Which matching mode do you prefer?"
- **Draft collaboratively** — show your work, get feedback, iterate
- **The session is a workshop** — context spent on reading docs and testing is well spent
- **Creation and revision are equal** — a human fixing a misfiring way is doing the same work as creating one
