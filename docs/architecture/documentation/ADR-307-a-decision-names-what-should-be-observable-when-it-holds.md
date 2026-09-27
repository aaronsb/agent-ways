---
contract: adr/v1
kind: decision
verb: change
capability: adr
extends: [ADR-304]
amends: [ADR-304#4]
basis:
  - operator: aaronsb
    level: guided
    said: "experiencing the phenomenonolgy of software is a missing piece that I think an agent decision record can promote."
    via: session 2026-09-27
  - operator: aaronsb
    level: guided
    said: "A complex flow could even demonstrate the phenomenon of the work itself as one modal in the flow, if it's possible to do so"
    via: session 2026-09-27
  - operator: aaronsb
    level: guided
    said: "Option one but looser because there's so many possible variations depending on the work at hand"
    via: "session 2026-09-27, on the shape of an observable; option one, written by the agent, was a plain-words `see` with an optional `run` command"
  - operator: aaronsb
    level: guided
    said: "Optional everywhere"
    via: "session 2026-09-27, selected from agent-written options on which decisions must name an observable"
  - operator: aaronsb
    level: guided
    said: "No, `via` covers it"
    via: "session 2026-09-27, selected from agent-written options on whether considered records what was seen"
  - operator: aaronsb
    level: guided
    said: "Not yet (Recommended)"
    via: "session 2026-09-27, selected from agent-written options on whether the adr tool runs observable commands"
  - evidence: "an analysis of 121 vendor documents across four agentic tools found that the platform records events and nothing records the verification duty discharged (arXiv 2608.15678, Aug 2026)"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "It has to be flexible; we are asking for a way of observing function which is not predictable."
    via: session 2026-09-27, answering the probes on PR #596
    covers: [flexible-shape]
  - operator: aaronsb
    said: "I think an ask for an observable is fair. This way the operator could decline or just tell the agent \"observe it yourself, you can loop and iterate\" - don't use that verbatim but that is adjacentto the develop skill"
    via: session 2026-09-27, answering the probes on PR #596
    covers: [optional-unused]
  - operator: aaronsb
    said: "I like the shape of the pr it tracks the minimum needed to state something was actually real. Let's accept and continue"
    via: session 2026-09-27, PR #596
    covers: []
status: accepted
date: 2026-09-27
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 306
---

# ADR-307: A decision names what should be observable when it holds

## Summary

- **Decided:** a decision may carry `observable`: what someone should be able to see, run or try when the decision holds. Its shape is loose, because the work varies. It is optional on every decision, and it may be added or refined after acceptance. When a decision is handed to the operator, the agent demonstrates its observables, where that is possible, before asking.
- **Trades away:** a checkable form. A loose field can't be verified by the tool, so an observable is only as good as its author makes it.
- **One-way?** No. The field is optional, so removing it later changes no record's validity.
- **Probes:** *Confident (flexible-shape):* a free-form list fits the range of work, from a command's output to a page to click through. *Not confident (optional-unused):* whether an optional field gets used at all, or whether the handover guidance alone carries it.
- **Inversion:** at one end the record is prose to be read, and consideration rests on reading. At the other end every decision carries a runnable check the tool enforces, which fits commands and misses everything seen by eye. This decision names what to observe, leaves its form open, and puts the demonstration in the conversation.

## Context

`considered` records the operator's words on a decision, but nothing ties those words to having seen the work. Reading a record is reading about the act. Experiencing what the software does is the missing piece, and a decision record can promote it by saying what should be observable once the decision holds.

`cut` and `retire` decisions already have an observable end in `enacted`, the commit where the removal landed. `add` and `change` decisions say what was decided, but not how anyone would see that it holds.

## Decision

### 1. The observable field

A decision may carry `observable`, a list. Each entry is either a line of plain words or a mapping whose keys the author chooses to suit the work:

```yaml
observable:
  - "tier 2 adr-migrate passes 10 of 10"
  - see: a record scanned and applied comes back byte-identical
    run: bash tests/adr-import-roundtrip.sh
  - see: the evidence page renders the findings table
    url: https://claude.ai/artifact/…
```

`see` and `run` are conventions, not requirements. A screenshot path, a URL, a scenario name or a step-by-step description are equally valid. Lint checks only that `observable` is a list of strings or mappings.

### 2. Optional, and open after acceptance

No decision is required to carry an observable. `observable` joins the fields that may change after a decision leaves proposed (ADR-304 §4, `mutable_after_accept`), because what shows a decision holding often becomes clear only once it is built.

### 3. Asked for when drafting, demonstrated in the handover

When an agent drafts an `add` or `change` decision, it asks the operator what should be observable once the decision holds. The operator can name an observable, decline, or hand the observing to the agent. In the last case the agent works out what to observe, runs the work and iterates until it can show the outcome, as the develop loop does, and then writes the observable it used into the record.

When a decision with observables is handed to the operator, the agent demonstrates them, where possible, as one step of the flow: it runs the command, shows the output or a screenshot, or opens the page. It asks its questions afterwards. The consider way and the choices way carry this guidance. `considered.via` says what the operator was shown. No separate field records it.

### 4. The tool does not run observables

`adr` does not execute `run` entries. The agent runs them during the handover, or while iterating on the work (§3). A command runner in the tool can be decided later, once there is evidence of how observables are written.

## Consequences

### Positive

- A decision can say how anyone, human or agent, would see it holding, in whatever form suits the work.
- A consideration made after a demonstration differs, in its `via`, from one made after reading.

### Negative

- Nothing enforces that an observable exists, or that it works.
- A loose field is harder to use mechanically later, for a runner or a report.

### Neutral

- Imported records carry no observables until someone adds them, which §2 allows.
- `enacted` stays as it is. It is the observable end of a cut or retire.

## Alternatives Considered

- **A fixed shape: `see` plus an optional `run`.** Rejected by the operator as too narrow for the variety of work.
- **Required on add and change, with a lint warning.** Rejected in favour of optional everywhere. The agent asks when drafting (§3), and the field itself stays optional.
- **A `seen` list on `considered`.** Rejected: `via` already says what the operator was shown.
- **`adr observe N` running a decision's commands.** Deferred until observables have been written in practice.
