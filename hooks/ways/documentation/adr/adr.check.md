---
description: writing architecture decision record, documenting design choice, ADR context and consequences
vocabulary: adr decision record context consequences alternatives trade-off architectural choice
scope: agent
---

## anchor

You are writing an ADR. The value is in accurately capturing *why*, not just *what* — verify your understanding of the context.

## check

Before writing or updating this ADR:
- Have you **read the code** this decision affects, or are you writing from conversation context alone?
- Are the alternatives listed ones that were **actually considered**, or filler?
- Do the consequences reflect **real trade-offs**, not just "this is good / this could be bad"?
- Does an existing ADR already cover this decision? Check with `docs/scripts/adr list --group`.
- Is the context section capturing the **actual motivation** (who needs this, what constraint drove it)?
- Does this read as a **generalized decision** a future reader could apply to a different feature — or as a **trip report** of the exploration that birthed it? Lift the principle out; relegate the discovery to a one-line motivating example.
