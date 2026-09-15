---
description: tracing one change from clean checkout to production to find where it waits and who can move it, and separating coding time from waiting time before proposing tools, agents, or automation
vocabulary: stall waiting time where does it wait who moves it end to end walkthrough trace one change lead time cycle time bottleneck stuck handoff pending approval idle throughput why aren't we shipping faster
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Follow One Change

When the question is "why isn't more coming out the other end," the answer is rarely visible from the tooling. Before recommending anything, we pick **one** change and walk it from a clean checkout into production, with the people who did it.

## What we record

| Step | We write down |
|---|---|
| What builds | The command, or the person |
| Which checks run | Automatically, by hand, or "rerun until green" |
| Where it waits | Review, environment, approval, a product decision, a release window |
| Who could move it | A named person or role, and whether they know it is waiting |
| How long | Working time and waiting time, kept separate |

A test directory can look fine until someone explains the reruns. A deployment script can have an impressive name for something that still needs the one person who remembers.

## Then aim at the longest wait

Suppose coding takes two days and everything after it takes eight. Cutting coding in half saves one day in ten. The slide can say 50%; the customer waits nine days. Recovered coding time counts — it is just not five days.

So the fix goes where the change waited longest. Another hundred tests will not make someone answer an approval request. Another agent will not schedule the release window. If the longest wait is a decision, the finding is a decision, and it belongs to a person.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "We already know it's slow, let's just add automation" | Automation of the fast part is how coding got faster and delivery didn't. Walk it first. |
| "One change isn't a representative sample" | It is a real one. Walk a second comparable change after the fix and compare. |
| "The delay is organizational, not our problem" | Then say so, with the wait measured and the owner named. That is the finding. |
| "The tests pass eventually" | Record how many runs. Eventually is a wait. |

## See Also

- delivery/merge(softwaredev) — the review gate as one of the places a change waits
- incident(itops) — the closure artifacts a stall finding is filed alongside
- delivery/groundwork(softwaredev) — parent: readiness before change
