---
description: Producing long-form prose — a thorough assessment, a detailed report, a written analysis, an in-depth explanation — and how that prose reads, including decoration, self-explanation, and editing your own draft
vocabulary: write assessment report analysis review explanation summary document draft thorough detailed comprehensive in depth long form walkthrough writeup prose style tone register decoration significance clause phrasing revise edit rewrite wording paragraph readable verbose padding emphasis punctuation two-pass self-editing searchable
pattern: write (me |up )?(an?|the) (thorough|detailed|comprehensive|full|proper|honest|long) (assessment|analysis|review|report|write-?up|explanation|evaluation|breakdown|overview|summary|essay|memo|brief|proposal|comparison)|\b(thorough|detailed|comprehensive|in-depth|full) (assessment|analysis|review|report|write-?up|explanation|evaluation|breakdown|overview|summary|essay|memo|brief|proposal|comparison)\b|\baround \d{3,4}[- ]?words\b|\b\d{3,4}[- ]?words? (long|or so|minimum)\b
pattern_strict: true
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Prose — Delivering vs Decorating

This way covers decoration in long drafts: a clause whose only job is to tell the reader that the fact before it matters. The rule fails most in long drafting sessions, many turns after the session's first guidance arrived. Sentences, tense and terms are in writing/plain.

## The failure this way is written against

Measured in this repo. An 11,500-word document drafted across a dozen turns contained significance clauses at 3.4 per thousand words against 0.5 in prose that had been reviewed, plus eleven instances of a construction core explicitly bans. Core had fired. The writing way had fired, fifty epochs earlier, carrying "use em dashes sparingly."

The measurement shows two things.

A style rule holds for short output and decays across a long draft. You do not notice the violation while you produce it, so a rule you were told once does not survive an essay.

A rule with an adverb in it cannot be checked. "Sparingly" has no threshold, so there is no moment at which you can test yourself against it. "Cut any clause that explains why the previous clause matters" is a search you can actually run.

## What decoration looks like

The tell is a sentence that explains its own importance. You write a fact, then attach a clause telling the reader what to think about it.

| Decorated | Delivered |
|---|---|
| "The audit found three dead triggers. That is the mark of a system with no linting." | "The audit found three dead triggers." |
| "Core fires once per session. This is the most serious gap in the retention model." | "Core fires once per session." |
| "It is worth noting that the corpus has never shrunk." | "The corpus has never shrunk." |

The fix is deletion. Watch for the version of this edit that only relocates the evaluation into a subordinate clause — "three dead triggers, none of which the system would have surfaced on its own" keeps the whole move and merely hides the tell from a search. If the appended clause carries a fact the reader needs, give it its own sentence stating the fact: "None of the three appear in the linter's ruleset." If it only ranks what came before, cut it.

Watch for these specifically, because they are the productive forms:

- `That is the…` / `This is the…` opening a sentence whose only job is evaluation
- `which is exactly…` / `which is why…` bolted onto a finished clause
- `worth noting/stating/flagging`
- `X matters more than Y` where nothing downstream depends on the ranking
- A one-sentence paragraph that only announces the previous paragraph's significance

## Volume

In file prose, em-dashes, dramatic colons and antithesis constructions become a tic at volume. Watch the volume after drafting, with the check that counts (markdown/density). Counting punctuation while drafting gets in the way of the draft.

## When you are editing your own draft

Two passes, in this order:

1. Search for the patterns above and cut. Cutting is almost always the fix.
2. Reread, and cut whatever you added back to preserve rhythm.

## See Also

- trust(meta) — the relational model this posture derives from
- trust/voice(meta) — whose voice to write in, a separate question from how it reads
- writing(writing) — structure, audience, and format for content creation
- writing/plain(writing) — whole sentences in plain words, each fact once
- markdown/density(documentation) — the postcheck that counts what you just wrote
