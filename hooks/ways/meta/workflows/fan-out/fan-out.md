---
description: Parallel workers that must agree — the shared contract verbatim in every worker prompt, the lead as the single writer and sender, skeptics and a completeness critic, and a single ranked handback
vocabulary: workers disagree shared contract verbatim same data format single writer lead writes every file skeptics completeness critic gap-fill partial findings ranked handback never sends
pattern: completeness critic|\bverified multi.?agent\b
refire: 0.15
scope: agent
---
<!-- epistemic: convention -->
# Running a Fan-Out

The parent way decides whether a task warrants a workflow. This way covers running one so the result can be trusted. If the harness ships a workflow-authoring skill, load it before you write the script.

## Phase shape

1. **Sweep.** Run parallel agents, one per source or work package, within the width the parent way and subagents(meta) set.
2. **Verify.** Run skeptics or refuters on each key claim.
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

Honor each constraint the user attaches to the run exactly. Common ones are research-only with no edits, a named model on every agent, and a named skill used by every agent.

## Pace

- The readout stays as short as the user asked. It is fact-checked and honest about its numbers.
- Before a long round, give an honest estimate of elapsed time. Run independent evaluations in parallel. Keep merge phases short.
- When critique scores plateau, stop iterating and say so.

## Handback

Return one consolidated, ranked report. List the decisions the user must make on their own. State what you deliberately left uncovered. Do not stream partial findings in place of the final synthesis.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Each agent can write its own output file." | Agents return text. The main loop writes each file once. |
| "The critic found little, so skip the gap-fill." | Fill each gap it named, then deliver. |
| "I'll send findings as they arrive." | Deliver one consolidated report. |

## See Also

- workflows(meta) — parent: when a workflow is warranted and the opt-in it needs
- subagents/briefs(meta) — what each worker's prompt carries
- research(research) — source authority and fan-out brief rules
- delivery/groundwork/claims(softwaredev) — agent reports are claims to verify
