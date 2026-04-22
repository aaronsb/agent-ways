---
description: Structured investigation — exploring topics, comparing options, synthesizing findings, evaluating sources
vocabulary: research investigate explore find out compare evaluate analyze synthesize sources evidence survey landscape assess discover understand learn about look into dig into alternatives options
macro: append
scope: agent
requires: ["Bash(grep:*)"]
refire: 0.15
---
<!-- epistemic: heuristic -->
# Research Way

## Define the Question

Before searching, state what you're trying to learn and why. A vague "research X" becomes "I need to understand X to decide between Y and Z." The user may not have articulated this — help them sharpen it.

## Investigation Structure

1. **Scope** — What's in bounds? What would be a tangent?
2. **Gather** — Use tools (WebSearch, WebFetch, Grep, Read) to collect information. Prefer primary sources over summaries of summaries.
3. **Evaluate** — Not all sources are equal. Official docs > blog posts > forum answers > LLM-generated content. Flag confidence levels.
4. **Synthesize** — Compress findings into a structure the user can act on. Don't dump raw results.
5. **Present** — Lead with the answer, then the evidence. The user wants the conclusion first.

## Comparative Analysis

When comparing options (tools, approaches, vendors, libraries):

| Column | Purpose |
|--------|---------|
| **Option** | What's being compared |
| **Strengths** | What it does well |
| **Weaknesses** | Where it falls short |
| **Fits when** | The scenario where this option wins |
| **Avoid when** | The scenario where it's wrong |

End with a recommendation. Don't just present a neutral table — defend a choice based on the user's context.

## Source Discipline

- Distinguish what you know (training data) from what you found (tool results) from what you inferred.
- When citing web results, include the URL so the user can verify.
- If you can't find reliable information, say so. "I couldn't find authoritative sources on X" is more useful than hedging.
- Stale information is worse than no information. Check dates on sources.

## When Research is Overkill

Not every question needs a structured investigation. If the answer is in the codebase, read the code. If it's a factual recall question, just answer it. This way is for genuine unknowns that require gathering and synthesis.

## See Also

- architecture/design(softwaredev) — research feeds design decisions
- think(meta) — structured thinking strategies for investigation
