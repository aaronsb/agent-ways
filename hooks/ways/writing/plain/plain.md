---
description: drafting a commit message, a PR description, an issue, a README section, a doc page, a docstring or a code comment so that a newcomer understands it on the first read; whole sentences in plain words and the present tense, each fact said once, a project term defined where it first appears, and each sentence naming who does what
vocabulary: readme section doc page docs documentation explain explaining add a comment doc comment docstring code comment what this function does commit message pr description issue body write draft plain clear readable sentence present tense past tense newcomer first read define term everyday word who does what compressed reread
files: \.md$
pattern: \b(doc ?comments?|docstrings?|jsdoc|tsdoc|rustdoc|godoc|pydoc|javadoc)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Plain Writing

These rules cover text that someone reads later: docs, READMEs, code comments, ADRs, commit messages, PR bodies, issues, reports and user-facing strings. The output style sets the register of the conversation itself (ADR-178). A project's own writing rules in its CLAUDE.md or AGENTS.md come first, and text the user wrote stays in their words (writing/own-words).

## The rules

1. Write about the project in the present tense, as what it does now. Use the past tense for history, and for a predecessor or a reference implementation.
2. Say each fact once. Where another place needs the fact, point to the place that states it.
3. Write the way you would explain the thing to a colleague out loud: whole sentences with a subject and a verb, in plain words. Don't be clever, because a metaphor or a slogan makes the reader translate it back into the fact. Someone who has never seen the code should understand the text on the first read. If that takes more words, use more words.
4. Unpack a compressed phrase into what it means. A string of nouns usually hides a verb and the thing that does it.
5. Define a project term the first time a section uses it, in the words a newcomer would use. Where an everyday word means the same thing, use the everyday word.
6. Name who does what: the program, person or part that acts, with a plain verb. Leave a sentence passive only when nobody knows or cares who acted. writing/ai-signs/actors lists the habits to fix in an existing draft.
7. If you had to read a sentence twice, rewrite it before you commit it.

In a markdown file, each paragraph stays on one line (markdown(documentation)). A brief for a subagent that writes prose for people carries these rules (subagents/briefs).

## Before and after

| Rule | Before | After |
|---|---|---|
| Present tense | The hook was changed to skip subagents. | The hook skips subagents. |
| Once | A second section repeats "retries stop after three attempts". | The second section says "retries stop at the cap" and links the section that sets it. |
| Out loud | Rigor gets cheap. | When a check lives in a tool, it costs one command, so the agent runs it every time. |
| Unpack | A child under a fired parent clears a boosted bar. | Once a parent way has fired, its child ways fire on a weaker match. |
| Define | The projection refuses a real directory. | agent-ways links its folders into `~/.claude` (the projection), and it stops when one of those paths is already a real folder. |
| Who does what | The keyword lane falls to the semantic floor. | The matcher fires a way on a keyword hit only when the prompt's meaning is already close to the way. |

## See Also

- writing(writing) — parent: audience, structure and genre
- writing/ai-signs/actors(writing) — rewriting a draft so each sentence names who acted
- trust/prose(meta) — decoration in long drafts, and the self-edit pass
- markdown(documentation) — line handling in markdown files
- subagents/briefs(meta) — the writing rules a prose-writing worker receives
