---
description: making a draft more formal or more plain for the people who will read it, such as tightening a casual note for a board or turning engineer-speak into friendly copy for consumers, and holding a style the user wants kept across a project by writing it into the output style, a project-scope instruction file or a skill, with each rule checkable and each ban paired with what to write instead
vocabulary: register formal informal formality casual too casual too formal more professional tone it down tighten it up tone plain language engineer speak jargon friendly audience board executives consumers customers readers diction style rules house style style guide always write our docs british spelling no em dashes banned words claude.md agents.md output style keep this style every session
pattern: \bhouse style\b|\boutput.?style\b|\b(em|en).?dash(es)?\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Register

Register is how formal, plain or technical a text reads. It comes up in two kinds of work: shifting a draft's register for its reader, and holding a register the user sets for a project.

## Shifting register for a reader

- Name the reader and what they already know. A board reads decisions, risk and cost. A consumer reads what happened and what to do next.
- Change diction and sentence form, and keep every verifiable fact. Reword instead of deleting.
- For a formal reader, drop slang and chat phrasing. For a lay reader, replace internal and engineering terms with what the reader sees, and spell out a term the first time it must stay.
- Rewrite whole sentences. Swapping single words leaves the old register in the sentence shape.

## Holding a style across a project

The session's register comes from the output style (ADR-178). When the user wants a style kept beyond this conversation:

- Write it where every session reads it: the output style, a project-scope CLAUDE.md or AGENTS.md, or a project skill. Memory and refiring ways are the wrong home for it.
- Write each rule so a search can check it. "No em dashes" can be checked. "Use em dashes sparingly" has no threshold.
- Pair each ban with what to write instead. A ban on long sentences with no positive rule pushes the writer into fragments.
- When a rule bans a punctuation mark, rewrite the sentence. A colon or a double hyphen in place of a banned dash keeps the same construction.
- Leave the user's own titles and quotes as written.

A requested length or shape is covered in trust/prose.

## See Also

- writing(writing) — parent: audience and genre
- trust/prose(meta) — decoration, self-editing, and requested length and shape
- trust/voice(meta) — whose voice to write in, a separate question from how it reads
- writing/own-words(writing) — editing the user's text in their voice
