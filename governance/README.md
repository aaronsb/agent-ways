# Compliance Claims

<img src="../docs/images/lumon-hq.jpg" alt="The institutional perspective" width="100%" />

<sub>Someone decided what the handbooks should say. Someone decided which departments get which manuals.<br/>This is where those decisions are traceable.</sub>

---

The [main project](../README.md) manages what happens on the severed floor: how agents receive guidance, how teams coordinate, how context flows. This directory is concerned with the floor above: which external controls a way claims to address, which policy documents those claims derive from, and what evidence a session produced.

## The problem it solves

Every organization with AI agents faces the same question from compliance: *"How do you know your agents are following policy?"*

The usual answer involves expensive GRC platforms, manual attestation spreadsheets, or "we told them to." The GRC platform cannot look inside the agent's context window. The spreadsheet goes stale. "We told them to" does not pass audit.

agent-ways gives a narrower answer. A way states which controls its guidance is designed to address. That statement is a **claim**, not evidence ([ADR-200](../docs/architecture/governance/ADR-200-compliance-claims-and-session-derived-findings.md)). Evidence comes from sessions: the event log shows that the guidance reached the agent at the point of work, and a transcript shows what the work looked like. `ways-audit` assembles the claim and that evidence into a **finding** record for an assessor to judge. It does not certify conformance, and it is not a GRC platform; its output is input for one.

## Where to go

| To | Read |
|----|------|
| Add a claim to a way | [Adding a compliance claim](../docs/hooks-and-ways/provenance.md) |
| Look up a claim field or a `ways-audit` command | [Governance & compliance traceability](../docs/governance.md) |
| Understand claims versus findings | [ADR-200](../docs/architecture/governance/ADR-200-compliance-claims-and-session-derived-findings.md), [ADR-201](../docs/architecture/governance/ADR-201-findings-assembled-as-classifier-ready-assessment-records.md) |
| See why `ways-audit` is its own binary | [ADR-151](../docs/architecture/governance/ADR-151-extract-ways-core-crate-and-ways-audit-sibling-binary.md) |

```bash
ways-audit --global report                              # which shipped ways carry a claim
ways-audit --global trace softwaredev/delivery/commits  # one way's chain, end to end
```

## What's here

`policies/` holds the policy documents that claims point at: the human-readable layer between a control framework and a way. The claims themselves are `provenance.yaml` files beside the ways under `hooks/ways/`.

## Real standards

The claims on the built-in ways reference public standards:

| Way | Standards claimed |
|-----|-------------------|
| **delivery/commits** | NIST CM-3, SOC 2 CC8.1, ISO 27001 A.8.32 |
| **code/security** | OWASP A03, NIST IA-5, CIS Controls 16.12, SOC 2 CC6.1 |
| **code/quality** | ISO 25010, NIST SA-15, IEEE 730 |
| **meta/knowledge** | ISO 27001 5.2, NIST PL-2 |

An auditor familiar with NIST 800-53 can read the commits way's claim and follow the line from CM-3 through the policy document (`code-lifecycle.md`) to the guidance the agent receives. That line is a claim about design. Whether a given session followed it is what a finding records.

## Making this its own repo

This directory is separable:

1. Copy `governance/` to a new repository.
2. Add `provenance.yaml` files to your ways, with policy URIs pointing at your own policy documents.
3. Run `ways-audit report` from a project whose `.claude/ways/` holds those ways.
4. Run `ways-audit lint` from the compliance repository's root, so relative policy URIs resolve against it.

The `ways-audit` binary is the only dependency. Your compliance repo owns the policies; your ways own the guidance and the claims.
