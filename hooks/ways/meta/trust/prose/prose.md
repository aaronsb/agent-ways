---
description: Producing long-form prose — a thorough assessment, a detailed report, a written analysis, an in-depth explanation — and how that prose reads, including decoration, self-explanation, and editing your own draft
vocabulary: write assessment report analysis review explanation summary document draft thorough detailed comprehensive in depth long form walkthrough writeup prose style tone register decoration significance clause phrasing revise edit rewrite wording paragraph readable verbose padding
pattern: write (me |up )?(an?|the) (thorough|detailed|comprehensive|full|proper|honest|long) (assessment|analysis|review|report|write-?up|explanation|evaluation|breakdown|overview|summary|essay|memo|brief|proposal|comparison)|\b(thorough|detailed|comprehensive|in-depth|full) (assessment|analysis|review|report|write-?up|explanation|evaluation|breakdown|overview|summary|essay|memo|brief|proposal|comparison)\b|\baround \d{3,4}[- ]?words\b|\b\d{3,4}[- ]?words? (long|or so|minimum)\b
pattern_strict: true
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Prose — Delivering vs Decorating

Core states the rule in two lines. This way carries the expanded version, because the rule fails in one specific place: long drafting sessions, many turns after core landed.

## The failure this way is written against

Measured in this repo. An 11,500-word document drafted across a dozen turns contained significance clauses at 3.4 per thousand words against 0.5 in prose that had been reviewed, plus eleven instances of a construction core explicitly bans. Core had fired. The writing way had fired, fifty epochs earlier, carrying "use em dashes sparingly."

Two lessons sit in that.

**A style rule holds for short output and decays across a long draft.** The failure is in detection rather than compliance. You do not notice the violation while producing it, so a rule you were told once does not survive an essay.

**A rule with an adverb in it cannot be checked.** "Sparingly" has no threshold, so there is no moment at which you can test yourself against it. "Cut any clause that explains why the previous clause matters" is a search you can actually run.

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

## Directness and sentence length

A short sentence is not automatically a direct one. Directness means going straight at the point, which a long sentence can do perfectly well. Connect clauses with *and*, *because*, *so* rather than breaking for emphasis. Emphasis by fragment asks the reader to supply the drama.

## Density is a symptom, not a rule

Em-dashes, dramatic colons, and antithesis constructions are not wrong individually. They become a tic at volume, and volume is the thing to watch. Nobody should be counting punctuation while drafting; the count belongs to the check that runs afterward.

## When you are editing your own draft

Two passes, in this order:

1. Search for the patterns above and cut. Cutting is almost always the correct fix; rewriting rarely is.
2. Reread, and cut whatever you added back to preserve rhythm.

## See Also

- trust(meta) — the relational model this posture derives from
- trust/voice(meta) — whose voice to write in, a separate question from how it reads
- writing(writing) — structure, audience, and format for content creation
- `documentation/markdown/density` — the postcheck that counts what you just wrote
