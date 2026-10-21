---
description: The prompt is built from XML-tagged sections such as task, context, requirements, and style, or the user is dictating a long ramble with many things mixed together. Treat each tag as a separate labeled input and hold word caps exactly. Sort a ramble into separate items, say what depends on what, and flag blockers before you do anything.
vocabulary: xml tags tagged sections task context requirements style braindump dictating phone ramble mixed together separate items depends blocked word cap
pattern: </?(task|context|braindump|methodology|writing[-_ ]?style|style[-_ ]?guid[a-z]*|requirements|discovery|thoughts|transcript|raw[-_ ]?materials|console[-_]?logs)>
pattern_strict: true
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Tagged Prompts and Braindumps

Some users write important prompts as XML-tagged sections, in any order. Each tagged block is a separate labeled input. Refer to it by its tag name. Never merge the blocks into one undifferentiated input. Tags hold the material. Text outside the tags holds the instruction, for example "Rate the performance" after a `<console_logs>` block.

## What each tag means

| Tag | Treat as |
|-----|----------|
| `<task>` | The deliverable list. Address every item. |
| `<requirements>` | Acceptance criteria. Check each one before delivering. |
| `<methodology>` | The required process, not a suggestion. |
| `<style>`, `<writing_style>`, `<style_guide>` | Register and output format. These override your defaults. |
| `<context>`, `<raw_materials>`, `<transcript>`, `<console_logs>`, `<example>`, `<thoughts>` | Material to honor in full. |
| `<discovery>` | Research tasks to run before drafting. |

Users invent their own tags. Read an unfamiliar tag by its name and its contents, and treat it as labeled material.

## Hold every constraint

- Word caps and per-section counts are hard limits. Count the words before you deliver.
- "Return in chat only" means write no files.
- "No front matter or postlude" means no intro and no closing summary.
- Cite the named sources and use the named accounts.
- Scope fences are exact. "Do not plan the implementation, no code, at most 500 words" means exactly that.

## Braindumps

Braindumps are often dictated from a phone. Read them for intent, not literally, and keep any phrasing the user quotes verbatim. When one ends with "let me know your thoughts", reply in chat with your understanding and produce no artifacts yet.

Before acting on a braindump, decompose it:

1. List the discrete items or workstreams.
2. Note what depends on what.
3. Flag what is blocked, for example on credentials the user must provide.
4. Pick the simplest approach for each item.

Restate that breakdown in chat before building, unless the user already told you to proceed. When you build, track the items in the task list on a dedicated feature branch.

## Pasted transcripts

Extract only the decisions, commitments, and features relevant to this project, especially what the user committed to update or add. Return them as a numbered list or as issues. Build nothing until told.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The cap is roughly right" | A cap is a limit. Count and cut. |
| "The context block is background, I can skim it" | Tagged material is input. Honor it in full. |
| "The braindump is clear enough to start" | Restate the breakdown first. Dictation hides dependencies. |

## See Also

- choices(meta) — raising the open questions a braindump leaves
- todos(meta) — tracking the decomposed items
