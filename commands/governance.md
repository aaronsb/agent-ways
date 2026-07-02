---
description: Run governance traceability report — provenance coverage, control queries, way traces
---

Run `ways-audit` with the user's arguments (if any) and display the output.

This is the governance operator. Common invocations:

- `ways-audit report` — coverage report (default)
- `ways-audit trace softwaredev/commits` — end-to-end trace for a way
- `ways-audit control NIST` — which ways implement controls matching "NIST"
- `ways-audit policy code-lifecycle` — which ways derive from a policy
- `ways-audit gaps` — list ways without provenance
- `ways-audit stale` — ways with stale verified dates
- `ways-audit active` — cross-reference provenance with way firing stats
- `ways-audit matrix` — flat traceability matrix (way | control | justification)
- `ways-audit lint` — validate provenance integrity
- Add `--json` to any mode for machine-readable output

If the user provides arguments after `/governance`, pass them through. If no arguments, run the default coverage report.
