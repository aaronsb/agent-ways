---
description: writing and editing prose for people — a report, proposal, memo, status or progress update, presentation, or announcement, and the conventions each genre follows
vocabulary: write writing draft compose write up proposal report presentation deck slides memo status update progress update announcement outline revise edit rewrite polish tone audience prose style genre diction corporate gloss revision genre-specific filler
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
4. **Style** — the output style sets register (ADR-178). Ask about voice or tone only under the asking bar in core.md.

## Structure

Outline before drafting. State the structure in the reply.

| Content type | Structure |
|---|---|
| Proposal / pitch | Problem → solution → evidence → ask |
| Technical report | Summary → findings → analysis → recommendations |
| Status update | Progress → blockers → next steps |
| Presentation | Hook → tension → resolution → takeaway |
| Decision doc | Context → options → recommendation → consequences |

## Genre

Where a genre's needs and a general style rule disagree, the genre wins.

| Genre | Prioritize | Avoid |
|---|---|---|
| Technical doc | task success, exact terms, prerequisites, observable behavior, examples, failure modes, recovery | flourish in a procedure; varying a canonical term for style; softening mandatory wording |
| ADR | problem, constraints, alternatives considered, decision, trade-offs, interfaces, migration, verification | dressing a preference as an inevitability; blurring "we chose" with "the system requires" |
| Brief for an agent | the task, the done criteria, the bounds, what to read first, the shape of the report back | pasting context the agent can read from disk; leaving done undefined |
| Proposal | problem, proposed change, rationale, scope, cost where known, trade-offs, risks, success criteria | overselling; omitting limitations |
| Status update | what changed, what is blocked and by whom, the next action, all early | scene-setting the reader already has; a list of next steps with no owner |

## Prose Style

Sentences, tense and terms follow writing/plain. Write ideas that depend on each other as connected sentences, and keep a list for items that stand alone.

- Lead with the conclusion when the audience is busy. Lead with context when the audience needs persuading.
- Cut filler on revision. "In order to" becomes "To". "It should be noted that" goes.
- Say what a thing is, and leave out a definition by negation ("it's not X, it's Y").
- Use one precise adjective where three vague ones would stack up.
- Leave out corporate filler: hollow superlatives, engagement bait, and enthusiasm the substance does not support.

## Revision

Revise in passes:
1. **Structure.** Is the argument in the right order?
2. **Clarity.** Rewrite any sentence you had to read twice (writing/plain).
3. **Tone.** Does it match the audience and purpose?
4. **Concision.** What can be cut without losing meaning?

## Pushback

If the format doesn't fit the content (a slide deck for something that needs a detailed report), push back. Explain why format matters and suggest an alternative. Preserve the user's intent — they want to communicate something, the format is negotiable.

## See Also

- documentation(documentation) — code documentation is a specialized form of writing
- trust/prose(meta) — decoration and self-explanation in your own prose
- trust/voice(meta) — voice and tone in written communication
- writing/deliverables(writing) — finished decks, PDFs, documents and sheets for a client or leadership
- research/provenance(research) — every claim, figure and citation traced to its source
- writing/client-boundary(writing) — internal material kept out of outbound documents
- writing/ai-signs(writing) — a draft that reads as if a model wrote it
- writing/ai-signs/actors(writing) — sentences that say who did what
- writing/status-report(writing) — recurring reports that follow the last approved one
- writing/own-words(writing) — editing the user's own text in their voice
- writing/register(writing) — shifting register for a reader, and holding a user's style rules
- writing/plain(writing) — present tense, each fact once, whole sentences, terms defined, who does what
