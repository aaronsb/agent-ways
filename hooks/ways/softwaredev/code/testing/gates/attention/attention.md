---
description: focal and ambient verification at agent pace; run the gates a change can reach on its own branch, run the full suite once on the combined tree where branches come together, test a tree that already passed only when it has changed, and prune change-detector tests at the integration pass
vocabulary: focal ambient regression test selection rts presubmit postsubmit test result caching tree hash merge queue integration branch wave candidate release bump full suite every branch run it again duplicate run rerun wall clock waiting on ci critical path reachable gates does not reach change-detector test mutation testing defect seeding honest suite suboptimization deming combined tree
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Focal and Ambient Verification

CI practices grew up at human pace, where a pipeline's minutes hid behind a person's thinking time. At agent pace, CI's wall-clock time is the longest wait in the loop, so every duplicate run lands on the critical path. Agents also follow written procedures literally, so guidance must say when a check is already done.

Verification therefore runs in two places, and each finds defects the other cannot.

## Focal verification

Focal verification is the set of checks a change can reach, chosen by the worker's judgment. The worker is the agent that holds one worktree and one change. The established name for this is regression test selection (RTS): run the tests that exercise the code the change touches, and leave the rest. The gate depth table in the parent way sets how far a change of each class reaches.

A gate left unrun is recorded as discovered, with the reason "the change does not reach it". A re-review after a fix is sized to the fix: it replays each finding's scenario and reads the lines the fix changed (code/testing/evaluation).

## Ambient verification

Ambient verification is one full pass over the combined tree where branches come together: an integration branch, a wave candidate, or the release bump. It runs once per tree, usually in CI. The established names are presubmit and postsubmit testing. The ambient pass catches the defects that live between branches, which no single branch's checks can see, and it catches the focal calls that were wrong. A failure in a gate that a worker recorded as not reached goes back to that worker's branch.

Running the full suite on every branch as well spends each branch's wall-clock time on defects the ambient pass finds anyway. Each branch then looks thorough while the whole loop runs slower. Deming called this suboptimization: a part tuned on its own measure at the cost of the whole system.

The lead, the agent that steers the workers, holds ambient attention and does not re-run a worker's gates (subagents/lead).

## One run per tree

A tree that passed is not tested again by a later push, merge or bump of the same tree. A local run counts as that tree's run, and is repeated only when the tree changed. Test result caching keyed by tree hash and merge queues are the mechanisms; delivery/ci covers them and the rule for a check that is still waiting for a runner.

## An honest suite

The ambient pass can trust the suite only when every test in it can fail for a reason the product cares about. Mutation testing proves this across a suite: a tool makes small changes to the product code and reports each change that no test caught. Defect seeding proves it for one gate, by the planted-defect procedure (code/testing/gates).

A change-detector test asserts that a collaborator got the exact arguments the code hardcodes (code/testing/gates/assertions). It fails on each edit to that code and on no defect, so it slows every focal run and protects nothing. The ambient pass prunes change-detector tests: it deletes each one, or replaces it with an assertion on an outcome. The pruning belongs to the ambient pass because that pass reads the whole suite, while a worker's change owns only its own tests.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll run the full suite on this branch to be safe" | The full suite runs once, on the combined tree. Run the gates this change reaches and record the rest as discovered. |
| "CI will run it again after the merge" | A tree that passed is not tested again. Cite the run that passed. |
| "The suite is green, so it guards the product" | Each test must be able to fail for a reason the product cares about. A planted defect or a mutation run shows whether it can. |
| "I'll review the whole branch again after the fix" | Size the re-review to the fix. Breadth comes at integration. |

## See Also

- code/testing/gates(softwaredev) — parent: gate states, gate depth by change class, and the planted-defect procedure
- code/testing/gates/assertions(softwaredev) — change-detector tests and what to assert instead
- code/testing/mocking(softwaredev) — when a call count may be asserted
- code/testing/evaluation(softwaredev) — replaying a finding's scenario, and the cap on review rounds per branch
- delivery/ci(softwaredev) — test result caching, merge queues, and checks still waiting for a runner
- subagents/lead(meta) — the lead's ambient attention across the workers
- delivery/merge(softwaredev) — review depth for a change that sets direction or has a high blast radius
