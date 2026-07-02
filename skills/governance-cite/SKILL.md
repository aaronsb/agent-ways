---
name: governance-cite
description: Look up governance controls and their justifications via the governance CLI, to ground a recommendation in a real standard (NIST, OWASP, ISO, SOC 2, CIS, IEEE). Use when you need the control IDs and justification text behind a practice. Not for deciding whether to cite or how to phrase it — that's the governance citation way.
allowed-tools: Bash, Read, Grep, Glob
---

# Governance Citation Lookup

A governance traceability system maps agent guidance (ways) to the regulatory
controls their guidance *claims* to address, each with asserted justifications —
control-*design* claims, not evidence the control operates. This skill is the
**lookup** — run it to fetch the control IDs and justification text behind a
practice. For *when*
to cite and *how* to phrase it, see the **governance citation** way.

## Look up controls

### By topic

```bash
ways governance control PATTERN          # e.g. NIST, OWASP, ISO, SOC, CIS, change
```

### By way (full trace)

```bash
ways governance trace softwaredev/commits
ways governance trace softwaredev/security
```

### Machine-readable

```bash
ways governance control PATTERN --json
ways governance trace WAY --json
ways governance matrix --json            # the complete traceability matrix
```

Read the governed ways and controls **from the data** — run `ways governance
matrix` for the current set. Never cite from memory; provenance changes.

## Not for

- Deciding *whether* to cite, or *how* to phrase a citation — that's the **governance citation** way.
- The full coverage/provenance report — that's the **governance** skill (`ways governance report`).
