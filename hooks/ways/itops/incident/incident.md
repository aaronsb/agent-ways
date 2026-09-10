---
description: production incidents — something is down or broken, alert triage, escalation tiers and MTTR targets, fixing forward versus rolling back, and what has to exist before an incident closes
vocabulary: incident outage production down broken failing alert page on-call escalation severity triage tier l0 l1 l2 mttr remediate fix forward roll back restore contain regression postmortem closure prevention hazard residual
pattern: incident.?response|l0.?support|l1.?support|l2.?support|escalat|mean.?time|alert.?(response|triage)|remediat|fix.?forward|post.?mortem|on.?call
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: heuristic -->
# Incident Response Way

## Support Tiers

| Tier | Domain | Autonomy | Example |
|------|--------|----------|---------|
| **L0** | End-user IT | High (act + notify) | Account unlock, password reset |
| **L1/L2** | Service ops | Medium (known patterns) | Service restart, log analysis |
| **DevOps/SRE** | Infrastructure | Low (propose + approve) | IaC changes, capacity scaling |
| **Senior** | Architecture | Advisory only | Migration planning |

## Incident Flow

```
Trigger → Diagnose → Remediate → Verify → Close
                ↓
           Escalate (if needed)
```

## Contextual Escalation

When escalating, provide:
- Original request/alert
- Diagnostic steps taken
- Evidence collected (logs, metrics)
- Hypotheses considered
- Why escalation needed

**Bad**: "User can't connect to VPN"
**Good**: "User locked after 5 failed attempts. No password change. No security alerts. Unlocked account - user should retry in 2 min."

**Related**: Policy Way (operation classification, approval levels), Proposals Way (structured approval requests).

## L0 Example (VPN Failure)

1. Query AD → Account locked
2. Query VPN logs → 5 failed attempts
3. Check password changes → None recent
4. Check security alerts → Clean
5. **Autonomous action**: Unlock account
6. Respond with context and next steps

## Fix Forward

The first move on a failure is the smallest forward fix or the smallest reversible containment. Classify the fault before reversing anything. A configuration fault takes the non-destructive path. A data restore is a separate, separately approved last resort. Do not restore a database to fix a configuration problem. When reversal is chosen, record why and who approved it. A containment that leaves the improper state standing stays open with an owner.

## Closure

An incident closes with three artifacts: a regression test that reproduces the failure, a gate where one would have caught it, and a prevention rule recorded where the system keeps its durable rules. The narrative is the least durable part of the record and never counts as closure. A hazard seen twice is recorded as a fact with an owner, and the earlier one-off note is retired. Findings outside the incident's scope become named residual issues with an owner and the condition that reopens them.

## See Also

- delivery/issues(softwaredev) — residuals with an owner and a reopen condition
- delivery/release(softwaredev) — the rehearsed rollback a reversal relies on
