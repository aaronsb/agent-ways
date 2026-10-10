---
description: planning how to implement a feature, deciding an approach, understanding existing project decisions, starting work on an item, investigating why something was built a certain way
vocabulary: plan approach debate implement build work pick understand investigate why how decision context tradeoff evaluate option consider scope superseding surveys selective
scope: agent, subagent
macro: prepend
requires: ["Read", "Bash(find:*)", "Bash(wc:*)"]
refire: 0.15
---
<!-- epistemic: convention -->
# ADR Context — Read Before You Build

Before diving into implementation, check if the project has Agent Decision Records that inform the work.

## Discovery

Use the ADR tool if installed (`docs/scripts/adr` or similar). Under adr/v1 (see the project's `docs/architecture/adr.yaml`), query rather than browse:

```
adr list --capability X      # records touching a capability
adr list --kind decision     # decisions only (also: spec, evidence)
adr list --field verb=cut    # any frontmatter field
adr list --group-by capability
adr view <N>                 # read a specific record
```

No `adr.yaml`, or no tool? The project is adr/v0 — check `docs/architecture/` for `ADR-*.md` files directly.

## Reading Strategy

**Read selectively, not exhaustively.**

- Identify 1-3 records most relevant to the current task
- Prioritize **accepted** status — those are active decisions
- On a decision record, read the **Summary** first (decided, trades away, one-way?) before the body
- Follow `supersedes`/`amends` links to the current version of a decision
- Evidence records (findings, surveys, audits) are background a decision's `basis` cites — read one when a decision points to it
- Don't bulk-read the entire corpus — it consumes context without payoff

## When to Check

- Starting work in an area that likely has existing decisions
- User asks "why is X done this way?"
- About to make a choice that might contradict or duplicate an existing decision
- Picking up work that references ADR numbers

## When to Skip

- Simple bug fixes with obvious patterns
- User already provided full context
- You've already read the relevant ADRs this session
