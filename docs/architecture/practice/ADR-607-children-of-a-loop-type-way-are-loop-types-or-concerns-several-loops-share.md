---
contract: adr/v1
kind: decision
verb: change
capability: method
amends:
  - ADR-605#Decision
basis:
  - operator: aaronsb
    level: guided
    said: "I guess I'm ok with the ways as long as we keep them organized (so they can be managed and not mixed."
    via: chat, 2026-10-22
  - precedent: ADR-605
  - evidence: "hooks/ways/evaluative/held-out/held-out.md: opens 'This applies across loop types' and names the probe-set, agent-eval, rubric and forecast loops it serves; hooks/ways/evaluative/evaluative.md routes to it by the row 'The cases for any loop above'. ADR-605 Decision 3 lists it among the loop types while describing it as applying 'to the cases of any loop above'."
agent:
  name: claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Yes, that's it"
    via: "chat, AskUserQuestion, 2026-10-22, probe: organized"
  - operator: aaronsb
    said: "Rows in the table (Recommended)"
    via: "chat, AskUserQuestion, 2026-10-22, probe: table"
status: accepted
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-605
---

# ADR-607: Children of a loop-type way are loop types or concerns several loops share

## Summary

- **Decided:** under a parent way that routes by loop type, such as `evaluative`, a child is one of two kinds. It is a loop type, one per kind of output and judge, or a shared child: method that several loop types use and that no single loop way would reach. A shared child names the loops it serves, the loop ways link to it and do not restate it, and the parent's routing table gives it a row keyed by what it applies to. Method that arrives as its own body of work takes its own child of one of these kinds and is not mixed into an existing way's text. This amends ADR-605 Decision 3, which said every child is a loop type.
- **Trades away:** the routing table no longer answers one question. Most rows ask "what kind of loop is this", and the shared rows ask "what part of any loop is this", so a reader scans two kinds of row. Each shared child is also one more prompt-matched way that competes with the loops it serves.
- **One-way?** No. A shared child can be folded into its parent or into one loop way, and the rule restated, in one change.
- **Probes:** *Confident (organized):* method that came in on its own, such as error bars and pass-rate counting, stays in its own way next to the loops, so each way can be managed by itself and nothing gets mixed into a way that already existed. *Not confident (table):* the shared ways appear as rows in the `evaluative` routing table beside the loop types. You may prefer the table to list loop types only, with the shared ways reached through links from each loop.
- **Inversion:** between a tree where every child is a loop type and shared method is repeated or squeezed into the parent, and a tree split by cross-cutting principle that ADR-605 rejected. This takes a middle: loop types stay the main axis, and a concern earns a shared child only when several loops use it.

## Context

ADR-605 Decision 3 made each child of `evaluative` a loop type. The list already held one exception: `held-out` is about the cases every loop uses, not a loop of its own, and ADR-605 describes it that way while listing it as a loop type. A second concern of the same shape, the statistics behind any score over cases or trials, now has its own child, because the operator asked that separately contributed method be kept in ways of its own rather than mixed into existing ones. The rule in Decision 3 no longer describes the tree.

## Decision

This replaces the opening sentence of ADR-605 Decision 3 ("Each child is a loop type, one per kind of output and judge") and leaves its list of loop types as written.

1. A child of a parent way that routes by loop type is either:
   - a **loop type**, one per kind of output and judge; or
   - a **shared child**, holding method that several loop types use and that no single loop way would reach.
2. A shared child states near its opening which loops it serves. The loop ways link to it and do not restate it.
3. A shared child joins the parent's routing table as a row keyed by what it applies to, in the form "The cases for any loop above" or "The score of any loop above".
4. A shared child holds method only. A way of using one loop, such as tuning a skill round by round against cases, belongs to that loop or to one of its children.
5. A body of method that arrives on its own, from a contribution or a later round of work, takes its own child of one of these two kinds and is not merged into an existing way's text.
6. A child of a loop type or of a shared child refines its parent and is outside this rule.
7. `held-out` is a shared child under this rule.

## Consequences

### Positive

- The rule matches the tree: `held-out` no longer sits in a list of loop types it does not belong to.
- Method several loops need has one home, and each loop way reaches it by a link instead of carrying its own copy.
- Each way keeps one source and one subject, so it can be tuned, disabled or replaced without editing a way that already existed.

### Negative

- The routing table holds two kinds of row. A reader picks a loop by output and judge, then has to notice the shared rows apply on top of it.
- A shared child has to be told apart from a loop type in its wording and vocabulary, or it fires in place of the loop it serves.
- Whether a concern is used by "several" loops is a judgement call; a concern used by one loop belongs in that loop's subtree.

### Neutral

- The root way's routing table gains a row for each shared child; that is an edit to the way's body, made with the way.
- ADR-605 Decision 3's list of loop types stays as written, with `held-out` now read as a shared child.

## Alternatives Considered

- **Keep every child a loop type and put shared method in the root.** Rejected: the root would grow with each shared concern, and contributed method would be mixed into an existing way, against the operator's direction to keep ways organized and not mixed.
- **Repeat shared method in each loop way that needs it.** Rejected: the copies drift, and a fix lands in one loop and not the others.
- **A separate parent for shared concerns, beside `evaluative`.** Rejected for now: it splits one method across two trees for a small number of shared children. It is worth revisiting if shared children come to outnumber a few.
- **Children by cross-cutting principle.** Rejected in ADR-605 and not reopened: the main choice an agent makes is which loop to build.
