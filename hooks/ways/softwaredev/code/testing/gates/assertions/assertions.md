---
description: the shape of a test assertion, composition versus a bare count, expected values independent of the subject, failure messages that name what drifted, mock assertions and change-detector tests that echo hardcoded arguments, snapshot and golden baselines, a named known-bug marker that expires when the bug is fixed
vocabulary: assert assertion expect expected mock called toHaveBeenCalled toHaveBeenCalledWith change-detector hardcoded arguments prune snapshot golden baseline known bug xfail known_bug marker tautological witness drift composition count vacuous volatile self-expiring serialized expectation echo
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Assertion Shape

A gate that executes and asserts can still prove nothing. The assertion compares a count that offsetting errors restore, or an expected value computed from the very thing under test, or a mock call tally that goes vacuous the moment the code stops calling the collaborator, or a mock argument that the test copied from the code. Each of these stays green through the bug it was written to catch.

## Three questions

**Does it assert composition, or only a count?** A total passes again the moment two errors cancel. A count of three returns to three while the set holds the wrong three. Assert what the result is made of: the names, the keys, the ordered members. Where completeness matters, take the full set difference. A spot check of the newest items is a sample. Report new coverage as a delta from the previous run, and establish "no regression" on a red baseline by comparing the names of failing tests before and after.

**Is the expected value independent of the subject?** An expectation derived from the value under test agrees with every mutation of it. Spell expected values as literals. Verify a derived value by a join across independently maintained sources. The test for any duplicate is the same: if one coordinated edit could change both copies with nothing failing, the copy is an echo. A copy that would fail is a witness, kept on purpose.

**Does a failure say what drifted?** Assert each dimension under its own name so a red identifies the property that moved. One assertion over a serialized blob says "different" and stops. Choose a witness sensitive to the change you guard against. A projection only witnesses what it projects.

## Mock assertions and change-detector tests

The canonical vacuous assertion is `expect(mock).toHaveBeenCalledTimes(1)`, and the rule for when a count is the behavior is in code/testing/mocking. Assert instead on what the input determines: the destination, the content, or the argument the code derived.

```js
// The input is a disk reading of 91%; the subject is what the code builds from it.
expect(send).toHaveBeenCalledWith(expect.objectContaining({ subject: "Disk 91%" }));
```

A change-detector test asserts that a collaborator got the exact arguments the code hardcodes, such as a fixed URL, a header set, or a retry setting copied from the source into the test. Its expected value is an echo of the code, so it fails on each edit to that line and on no defect. Change-detector tests are pruned at the ambient pass, the one full pass over the combined tree (code/testing/gates/attention): each is deleted, or replaced with an assertion on an outcome the product cares about.

## The self-expiring known-bug marker

When a suite must stay green while a confirmed bug still lives, do not weaken or skip the gate. Assert today's broken behavior on purpose, under a named marker:

```python
def test_KNOWN_BUG_1432_unauthenticated_read_returns_500(client):
    # Owed: require 401 once the auth middleware handles a missing token.
    assert client.get("/admin", auth=None).status_code == 500
```

The assertion passes while the bug lives and flips red the moment the bug is fixed without the assertion being tightened. The debt is named in the suite and retires itself. A skipped or relaxed test hides the same hole. An `xfail` without a reason and a ticket hides it more quietly.

## Golden and snapshot assertions

A snapshot is an assertion whose expected side was captured from the subject. It becomes a witness once a person has reviewed the baseline and the review is on record. Capture it before the change, with volatile leaves masked (timestamps, ids, tokens). After the change, allow only an enumerated list of intended deltas, each with its reason. An unexplained diff is a red to explain before the gate is green. Re-recording the snapshot to make it pass turns the baseline back into an echo.

## Rapid iteration phase

Recording a golden asserts that its layout is settled. While the operator is still deciding the flow or layout, that claim is false. Each round of polish feedback would then re-record dozens of frames, which misstates what is settled and costs effort every round.

- Keep a handful of anchor states as settled goldens.
- List the rest as provisional in a committed, reviewed file. A provisional golden is never asserted or re-recorded, in any run. A drift in one is noted and does not fail.
- Cover behaviour with state and transition tests, such as table tests of a pure step function.
- Promote the provisional goldens to settled once the operator accepts the layout. That acceptance ends the phase.

A provisional golden is not yet a judge, so changing one during the phase does not touch the judge. Adding a frame to the provisional list weakens the gate, and promoting one creates a judge. Each is a reviewed change. A refactor with no change in behaviour still leaves every settled golden byte-identical.

## See Also

- code/testing/gates(softwaredev) — parent: gate states, positive controls, and the report format
- code/testing/mocking(softwaredev) — the rule on when a call count may be asserted
- code/testing/gates/attention(softwaredev) — the ambient pass that prunes change-detector tests
- freshness/groundtruth(softwaredev) — the golden-master oracle captured before a refactor or migration
- code/testing/tdd(softwaredev) — a test must be seen red before its green means anything
