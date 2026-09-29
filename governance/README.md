# Compliance Claims

<img src="../docs/images/lumon-hq.jpg" alt="The institutional perspective" width="100%" />

<sub>Someone decided what the handbooks should say. Someone decided which departments get which manuals.<br/>This is where those decisions are traceable.</sub>

---

## Getting Started

```bash
# Which ways carry a compliance claim
ways-audit report

# Trace one way's claims end-to-end
ways-audit trace softwaredev/code/security

# Which ways claim a control
ways-audit control OWASP
```

For adding claims to your own ways, see [provenance.md](../docs/hooks-and-ways/provenance.md).

> **Current state:** `ways-audit` builds its view in memory from the `provenance.yaml` sidecars on each run and prints to stdout. `ways-audit assemble --write` is the one command that writes: it appends finding records to `$XDG_STATE/agent-ways/findings.jsonl`.

---

The [main project](../README.md) manages what happens on the severed floor — how agents receive guidance, how teams coordinate, how context flows. This directory is concerned with the floor above: which external controls a way claims to address, which policy documents those claims derive from, and what evidence a session produced.

## The Problem It Solves

Every organization with AI agents faces the same question from compliance: *"How do you know your agents are following policy?"*

The usual answer involves expensive GRC platforms, manual attestation spreadsheets, or "we told them to." The GRC platform cannot look inside the agent's context window. The spreadsheet goes stale. "We told them to" does not pass audit.

This directory and `ways-audit` give a narrower answer. A way states which controls its guidance is designed to address. That statement is a **claim**, not evidence ([ADR-200](../docs/architecture/governance/ADR-200-compliance-claims-and-session-derived-findings.md)). Evidence comes from sessions: the firing log shows that the guidance reached the agent at the point of work, and a transcript shows what the work looked like. `ways-audit` assembles the claim and that evidence into a **finding** record for an assessor to judge. It does not certify conformance, and it is not a GRC platform; its output is input for one.

## How It Works

A way carries its claims in a `provenance.yaml` sidecar beside the way file:

```yaml
policy:
  - uri: governance/policies/code-lifecycle.md
    type: governance-doc
controls:
  - id: NIST SP 800-53 CM-3 (Configuration Change Control)
    justifications:
      - Conventional commit types (feat/fix/refactor) classify changes by nature
      - Atomic single-concern commits make each change independently reviewable
    satisfied_when: >
      Commits created in the session carry a conventional-commit type prefix,
      each covers a single logical change, and the message body states the
      rationale for the change.
verified: 2026-02-05
rationale: >
  Conventional commits create structured change records with type
  classification and justification.
```

The sidecar is never injected, so claims cost no tokens in the agent's context window. A way without a sidecar runs identically.

Each control carries two kinds of statement:

- **`justifications`** — how the guidance is designed to address the control. These are assertions.
- **`satisfied_when`** — the determination criterion: the session behavior that would let an assessor mark the control *satisfied* or *other than satisfied*. A claim without one cannot become an outcome finding.

## The Chain

```
Regulatory Framework    NIST SP 800-53, ISO 27001, OWASP, SOC 2...
       ↓
Control Requirement     CM-3: Configuration Change Control
       ↓
Policy Document         code-lifecycle.md: "Atomic commits, conventional format"
       ↓
Way + claim             delivery/commits: guidance, and provenance.yaml naming CM-3
       ↓
Agent Context           Guidance injected when a commit is being made
       ↓
Finding                 firing event + transcript, assessed against satisfied_when
```

Each layer above the way compresses the one above it. The regulatory framework is hundreds of pages, the control is a paragraph, the policy is a few pages, and the way is a short set of directives. The layers down to the claim are authored. The finding is the only layer that comes from a session.

## What's Here

| File | Purpose |
|------|---------|
| `policies/` | Policy source documents — the human-readable interpretation layer that claims point at |

The sidecars live beside the ways under `hooks/ways/`. The command surface is the `ways-audit` binary (`make ways-audit`).

