---
description: surveying widely across many independent sources before answering or recommending, such as docs, benchmarks, github issues, papers and forum threads on a technology, or the inbox, chat, tickets and repositories behind a question about the user's own work; check prior work, size the fan-out of scouts and say the number, wait for every scout, read the primary sources yourself, have the key claims checked by someone trying to refute them, and keep a claim ledger
vocabulary: survey widely look at what's out there before recommending current state of the field landscape wide survey broad search many sources references literature papers benchmarks github issues forums community threads prior work scouts fan out several agents wait for every agent primary source refute check the key claims recompute figures claim ledger evidence scattered across email chat tickets wiki
pattern: \bclaim ledger\b|\bliterature (review|survey)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Research Sweep

The parent research way covers scoping, source authority and the rules every fan-out brief carries. This way covers a question whose evidence is spread over many independent sources, and the check on the key claims before the answer goes out.

## Prior work first

Look for existing research, notes and analyses in the project, and for decisions someone already recorded in the tracker, wiki or chat that settle part of the question. Build on them rather than redoing them.

## Size the fan-out

- Give each independent source, or group of sources, one scout. For a technology question that means docs, the issue tracker, papers, benchmarks and forums. For a question about the user's own work it means the inbox, chat, tickets, wiki and repositories.
- Say the number of scouts before starting them. Past about six, ask first, or group the sources until the count fits (meta/subagents).
- Give each scout explicit search terms, and ask for its confidence and for what it did not cover. A single quick lookup does not need a scout.
- When a source refuses access, the scout reports the gap. It does not look for a way around the control.

## Wait for every scout

Write no conclusions until every scout has reported. Say which ones are still out.

## Read the primary sources

- Open and read the pages, documents and transcripts the claims rest on. A scout's summary is its reading, not the source.
- Match each claim to a primary or official source. Quote borrowed wording with its link.

## Check the key claims

- For each claim the answer rests on, have an agent that did not find it try to refute it, and have an independent run recompute the key figures from the source. The refuter and the recompute run are independence spawns. It is asked for when the user asked for the sweep's claims to be checked, and otherwise you propose it. Name the payoff in a clause: a fresh agent is free of the finder's reading. The scouts follow the fan-out rule in research.
- Each verdict is refuted, could not refute, or unverifiable, with its evidence. Could not refute is the strongest verdict a check can give (the skeptic agent).
- Reconcile totals that appear in more than one view, and keep the caveats visible.

## Output

- The main agent saves durable findings where the project keeps its research, with a ledger of each claim, its source and its verdict. A subagent returns the ledger as text.
- In chat, give the answer, then the confidence, then the caveats.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The scout summarized it, so I can quote it." | Open the cited source and quote that. |
| "Three of four scouts are back, so I'll start drafting." | Wait for the fourth. |
| "The figure came from a query, so it is right." | Have an independent run recompute it. |
| "More scouts means more coverage." | Say the number. Past about six, ask. |

## See Also

- research(research) — parent: scoping, source authority, and fan-out brief rules
- subagents(meta) — fan-out width, briefs and the return shape for each scout
- delivery/groundwork/claims(softwaredev) — treating an agent's report as a claim
