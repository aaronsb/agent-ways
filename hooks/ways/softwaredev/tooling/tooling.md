---
description: building and maintaining a project's own CLI tooling — encoding repeated operations as commands rather than manual shell sequences
vocabulary: tool tooling cli script subcommand automate automation repeated manual incantation workflow efficiency wrapper helper scaffold makefile
pattern: build.?a.?(tool|script|cli)|subcommand|manual.?(step|process)|shell.?(script|incantation)|repeated.?(command|operation)
files: (scripts|tools|bin)/.*|Makefile$
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Project Tooling

**Build the tool, don't repeat the incantation.**

When an operation is done more than a couple of times, is error-prone by hand, or encodes rules a person would otherwise hold in their head, make it a **command**, not a manual shell sequence. A tool subcommand is discoverable, testable, and self-documenting; a remembered incantation is none of those and rots the moment the person who knew it moves on.

This is an efficiency principle, but the deeper payoff is that **rigor gets cheap**. A typed, validated, lint-enforced operation that would take a human minutes to perform carefully — and that they will therefore shortcut — costs an agent (or a human) one command once it lives in the tool. The maintenance burden a person would cap out on is exactly what a tool removes. So push the operation into the tool *on purpose*, past the point where a manual process would feel "good enough."

## When to reach for it

| Signal | Move |
|--------|------|
| You're about to write a multi-step `git`/`sed`/`mv` sequence a reader must get exactly right | Add a subcommand that does it atomically |
| The steps encode a convention (numbering, naming, a file layout) | The tool should own the convention, not a doc that humans follow by hand |
| You've done the same manual dance twice | The third time, build it — and backfill the first two |
| A step is reversible-only-with-care (renames, moves, history) | A command can do it safely (e.g. prefer the tool's `git mv` with a fallback) |

A concrete example from this corpus: renaming an ADR means editing its heading *and* moving its file *and* refreshing an index — a three-step manual dance that's easy to half-do. `adr rename` owns all three. The point isn't the command; it's that the operation now has one correct home instead of living in a person's memory.

## Don't over-build

The inverse failure is real: not every one-off needs a tool. If an operation runs
once and never recurs, a manual sequence is fine — wrapping it is its own waste.
The trigger is *repetition × risk*, not novelty. See `code/overbuild`.

## See Also

- code/overbuild(softwaredev) — the opposite failure: don't reinvent what already exists
- code/quality(softwaredev) — a validation/invariant earns its place only if its violation is a real defect
- adr(documentation) — the ADR tool is a worked example of operation-owning tooling