### The compliance operator

`ways-audit` is run on purpose, by an operator; no hook or CI job runs it. Without `--global` it reads the project's `.claude/ways/` if one exists, and otherwise `~/.claude/hooks/ways/`.

```bash
# Claim coverage report
ways-audit report

# Trace one way end-to-end (controls + justifications + firing stats)
ways-audit trace softwaredev/code/security

# Which ways claim a control, or derive from a policy
ways-audit control OWASP
ways-audit policy code-lifecycle

# Ways without a claim, and claims whose verified date is older than 90 days
ways-audit gaps
ways-audit stale 90

# Cross-reference claims with firing counts from the event log
ways-audit active

# Flat traceability matrix: way | control | justification
ways-audit matrix

# Validate claims: controls present, justifications present, policy URIs resolve
# (relative URIs against $HOME/.claude/), verified dates well-formed
ways-audit lint

# Assemble finding records from claims + the firing log; --write appends them
ways-audit assemble --write
ways-audit findings

# Any mode outputs JSON with --json
ways-audit matrix --json
```

### Findings

`ways-audit assemble` builds one record per claimed control: the claim, its `satisfied_when` criterion, and the firing evidence for the way. It leaves the determination empty. Whether a control is satisfied is decided by a separate assessor, not by `ways-audit` ([ADR-201](../docs/architecture/governance/ADR-201-findings-assembled-as-classifier-ready-assessment-records.md)). The findings ledger is append-only; each `--write` adds a timestamped snapshot.

Read `ways-audit report` coverage as *claims made*, not *conformance achieved*.

## Real Standards

The claims on the built-in ways reference public standards:

| Way | Standards claimed |
|-----|-------------------|
| **delivery/commits** | NIST CM-3, SOC 2 CC8.1, ISO 27001 A.8.32 |
| **code/security** | OWASP A03, NIST IA-5, CIS Controls 16.12, SOC 2 CC6.1 |
| **code/quality** | ISO 25010, NIST SA-15, IEEE 730 |
| **meta/knowledge** | ISO 27001 5.2, NIST PL-2 |

An auditor familiar with NIST 800-53 can read the commits way's claim and follow the line from CM-3 through the policy document (`code-lifecycle.md`) to the guidance the agent receives. That line is a claim about design. Whether a given session followed it is what a finding records.

## Making This Its Own Repo

This directory is designed to be separable. To use it standalone:

1. Copy `governance/` to a new repo
2. Add `provenance.yaml` sidecars to your ways, with policy URIs pointing at your own policy documents
3. Run `ways-audit report` from a project whose `.claude/ways/` holds those ways, or `ways-audit --global report` for `~/.claude/hooks/ways/`
4. Run `ways-audit lint` to check that the policy URIs resolve. Lint resolves a relative URI against `$HOME/.claude/<uri>`, not against the copied repo, so the policy files must be reachable under `~/.claude/` at that path (URIs starting with `http` or `github://` are not checked)

The `ways-audit` binary is the only dependency.

Your compliance repo owns the policies. Your ways repo owns the guidance and the claims. This directory holds the policy layer the claims point at.

## Further Reading

- [ADR-200: Compliance claims and session-derived findings](../docs/architecture/governance/ADR-200-compliance-claims-and-session-derived-findings.md) — claims versus findings, and the scope of the compliance layer
- [ADR-201: Findings assembled as classifier-ready assessment records](../docs/architecture/governance/ADR-201-findings-assembled-as-classifier-ready-assessment-records.md) — the finding record and the assessor boundary
- [ADR-151: ways-core crate and ways-audit sibling binary](../docs/architecture/governance/ADR-151-extract-ways-core-crate-and-ways-audit-sibling-binary.md) — why `ways-audit` is a separate binary
- [Provenance documentation](../docs/hooks-and-ways/provenance.md) — the sidecar reference
- [The Cost of Bad Instructions](../docs/hooks-and-ways/rationale.md) — why this matters economically and environmentally
