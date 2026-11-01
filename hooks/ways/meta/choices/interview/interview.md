---
description: interviewing the operator on a series of open directional decisions, such as a draft record's open questions or a design's forks, each question carrying a brief, the case for and against, and a lean defended from checkable evidence, and ending when the operator trusts the agent to carry on alone
vocabulary: interview operator directional open questions lean evidence brief for against judge micro-decisions microdecisions framing round series satiation consequence defend grounded accept leans hold research unknowns handoff get it done carry on alone sufficient trust stop conditions
pattern: \bopen questions?\b|\binterview (me|the operator)\b|\bmicro-?decisions?\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Interviewing the Operator

When several calls above the waterline (choices) are open at once, such as a draft record's open questions or a design's forks, put them to the operator as a series of questions, each with the evidence to judge it by.

## Before the questions

Triage first. Settle the calls below the waterline yourself, with your subagents, and list each in a line with your lean so the operator can overrule it. If you cannot write a real case against your lean, the call is yours. A call made under high doubt (core.md) goes to the operator even below the waterline.

Then frame the set in the reply as a markdown table with numbered rows: each open decision, what it would change in the finished work, and the order you will take them in. An answer can then name its rows. Keep artifact pages for things the operator looks at or uses.

## Each question

Each question has three parts, worded for that decision:

- A brief in plain words: what the setting or rule does and what a user would see under each option, with an everyday example. A newcomer to the domain can follow it. Keep mode numbers, protocol names and spec terms out, or define each in a few words.
- The case for the leading option, and the case against it.
- Your lean, listed first and marked (Recommended), with evidence the operator can check: a file and line, a record, a measurement, a spec, a precedent. A lean with no evidence is a defect. Find the evidence, or say that the call turns only on what the operator wants.

Keep a question to about a hundred words, and move the rest into the framing or the record. The other presentation rules are in choices.

Name the unknowns your evidence leaves open. Offer a research pass before asking where a call needs knowledge the operator may lack, and a "hold, I'll research first" option where they may want to look before committing, as between libraries. A hold pauses only the work that depends on the call.

## Rounds

- Ask one to four questions per round, the call that most changes the work first.
- Offer a way to accept the remaining leans in one answer.
- Word each question from its own decision, since a stock sentence repeated across a batch stops being read.
- Re-triage after each round. An answer can settle a later question or raise a new one.

## After the answers

Record each answer where the work keeps it: `adr consider N --said "..." --via "..."` for a decision record, a comment on the PR or issue otherwise. Then act, and say which leans were taken, which were overruled, and which calls are on hold.

## Ending in a handoff

When calls above the waterline will keep coming, the last round may ask whether to keep asking or carry on alone. First check your read of the operator's intent in up to four questions:

- The goal and what done means, in two or three lines.
- The leans you will take on the calls ahead.
- What sends you back to ask. Offer as defaults a one-way step, an action others see outside the working branch, spending money or quota, and anything outside the stated scope. Rising doubt (core.md) stays on the list under every grant.
- How you will report: a log in the PR or the record of each call above the waterline you made alone, with its lean and evidence, reviewed at the end.

Then ask for the mode: keep asking, or carry on, optionally bounded by scope or time. Mark neither option as recommended. A "no" or no answer means keep asking (trust/autonomy).

A grant covers this one piece of work until it is done or a stop condition is reached. Under it, decide the remaining calls by your stated leans and log each one. Only checks you have seen fail on a planted defect (core.md) count toward low doubt. In a decision record, cite the grant as the operator basis (adr/consider). For work that runs unattended across turns, write the grant as a `/goal` condition (goal-author).

## See Also

- choices(meta) — parent: the waterline, and presenting a single choice
- adr/consider(documentation) — the operator's check of a drafted decision record
- trust/autonomy(meta) — the operator grants autonomy, and the agent does not argue for it
- goals(meta) — a grant that runs unattended as a `/goal` condition
- think(meta) — framing an unknown your tools cannot resolve
