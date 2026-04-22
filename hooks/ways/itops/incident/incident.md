---
description: Incident response tiers, escalation paths, MTTR targets, alert triage, and remediation workflows
vocabulary: incident response escalation support tier l0 l1 l2 mttr mean time alert triage remediate on-call outage severity page production down broken
pattern: incident.?response|l0.?support|l1.?support|l2.?support|escalat|mttr|mean.?time|alert.?(response|triage)|remediat
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

