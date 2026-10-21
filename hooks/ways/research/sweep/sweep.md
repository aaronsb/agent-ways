---
description: Investigating a broad question whose evidence is spread across many systems, such as email, chat, tickets, git, and business records, with one scout agent per source, then verifying the key claims adversarially before delivering. Check prior work first, read primary pages yourself, wait for every scout, label claims HOLDS or OVERTURNED with evidence, and save a claim ledger in a research folder.
vocabulary: investigate evidence spread scattered scout scouts per-source inbox calendar chat tickets wiki crm finance-system knowledge-graph prior-work refuter refuters verifier adversarial holds overturned unverifiable keep rescope kill reconcile recompute claim-ledger research-folder lossless leave-no-stone cross-reference
pattern: (one|a dedicated|a separate|separate) (sub)?(agent|scout)s? (per|for each) (source|inbox|account|system|domain|channel)|(until|before) all (the )?(sub)?(agents|scouts|tracks) (have )?(fully )?(returned|reported)|leave no stone|research ledger|claim ledger|research (sub)?(folder|directory|subdirectory)|adversarial(ly)? verif|\blossless\b|(multiple|parallel|many) research (sub)?agents
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Research Sweep and Verify

The parent way covers scoping, source authority, and the four fan-out brief rules. This way covers an investigation that sweeps many systems and the verification pass before delivery.

## Check prior work first

- Check the repo for existing research, transcripts, and analyses. Skip what is already done.
- Search chat, tickets, the wiki, and meeting transcripts for rules someone already gave, such as how a manager wants a cost classified. Follow those rules instead of guessing.

## One scout per source

- Start one scout per source: each inbox, each calendar, git, local project folders, chat, the ticket tracker, the wiki, and each business system the user can reach, such as a CRM or finance system.
- When the user asks for depth, dig past the first access blocker. Look for a workaround before you report a gap.
- Give each scout explicit search terms. Use scouts for big subtrees, not for a single quick lookup.
- Keep any team whose members message each other small. Run the remaining sources as isolated subagents.
- Have scouts cross-reference each other. Decide what counts from the combined evidence. Test hypotheses instead of assuming them.
- Every brief also asks for a confidence level and a list of what was not covered.

## Read primary pages yourself

- Fetch and read page bodies, shared documents, and transcripts directly. Do not send agents for summaries, and do not use summary-only fetch modes for the claims you will cite.
- Match each claim to a primary or official source: the original author over a textbook, and official docs over secondary write-ups.
- Quote borrowed wording in blockquotes with links.

## No synthesis until every scout reports

Write no conclusions until every track has returned. Say which tracks you are waiting on.

## Verify before delivering

- For high-stakes claims, run refuters with distinct lenses. Run an independent verifier that recomputes the key figures before it reads the analysis code.
- Label each claim HOLDS, OVERTURNED, or STILL-UNVERIFIABLE. For recommendations, use keep, rescope, or kill. Cite the evidence in every verdict.
- Distrust naive reads. A sandbox query can undercount, and sandbox state can differ from production.
- Reconcile totals across every view. Apply every discrepancy you find. Keep caveats visible.
- Agent output is the agent's reading of the record. It is not the user's position. Open the cited file or slide before you quote it.

## Output

- Save durable findings as markdown under `research/<topic>/`, with a ledger of each claim, its source, its verdict, and the fixes applied.
- Give a short readout in chat: the bottom line, then confidence, then caveats. Respect any length cap the user sets.
- Commit only when asked.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The scout summarized it, so I can quote it." | Open the cited source and quote that. |
| "Three of four scouts are back, so I'll start drafting." | Wait for the fourth. |
| "The figure came from a query, so it is right." | Have a verifier recompute it from the source. |
| "The access error means the data is unavailable." | Look for another route before you report the gap. |

## See Also

- research(research) — parent: scoping, source authority, and fan-out brief rules
- subagents(meta) — briefs and the return shape for each scout
- delivery/groundwork/claims(softwaredev) — treating an agent's report as a claim
