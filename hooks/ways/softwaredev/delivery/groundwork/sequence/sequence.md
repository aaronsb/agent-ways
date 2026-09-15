---
description: ordering a queue of open pull requests and issues so changes that establish verification land before changes that assume it, and holding changes into an area nobody can yet test or deploy
vocabulary: queue ordering triage order backlog land first jump the queue hold blocked on prerequisite stack piling up unverifiable module merit versus order which first sequence open prs
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Order in the Queue

A change can be individually good and still wrong to accept **now**. That is what *premature* means: a question of order, not merit. Five reasonable capabilities merged onto a module nobody can test make the module harder to fix and each other harder to trust.

## Triage by readiness

For each open item, name the area it touches and check that area against the four conditions in `delivery/groundwork`. Then place the item:

| The item | Placement |
|---|---|
| **Establishes** a missing condition — adds the test command, wires CI, makes the deploy repeatable, demonstrates rollback | Jumps the queue. This is the work that makes the rest possible. |
| **Assumes** a condition the area does not meet — changes behaviour nobody can verify, deploys through a path nobody can repeat | Held, with the prerequisite named. Not rejected: held. |
| Touches an area that meets all four | Normal review depth per `delivery/merge`. |

A hold is a state with an exit condition: the prerequisite item landing, or a named person deciding the area does not need it. Record it that way (see `delivery/issues`, residuals). A hold without a condition is a quiet drop.

## What we do not do

- Merge a stack of behaviour changes into an unverifiable area because each one is small.
- Write the tests *after* the queue clears. The queue is what needs them.
- Rank items by how well they are argued. Rank them by what they make possible.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "This PR is fine on its own" | On its own it is. In this area, nothing is. Land the verification first. |
| "Holding it will annoy the contributor" | Holding with a named prerequisite is respect. A silent merge into an untestable module is the disrespect. |
| "We'll add coverage once these are in" | Then we'll be covering five changes at once and guessing which one broke it. |
| "The area has never had tests and it's been fine" | Then the first item that adds one is the most valuable thing in the queue. |

## See Also

- delivery/merge(softwaredev) — review depth once the area is ready
- delivery/issues(softwaredev) — recording a hold as a residual with an owner and a reopen condition
- delivery/groundwork(softwaredev) — parent: the four conditions
