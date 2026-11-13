---
description: Agent Decision Records (ADRs) — creating, managing, and referencing ADRs for technical choices, how reversible a decision is, a deliberate deviation from a standard, and superseding an accepted ADR
vocabulary: adr architecture decision record design pattern technical choice trade-off rationale alternative reversibility reversible one-way irreversible deviation deviate depart standard exception waiver supersede superseded accepted defer as-built alternatives deferral vendoring
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

The section above this way, when present, gives this project's ADR commands, record format and lifecycle. It depends on the tool the project vendored and the contract its `adr.yaml` declares. Follow it over any habit from another project.

`adr contract` shows the contract `adr.yaml` declares and the one the tool writes. When `adr.yaml` is behind, `adr contract --upgrade` brings it to the tool's contract. Use it rather than editing the contract line by hand.

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
- **An as-built observation does not qualify.** A detail reconstructed from source with no recorded rationale goes in an evidence record, a design note or a README, marked "rationale not recorded".
- **A deliberate deviation qualifies.** When the project departs from a standard on purpose, the ADR names the standard, the reason, the scope (paths, environments, components), and the condition that ends it. Without the end condition a later session reads the departure as drift and fixes it.

## Reversibility

Tag every ADR with one of three grades: reversible (changed in one session, no data migration), expensive (a multi-day project), one-way (a rewrite or a migration on live data). Let the tag graduate when the cost changes at a known point: "reversible now, expensive after the first production import". A one-way decision gets the fullest Alternatives section, with a concrete reason each alternative lost.

## Citing Records

Records ship with the codebase and rarely with the software built from it. Where a citation goes depends on whether its reader can open the record.

- **The text stands on its own.** A comment, page, message or way says what the thing does and why in its own words. A reader should never have to open a record to finish a sentence. When you move a citation out and the sentence stops making sense, read the record and write the explanation it was standing in for.
- **In text that stays with the repository, cite at the end.** In a code comment, a `References:` line opens a block that runs to the end of the comment. In a docs page, a `References` heading opens a section. Each entry names the record and says what it decides for the code or page above it.
- **Text that ships carries no record numbers.** Program output, error and lint messages, TUI labels, hook messages and way bodies reach readers who cannot open the record. In a tool copied into other projects, the same number names a different record. Say what the record decides. A way that implements a decision names the record in its provenance sidecar as a `policy` entry with `type: adr`.

```rust
/// Retries the display handoff once a second while this terminal is shown.
/// Another program may release the display at any moment and nobody may
/// press a key, so the retry runs on its own instead of waiting for input.
///
/// References:
/// - ADR-1000, "Replies and policy": the slow retry and its pace.
/// - ADR-1400 D1: why the terminal needs display ownership to draw.
```

`adr cite` checks where each citation sits. `cite.placement` in `adr.yaml` sets the strength (`error`, `warn` or `off`; unset reads as `warn`), and `cite.shipped` lists paths whose text ships. `adr cite --check --since <ref>` holds only the lines changed since `<ref>` to the rule, so a project with old citations can enforce it on new text first.

## Fixing ADR Issues

**Run `adr lint` before editing ADR files.** The linter names what is wrong: missing or unclosed frontmatter, invalid fields, dangling or one-sided links, and under adr/v1 the contract's own rules. Fix what it reports rather than opening files and guessing.

## See Also

- adr/consider(documentation) — handing a decision to the operator, their answer, and raising concerns
- adr/figure(documentation) — a record that decides something a reader sees carries a figure, paired with it as published
- adr-context(documentation) — read existing ADRs before building
- adr/migration(documentation) — adopting ADR tooling in existing projects
- delivery/implement(softwaredev) — ADRs feed implementation planning
- writing/plain(writing) — plain sentences and terms defined on first use in a record
