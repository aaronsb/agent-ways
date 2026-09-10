---
description: the shape of a test assertion, composition versus a bare count, expected values independent of the subject, failure messages that name what drifted, mock call counts, snapshot and golden baselines, a named known-bug marker that expires when the bug is fixed
vocabulary: assert assertion expect expected mock called toHaveBeenCalled snapshot golden baseline known bug xfail known_bug marker tautological witness drift composition count
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Assertion Shape

A gate that executes and asserts can still prove nothing. The assertion compares a count that offsetting errors restore, or an expected value computed from the very thing under test, or a mock call tally that goes vacuous the moment the code stops calling the collaborator. Each of these stays green through the bug it was written to catch.

## Three questions

**Does it assert composition, or only a count?** A total passes again the moment two errors cancel. A count of three returns to three while the set holds the wrong three. Assert what the result is made of: the names, the keys, the ordered members. Where completeness matters, take the full set difference. A spot check of the newest items is a sample. Report new coverage as a delta from the previous run, and establish "no regression" on a red baseline by comparing the names of failing tests before and after.

**Is the expected value independent of the subject?** An expectation derived from the value under test agrees with every mutation of it. Spell expected values as literals. Verify a derived value by a join across independently maintained sources. The test for any duplicate is the same: if one coordinated edit could change both copies with nothing failing, the copy is an echo. A copy that would fail is a witness, kept on purpose.

**Does a failure say what drifted?** Assert each dimension under its own name so a red identifies the property that moved. One assertion over a serialized blob says "different" and stops. Choose a witness sensitive to the change you guard against. A projection only witnesses what it projects.

## The mock invocation count

The canonical vacuous assertion is `expect(mock).toHaveBeenCalledTimes(1)`. It passes for the right call, and it keeps passing when the code calls the collaborator once for an unrelated, wrong reason. Assert on the destination, the content, or the argument passed:

```js
expect(send).toHaveBeenCalledWith({ to: "ops@example.com", subject: "Disk 91%" });
```

A count is the behavior only when the count is what the code promises, such as exactly one charge per order. The mocking way holds the wider rule on call counts. This way covers what to assert instead.

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

## See Also

- code/testing/gates(softwaredev) — parent: gate states, positive controls, and the report format
- code/testing/mocking(softwaredev) — call counts are asserted only when the count is the behavior
- freshness/groundtruth(softwaredev) — the golden-master oracle captured before a refactor or migration
- code/testing/tdd(softwaredev) — a test must be seen red before its green means anything
