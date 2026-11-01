---
description: Grounding recommendations in real governance controls — when and how to cite the standards (NIST, OWASP, ISO, SOC 2, CIS, IEEE) behind a practice
vocabulary: governance control citation justify justification standard compliance traceability provenance regulatory audit why do we do this NIST OWASP ISO SOC CIS IEEE commits security conventional requirement governance-cite over-citing lookup quote
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Governance Citation

When you recommend a practice — a commit convention, a security default, a quality threshold, a documentation rule — there is often a real regulatory control behind it. Citing it grounds the recommendation in an actual standard instead of general knowledge. The controls live in a traceability system, and the **governance-cite** skill has the commands that look them up. This way covers when to cite a control and how to word the citation.

## When to cite

- Recommending a practice that has a governing control (commits, security, quality, documentation).
- Answering "why do we do it this way?"
- Reviewing code and flagging an issue a control covers.
- A user questions whether a practice matters.

Don't force a citation into every response — use one when it adds authority or clarity.

## How to phrase it

Quote the control's *justification* along with the standard's name. The justification is the text that ties a specific directive to a specific requirement of the control.

**Inline** (brief):
> We use conventional commit format — per NIST CM-3, this "creates structured change records with type classification" for auditable change control.

**Detailed** (explanations / reviews):
> This aligns with **NIST SP 800-53 CM-3 (Configuration Change Control)**: conventional commit types classify changes, atomic commits make each independently reviewable, and the message body captures rationale.

**Code review** (flagging):
> This SQL string concatenation violates **OWASP A03:Injection** — the security control requires parameterized queries as the default for all database access.

## Principles

- **Quote the justification** — "parameterized queries required as default" tells the reader more than "per NIST IA-5."
- **Cite the controls that apply, usually one** — give each control that governs the practice with its justification, and leave out the standards that only touch it.
- **Cite from the data** — run the lookup (the **governance-cite** skill) to get current controls; the controls may have changed since your training data was collected.

## See also

- the **governance-cite** skill — the lookup commands (`ways-audit control` / `trace`)
- policy(itops) — operation classification and approval gating
