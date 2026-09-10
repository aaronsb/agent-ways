---
description: fixing a bug, refactoring, or changing existing code so the change integrates into the file instead of being patched on; no v2 wrapper or new-suffix copy of the old function, delete dead code and dangling imports after a removal, treat a rename that crosses an API or wire boundary as a contract change, sweep sibling copies of the same defect
vocabulary: fix the bug fix refactor patch change existing code modify update legacy old handler callers break compatibility rename wrapper v2 cleanup clean up dead code unused duplicate copy paste additive diff sweep bolted stitched integrate integration siblings copies dangling import removal boundary
files: \.(rs|py|ts|tsx|js|go|java|rb|sh|c|cpp|h)$
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: heuristic -->
# Integration Over Patching

A fix landed as the smallest diff that satisfies the request leaves a seam. The new branch sits beside the old logic, the helper it made redundant stays, and the next reader has to work out which path is current. Six months of such seams and the file records its requests in the order they arrived.

## The unit of work

The unit of work is the whole file or module. A change is complete when the file reads as if the requirement had existed from the beginning. Before editing, state the file's responsibilities, its main abstractions, and its conventions; if you cannot, read until you can. Locate where the change belongs, list what it makes redundant (helpers to merge, branches that go dead, names and comments that go stale, tests it implies), then rewrite the affected regions as a whole. Deletion and consolidation are first-class outcomes.

This governs the code that delivers the requested behavior. It adds no capability nobody asked for.

## Forbidden moves

| Move | Do this instead |
|---|---|
| A wrapper around the old function, `handleXNew`, `_v2`, an `Improved` or `Enhanced` suffix | Change the function in place; the name describes what it does now |
| A boolean flag routing around old behavior | Replace the behavior |
| An `if` for the new case while the general logic stays untouched | Change the general logic when that is where the requirement lives |
| Appending at the bottom of the file | Put the code where its abstraction lives |
| Commented-out code, `TODO remove` markers, dead branches kept to be safe | Delete them; version control holds the history |
| Fixing the symptom at the call site | Fix the abstraction that owns the defect |

## Self-check before calling the change done

- Is the diff purely additive? If so, justify why nothing needed to change or die.
- Does anything now exist in two places?
- After a removal, sweep for what still points at the removed thing: imports, exports, re-exports, type and enum references, docs, and tests. Check these separately from call sites; the call sites can all be gone while the import survives.
- Do all names, comments, and docs still tell the truth?
- Could a reader tell where the patch was stitched in?

## Renames

A rename is trivial while the identifier stays inside one compiled unit. It becomes a contract change the moment it crosses a serialization, wire, or process boundary: a persisted field, an enum constant another party reads, a token claim name, an HTTP or RPC path, a queue routing key, a service-discovery name. Some stored record or in-flight message still speaks the old name, and nothing fails at compile time. Version it, migrate it, or dual-read it.

## The class sweep

A defect found in one place is often one defect with N locations: the same wrong path in six pipelines, the same unguarded call in four adapters. The location you were handed holds no privilege.

1. State the defect so a search can find it, then search.
2. Land the fix once, at the seam that owns it. Collapse the copies into one shared implementation each member invokes. Where the copies cannot collapse in this increment (a generated file per consumer, a library outside the change), fix each site in its local conventions, verify uniformity by diff, and file the collapse as its own increment.
3. Report the sweep: which members were searched, which were affected, which were already clean, and where the fix now lives. A sweep you cannot enumerate is a claim.

If the class is too large for this increment, fix the instance, name the remaining members, and file them. A silent partial fix reads as a complete one.

## Scope

The sweep is bounded by the defect's identity. The scope rule (do the task asked, file the rest) is bounded by relevance. Both bounds hold at once. A sibling carrying the defect you were sent to fix is the same work; a different defect you noticed on the way gets filed. When integration touches files the request did not name, including the seam that should own the fix, list them before editing.

## Append-only artifacts

Changelogs, ADRs, audit logs, and ledgers are deliberately append-only. A stale entry there is struck or superseded. This way governs code and single-current-truth documents, where two copies of a fact is a defect.

## When it does not apply

A typo, a comment, a lint fix, or a single config value takes the trivial shortcut. The test is the unit of work: "fix this typo" is trivial; "fix this bug" rarely is, because the bug usually lives in an abstraction.

## See Also

- code/quality(softwaredev) — parent: measurable quality thresholds
- code/quality/versioning(softwaredev) — the `_v2` twin and why the name should say what the thing is
- code/overbuild(softwaredev) — the other side of scope: no capability nobody asked for
- code/testing/tdd(softwaredev) — the test that makes restructuring existing code safe
