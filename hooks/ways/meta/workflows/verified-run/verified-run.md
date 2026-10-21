---
description: Running a whole task, build or research, as a planned and verified multi-agent run when the user says a trigger word they defined for it. Plan first, split work into packages with fixed contracts, sweep in parallel, verify key claims with skeptics, run a completeness critic, keep shared and side-effecting writes serial in the main loop, and return one consolidated result.
vocabulary: trigger-word user-defined keyword deep-run full-treatment go-deep planned verified multi-agent work-packages fixed-contract sweep skeptics completeness-critic gap-fill integrate final-qa consolidated ranked-report serial-writes read-only-fan-out dry-run rubric plateau elapsed-estimate
pattern: \bplanned,? (and )?verified multi.?agent\b|\bverified multi.?agent (run|workflow)\b|completeness critic|full (multi.?agent|workflow) treatment
refire: 0.15
scope: agent
---
<!-- epistemic: convention -->
# Planned, Verified Multi-Agent Runs

Some users define a trigger word in their project or user instructions that means "run this as a planned, verified multi-agent workflow". The word is the explicit opt-in the parent way requires. It grants depth even on a small ask, such as "finish that one action item" or "a brief benchmark". A plan alone perfects the approach. The trigger word means run it in parallel, so the main context stays clean. If the harness ships a workflow-authoring skill, load it before you write the script.

## Phase shape

1. **Sweep.** Run parallel agents, one per source or work package. Cover every source the user can reach: each mail account, shared drives, local folders, and past session transcripts.
2. **Verify.** Run two or three skeptics or refuters per key claim.
3. **Synthesize.** Start only after every agent has reported. Then run a completeness critic and fill each gap it finds.
4. **Integrate.** Merge the results in the lead, then run final QA.

For builds, split the work into packages that each own their files. Each package passes automated tests and screenshot checks against a written rubric. Do a dry run before the full matrix. For research, apply every fact-check discrepancy before delivering, and keep a traceable claim ledger.

## Contracts

Put the shared contract, such as the API, data shape, or output format, verbatim in every agent's prompt. Parallel agents then agree without talking to each other.

## What fans out and what stays serial

- Fan out research, drafting, judging, and verifying as read-only work. Agents return finished text, and the main loop writes each file once.
- A build agent edits only the files its package owns. Shared files and live systems have a single owner.
- Every side-effecting write runs serially in the main loop, one call each.
- Tell any agent that touches email, chat, or other comms, in its prompt, that it is read-only or drafts-only. It never sends.

## Run constraints

Honor each constraint the user attaches to the run exactly. Common ones are research-only with no edits, a named model on every agent, and a named skill used "throughout all agents".

## Depth and pace

- Depth is the default once the user says the word.
- The readout stays as short as the user asked, for example a few paragraphs. It is fact-checked and honest about its numbers.
- Before a long round, give an honest estimate of elapsed time. Run independent evaluations in parallel. Keep merge phases short.
- When critique scores plateau, stop iterating and say so.

## Handback

Return one consolidated, ranked report. List the decisions the user must make on their own. State what you deliberately left uncovered. Do not stream partial findings in place of the final synthesis.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The ask is small, so one agent is enough." | The user said the word. Run the phases. |
| "Each agent can write its own output file." | Agents return text. The main loop writes each file once. |
| "The critic found little, so skip the gap-fill." | Fill each gap it named, then deliver. |
| "I'll send findings as they arrive." | Deliver one consolidated report. |

## See Also

- workflows(meta) — parent: when a workflow is warranted and the opt-in it needs
- subagents(meta) — briefs, brief fidelity, and the return shape
- research(research) — source authority and fan-out brief rules
- delivery/groundwork/claims(softwaredev) — agent reports are claims to verify
