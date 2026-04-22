---
description: planning how to implement a feature, deciding an approach, understanding existing project decisions, starting work on an item, investigating why something was built a certain way
vocabulary: plan approach debate implement build work pick understand investigate why how decision context tradeoff evaluate option consider scope
scope: agent, subagent
macro: prepend
requires: ["Read", "Bash(find:*)", "Bash(wc:*)"]
refire: 0.15
---
<!-- epistemic: convention -->
# ADR Context — Read Before You Build

Before diving into implementation, check if the project has Architecture Decision Records that inform the work.

## Discovery

Use the ADR tool if installed (`docs/scripts/adr` or similar):

```
adr list --group     # see domains and decisions at a glance
adr view <N>         # read a specific ADR
```

No tool? Check `docs/architecture/` for `ADR-*.md` files directly.

## Reading Strategy

**Read selectively, not exhaustively.**

- Identify 1-3 ADRs most relevant to the current task
- Prioritize **Accepted** status — those are active decisions
- Read Context and Decision sections first; skip Alternatives unless debating a change
- Don't bulk-read the entire ADR corpus — it consumes context without payoff

## When to Check

- Starting work in an area that likely has existing decisions
- User asks "why is X done this way?"
- About to make a choice that might contradict or duplicate an existing decision
- Picking up work that references ADR numbers

## When to Skip

- Simple bug fixes with obvious patterns
- User already provided full context
- You've already read the relevant ADRs this session
