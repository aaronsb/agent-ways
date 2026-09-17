---
status: Accepted
date: 2026-09-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-111
  - ADR-184
---

# ADR-185: CLI output contract: structured output for people, JSON for machines

## Context

`ways config show` prints a Rust debug rendering of the config struct. Three of the binary's forty verbs take `--json`. `attend` renders tables through `agent-fmt` while `ways` prints ad hoc lines. ADR-111 chose the argument parser and said nothing about output, so every verb has decided for itself.

ADR-184 adds verbs whose output an operator reads to decide what to activate, and whose output a script reads to decide what to do next. Both readers need a stable form.

## Decision

**Every verb in `ways` and `attend` renders for a person by default, through `agent-fmt`, and renders JSON under `--json`. A debug rendering never reaches stdout.**

1. **Human output** is structured: tables for lists, labeled rows for records, color where the terminal supports it and none where it does not. The formatter already in the workspace is the one renderer.

2. **`--json`** emits one document on stdout and nothing else there. Diagnostics go to stderr. The document's shape is the verb's contract, and a change to it is a change to the verb.

3. **Two views where a resolved value differs from a stored one.** `--json` emits what the file says. `--json --effective` emits the resolved state with defaults applied. Only the first is accepted back by an `apply` verb. Round-tripping the effective view would freeze every default at its current value.

4. **Round trip.** A verb that shows a configuration object has a partner that accepts the same JSON back. The pair is the test: show, apply, show again, byte-equal.

5. **Exit codes carry the verdict.** Zero for done, one for a failure the verb reports, two for bad usage. A verb that refuses to act, such as reconcile at a real directory, uses a code the caller can distinguish from failure.

Reversibility: reversible. Each verb converts on its own; a verb not yet converted is a defect against this contract, and nothing depends on the order.

## Consequences

### Positive

- An operator reads a table; a script parses a document. Neither has to parse the other's form.
- The bootstrap in ADR-184 shows the plan per target in the same renderer attend uses, which people already read.
- Round-trip pairs make configuration editable by tools without hand-editing YAML.

### Negative

- Forty verbs, three converted. The sweep is spread over ordinary work, and the contract holds before the sweep completes.
- Effective-versus-stored is one more flag to explain.

### Neutral

- `agent-fmt` becomes a dependency of every output path in `ways`, as it already is in `attend`.
- The config verbs in ADR-184 are the first converted, since they are new.

## Alternatives Considered

- **JSON by default, human on a flag.** Rejected: the first reader of every verb is the operator at the terminal, and a guiding Claude reading the output prefers the labeled form too.
- **A third format such as YAML for round trips.** Rejected: one machine format keeps the pair test simple, and the apply verb writes the config file's YAML.
- **Leave output to each verb.** The status quo. Rejected by the defect that opened this decision.
