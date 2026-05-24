# IT Operations Ways

Guidance for incident response, operational policy, change proposals, and runbooks.

This domain can be disabled globally via `~/.claude/ways.json` when working across many projects that don't involve operational infrastructure. It's currently disabled by default. For per-project muting of a single way (e.g., just `itops/incident`), use `ways disable itops/incident` from the project root — see ADR-131.

## Incident Response

**Triggers**: Prompt mentions "incident response", "L0/L1/L2 support", "escalation", "MTTR", "mean time", "alert response", "triage", "remediation"

Defines a tiered support model where each level has clear autonomy boundaries:

| Tier | Autonomy | Scope |
|------|----------|-------|
| L0 | Automated responses, known runbooks | Predefined playbooks only |
| L1 | Basic troubleshooting, escalation | Single-service scope |
| L2 | Cross-service investigation | Multi-service correlation |
| DevOps | Infrastructure changes | Platform-level remediation |
| Senior | Architectural decisions | System-wide changes |

The key principle is **contextual escalation**: moving up a tier requires diagnostic evidence and a hypothesis, not just "it's broken." This prevents premature escalation (wasting senior time) and gives the next tier actionable context instead of starting from scratch.

The way also prescribes incident flow structure - detection, triage, investigation, remediation, postmortem - so that incidents follow a predictable process rather than ad-hoc firefighting.

## Policy Engine

**Triggers**: Prompt mentions "operation class", "policy engine", "enforcement", "approval gate", "blast radius", "risk class", "risk score"

Classifies operations by risk level and maps them to approval requirements:

| Class | Examples | Approval |
|-------|----------|----------|
| READ | Query data, list resources | None |
| WRITE_LOW | Update config, deploy to staging | Self-approve |
| WRITE_MEDIUM | Schema migration, production deploy | Peer review |
| WRITE_HIGH | Data migration, permission changes | Team lead |
| DESTRUCTIVE | Delete data, drop table, decommission | Director + backup verification |

Risk scoring factors:
- **Reversibility** - can this be undone? How quickly?
- **Blast radius** - how many users/services are affected?
- **Data impact** - is data being created, modified, or destroyed?

The purpose is to make risk assessment systematic rather than intuitive. "This feels risky" becomes "this is WRITE_HIGH because it's irreversible, affects all users, and modifies production data." The classification drives the approval requirement, not individual judgment.

## Proposals

**Triggers**: Prompt mentions "proposal primitive", "proposal lifecycle", "human in the loop", "approval workflow", "operation proposal"

Structures change proposals with required sections:

1. **What** - the specific change being proposed
2. **Why** - the problem it solves or improvement it delivers
3. **Risk** - what could go wrong, blast radius, rollback plan
4. **Expected Outcome** - measurable success criteria
5. **Safety** - pre-checks, monitoring, and rollback procedure

Proposals follow a lifecycle: CREATED → PENDING → APPROVED → EXECUTED → VERIFIED. Each transition has conditions - a proposal can't move to APPROVED without a reviewer, can't move to EXECUTED without approval at the appropriate level (per the policy engine classifications).

The human-in-the-loop principle is central: automated systems can create and execute proposals, but approval gates require human judgment for anything above WRITE_LOW.

## Runbooks

**Triggers**: Prompt mentions "runbook", "runbook automation", "playbook", "SOP automation", "operational procedure"

Key positions:

- **Check before creating** - always search for an existing runbook before writing ad-hoc scripts. Duplicated operational procedures diverge over time and cause confusion during incidents.
- **Executable, not narrative** - runbooks should be runnable, not just readable. Each step should be a command or a decision point, not a paragraph of prose.
- **Required properties** - every runbook procedure needs: error handling (what to do when a step fails), idempotence (safe to re-run), logging (what happened and when), and a separate README explaining purpose and prerequisites.
- **As-code** - runbooks belong in version control alongside the systems they operate. Changes to systems should include runbook updates in the same PR.
