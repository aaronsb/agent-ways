---
description: focal and ambient verification; run the gates a change can reach on its own branch, run the full suite once on the combined tree where branches come together, test a tree that already passed only when it has changed, and prune change-detector tests at the integration pass
vocabulary: focal ambient regression test selection rts presubmit postsubmit test result caching tree hash merge queue integration branch wave candidate release bump full suite every branch run it again duplicate run rerun wall clock waiting on ci critical path reachable gates does not reach change-detector test mutation testing defect seeding honest suite suboptimization combined tree
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Focal and Ambient Verification

Verification runs in two places, and each finds defects the other cannot. Each duplicate run adds CI's wall-clock time to the agent's wait.

## Focal verification

Focal verification is the set of checks one change can reach, chosen by the worker, the agent that holds one worktree and one change. Its established name is regression test selection (RTS): run the tests that exercise the code the change touches, and leave the rest. The gate depth table in the parent way sets how far a change of each class reaches.

The worker records a gate left unrun as discovered, with the reason "the change does not reach it". A re-review after a fix is sized to the fix: it replays each finding's scenario and reads the lines the fix changed (code/testing/evaluation).

## Ambient verification

Ambient verification is one full pass over the combined tree where branches come together: an integration branch, a wave candidate, or the release bump. It runs once per tree, usually in CI. Its established names are presubmit and postsubmit testing. The ambient pass catches the defects that live between branches and the focal calls that were wrong. A failure in a gate that a worker recorded as not reached goes back to that worker's branch.

Running the full suite on every branch as well is suboptimization, where each part is tuned to its own measure at the cost of the whole: each branch looks thorough while the whole loop runs slower.

The lead, the agent that steers the workers, holds ambient attention and does not re-run a worker's gates (subagents/lead). One run per tree is the rule that a tree which passed is not tested again, and delivery/ci covers it.

## An honest suite

The ambient pass can trust the suite only when every test in it can fail for a reason the product cares about. Mutation testing checks this across a suite: a tool makes small changes to the product code and reports each change that no test caught. Defect seeding checks it for one gate, by the planted-defect procedure (code/testing/gates).

A change-detector test asserts that a collaborator got the exact arguments the code hardcodes (code/testing/gates/assertions). It fails on each edit to that code and on no defect. The ambient pass prunes change-detector tests, because that pass reads the whole suite, while a worker's change owns only its own tests.

## Watch for

| You notice | It means | Do this |
|---|---|---|
| You are about to run `make test` or the full gate battery after a fix that touched one crate or one file | The fix reaches that crate or file, not the whole tree | Run the gates the fix reaches, such as that crate's tests. Record the others as discovered, with the reason "the change does not reach it". |
| A review swarm is about to start on one branch's small change | A breadth review on a branch repeats the one integration runs | Give the branch a review sized to its change. The breadth review runs once on the integrated candidate (delivery/merge). |
| The integration pass finds a defect that no branch could show, such as one branch citing a record that another branch adds | The defect lives between branches, where only the ambient pass looks | Fix it on the branch that owns the citation, or on the candidate. Keep each branch's review focal. |
| A test you are reading asserts that a mock got exactly the literal arguments the code passes | It is a change-detector test | At the ambient pass, delete it or replace it with an assertion on an outcome the product cares about. |
| A characterization test still pins behaviour that has since been decided | The test now asserts a guess | Rename it to state the decided behaviour, or delete it, as code/testing/tdd sets out. |

The signals about CI runs, triggers and parallel gate hosts are in delivery/ci. The signals about briefs and about checking a worker's report are in subagents/lead.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "CI will run it again after the merge" | A tree that passed is not tested again. Cite the run that passed. |
| "The suite is green, so it guards the product" | Each test must be able to fail for a reason the product cares about. A planted defect or a mutation run shows whether it can. |
| "I'll review the whole branch again after the fix" | Size the re-review to the fix. Breadth comes at integration. |

## See Also

- code/testing/gates(softwaredev) — parent: gate states, gate depth by change class, and the planted-defect procedure
- code/testing/gates/assertions(softwaredev) — change-detector tests and what to assert instead
- code/testing/mocking(softwaredev) — when a call count may be asserted
- code/testing/evaluation(softwaredev) — replaying a finding's scenario, and the cap on review rounds per branch
- code/testing/tdd(softwaredev) — when a characterization test expires
- delivery/ci(softwaredev) — test result caching, merge queues, and checks still waiting for a runner
- subagents/lead(meta) — the lead's ambient attention across the workers
- delivery/merge(softwaredev) — review depth for a change that sets direction or has a high blast radius
