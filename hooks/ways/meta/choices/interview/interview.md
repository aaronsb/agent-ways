---
description: interviewing the operator on a series of open directional decisions, such as a draft record's open questions or a design's forks, each question carrying a brief, the case for and against, and a lean defended from checkable evidence, and ending when the operator trusts the agent to carry on alone
vocabulary: interview operator directional open questions lean evidence brief for against judge micro-decisions microdecisions framing round series satiation consequence defend grounded accept leans hold research unknowns handoff get it done carry on alone sufficient trust stop conditions
pattern: \bopen questions?\b|\binterview (me|the operator)\b|\bmicro-?decisions?\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Interviewing the Operator

Some work reaches a point where several decisions are open at once: the open questions in a draft record, the forks in a design, the calls a plan leaves to the operator. Each question costs the operator attention, and a long run of questions in the same shape wears it down until the answers stop being judgements. The interview asks only the calls above the waterline (choices), and gives each one the evidence to judge it by.

## Before the questions

Settle the calls below the waterline yourself, with your subagents, and list what you chose in a line each so the operator can overrule any of them. If you cannot write a real case against your lean, the call is yours: make it and say so. A call made under high doubt, as core.md describes it, goes to the operator even when it sits below the waterline.

Then write a short framing in the reply: the decisions that are open, what each would change in the finished work, and the order you will take them in. A list of several open questions goes in a markdown table with numbered rows, so an answer can name rows; keep artifact pages for things the operator looks at or uses. Where running the work shows the answer better than a description, run it and show the output first.

## Each question

Each question carries four parts, in the words of that decision:

- A brief of the concern in plain words: what the setting or rule does and what a user would see under each option, with an everyday example. A newcomer to the domain follows it, and the operator answers without opening a file. Keep mode numbers, protocol names and spec terms out of the question, or define each in a few words.
- The case for the leading option.
- The case against it.
- Your lean and its defence, from evidence the operator can check: a file and line, a record, a measurement, a spec, a precedent. A lean with no evidence is a defect. Find the evidence, or say that the call turns only on what the operator wants.

The lean is the first option, marked (Recommended). Each other option says in one line what it trades. Keep a question to about a hundred words, and move detail past that into the framing or the record.

Name the unknowns your evidence leaves open, so the operator can see what more research would settle. The operator is not an expert in every domain a project touches. Where a call needs knowledge they may lack, offer a research pass before asking. Where they might want to look before committing, as with a choice between libraries, offer "hold, I'll research first" as an option. A hold pauses the work that depends on the call. Carry on with the work that does not.

A short example: *Cache budget, one per tenant or one shared? For one per tenant: what a tenant keeps depends only on its own traffic, so one tenant cannot evict another's entries. Against: memory grows with the tenant count, four times the shared budget at four tenants. Lean one per tenant: the spec's isolation rule is per tenant (its section and line), and the record that split the tenants made them independent sessions.*

## Rounds

- Ask one to four questions per round, the tool's limit, the call that most changes the work first.
- Offer a way to accept the remaining leans in one answer.
- Write each question fresh. The four parts stay, and the wording comes from the decision in front of you. A stock sentence repeated across a batch stops being read.
- Re-triage after each round. An answer can settle a later question or raise a new one.

## After the answers

Record each answer where the work keeps it: `adr consider N --said "..." --via "..."` for a decision record, a comment on the PR or issue otherwise. Then act on it, and say which leans were taken, which were overruled, and which calls are on hold.

## Ending in a handoff

When calls above the waterline will keep coming as the work goes on, the last round may ask whether to keep asking or carry on alone. Put a few questions that test your read of the operator's intent, one to four:

- The goal and what done means, restated in two or three lines. Is that right?
- The leans you will take on the calls still ahead. Any to change?
- What still stops you and sends you back to ask. Offer as defaults: a one-way step, an action outside the working branch that others see, spending money or quota, anything outside the stated scope. Rising doubt (core.md) is always on the list, and no grant removes it.
- How you will report: a log of each call above the waterline you made alone, with its lean and evidence, in the PR or the record, reviewed at the end.

Then ask for the mode: keep asking, or carry on, optionally bounded by scope or time. Mark neither option as recommended. The operator decides it, and a "no" or no answer means keep asking (trust/autonomy).

A grant covers this piece of work and ends when it is done or a stop condition is reached. It does not carry into the next task. Under it, decide the remaining calls by your stated leans and log each one as you make it. A loop's evidence counts toward low doubt only from checks you have seen fail on a planted defect and pass once it was removed. In a decision record, cite the grant as the operator basis, as adr/consider does for a goal condition. For work that must run unattended across turns, write the grant as a `/goal` condition (goal-author).

## See Also

- choices(meta) — parent: the waterline, and presenting a single choice
- adr/consider(documentation) — the operator's check of a drafted decision record
- trust/autonomy(meta) — the operator grants autonomy, and the agent does not argue for it
- goals(meta) — a grant that runs unattended as a `/goal` condition
- think(meta) — framing an unknown your tools cannot resolve
