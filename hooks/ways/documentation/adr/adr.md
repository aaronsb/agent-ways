---
description: Architecture Decision Records — creating, managing, and referencing ADRs for technical choices, how reversible a decision is, a deliberate deviation from a standard, and superseding an accepted ADR
vocabulary: adr architecture decision record design pattern technical choice trade-off rationale alternative reversibility reversible one-way irreversible deviation deviate depart standard exception waiver supersede superseded accepted defer
pattern: (^| )adr( |$)|architect|decision|design.?pattern|technical.?choice|trade.?off
files: docs/architecture/.*\.md$
macro: prepend
scope: agent, subagent
requires: ["Bash(chmod:*)", "Bash(cp:*)", "Bash(mkdir:*)", "Bash(touch:*)"]
refire: 0.15
---
<!-- epistemic: convention -->
# ADR Way

## When to Write an ADR
- Architectural choices (databases, frameworks, patterns)
- Technical approaches with trade-offs
- Process or methodology changes
- Security or performance decisions
- Anything you'll need to remember "why we did it this way"

## Commands, Format and Lifecycle

The section above this way, when present, gives this project's ADR commands, record format and lifecycle. It depends on the tool the project vendored and the contract its `adr.yaml` declares (ADR-304 §10). Follow it over any habit from another project.

Projects define their own domains and ranges in `adr.yaml`; `adr domains` shows them. A record's number is permanent. `adr domain add`, `rename` and `move` change the layout and rewrite every path to what moved, and they leave `ADR-N` citations as they are.

## Generalize the Decision

An ADR records the **durable principle**. The discovery path that led to it stays out. After a long exploration the draft wants to accrete the session's specifics — the exact feature, the messy inversion, the trip-report of what you tried. Strip them. The ADR is the *reusable decision*; the exploration stays in the session.

The test: a future reader who never saw your session should be able to apply this decision to a *different* feature. If the ADR only makes sense once you know the story that birthed it, it's a trip report.

- **State the principle** at the altitude where it generalizes — "a unit of work must do bounded work per cycle," not "the poller-wrapped-in-attend-wrapped-in-Monitor stack needs a bound."
- **Relegate the discovery** to at most a brief motivating example in Context — one or two sentences naming what surfaced the need. Leave the debugging replay out.
- **Decision and Consequences carry no session-specifics.** If a sentence only parses with this week's feature in mind, lift it to the general form or cut it.

A generalized ADR is reusable across everything that hits the same force. A discovery-laden one is single-use.

## What Counts as a Decision

- **Doing nothing is a decision** when the alternatives were live. Record the destination, the trigger that starts the work, and what makes waiting safe.
- **An as-built observation does not qualify.** A detail reconstructed from source with no recorded rationale goes in a design note or README, marked "rationale not recorded".
- **A deliberate deviation qualifies.** When the project departs from a standard on purpose, the ADR names the standard, the reason, the scope (paths, environments, components), and the condition that ends it. Without the end condition a later session reads the departure as drift and fixes it.

## Reversibility

Tag every ADR with one of three grades: reversible (changed in one session, no data migration), expensive (a multi-day project), one-way (a rewrite or a migration on live data). Let the tag graduate when the cost changes at a known point: "reversible now, expensive after the first production import". A one-way decision gets the fullest Alternatives section, with a concrete reason each alternative lost.

## Fixing ADR Issues

**Run `adr lint` before editing ADR files.** The linter names what is wrong: missing or unclosed frontmatter, invalid fields, dangling or one-sided links, and under adr/v1 the contract's own rules. Fix what it reports rather than opening files and guessing.

## See Also

- adr/consider(documentation) — handing a decision to the operator, their answer, and raising concerns
- adr-context(documentation) — read existing ADRs before building
- adr/migration(documentation) — adopting ADR tooling in existing projects
- delivery/implement(softwaredev) — ADRs feed implementation planning
