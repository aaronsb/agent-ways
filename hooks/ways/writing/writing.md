---
description: Content creation — documents, presentations, reports, proposals, editing, and structured writing beyond code documentation
vocabulary: write draft compose author proposal report presentation deck slides memo brief narrative outline revise edit polish tone audience prose style
macro: append
scope: agent
requires: ["Bash(grep:*)"]
refire: 0.15
---
<!-- epistemic: heuristic -->
# Writing Way

## Scope

General content creation — proposals, reports, presentations, memos, briefs. For code documentation (README, docstrings, API docs), see the Docs Way.

## Before Writing

1. **Audience** — who reads this and what do they need to walk away with?
2. **Format** — match the medium to the message. A slide deck argues differently than a report.
3. **Core message** — every document has one. Find it before outlining.
4. **Style** — ask the user about voice, tone, and register. Don't assume.

## Structure

Outline before drafting. Defend the structure to the user.

| Content type | Structure |
|---|---|
| Proposal / pitch | Problem → solution → evidence → ask |
| Technical report | Summary → findings → analysis → recommendations |
| Status update | Progress → blockers → next steps |
| Presentation | Hook → tension → resolution → takeaway |
| Decision doc | Context → options → recommendation → consequences |

## Prose Style

Write prose, not bullet points wearing a trenchcoat. Let ideas develop across sentences. Vary sentence length and rhythm — short sentences create urgency, longer ones let complex ideas breathe.

**Defaults (override per user preference):**
- Prefer active voice. Use passive only when the actor is irrelevant or unknown.
- Use em dashes sparingly — they lose force through overuse.
- Avoid staccato sentence fragments for rhetorical impact. Let the substance carry the weight.
- Lead with the conclusion when the audience is busy. Lead with context when the audience needs persuading.
- Cut filler on revision. "In order to" → "To". "It should be noted that" → delete.

**Ask the user** about style preferences early. Use literary terms when discussing: register (formal/informal), diction (word choice), syntax (sentence structure), cadence (rhythm), tone (attitude toward subject). These terms are precise and help the user articulate what they want.

**Avoid:**
- Corporate filler — hollow superlatives, engagement bait, breathless enthusiasm disconnected from substance.
- Constructivism framing — "it's not X, it's Y" patterns that define by negation. Say what it is.
- Adjective stacking. One precise adjective beats three vague ones.

## Revision

Revise in passes, not all at once:
1. **Structure** — is the argument in the right order?
2. **Clarity** — can each sentence be misread? Fix ambiguity.
3. **Tone** — does it match the audience and purpose?
4. **Concision** — what can be cut without losing meaning?

## Pushback

If the format doesn't fit the content (a slide deck for something that needs a detailed report), push back. Explain why format matters and suggest an alternative. Preserve the user's intent — they want to communicate something, the format is negotiable.

## See Also

- docs(softwaredev) — code documentation is a specialized form of writing
- trust/voice(meta) — voice and tone in written communication
