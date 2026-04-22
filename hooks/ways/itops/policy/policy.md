---
description: Operation classification, policy enforcement, approval gates, blast radius assessment, and risk scoring
vocabulary: operation class policy enforcement approval gate workflow blast radius risk score level dangerous safe critical
pattern: operation.?class|policy.?(engine|enforcement)|approval.?(gate|level|workflow)|blast.?radius|risk.?(class|level|score)
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: constraint -->
# Policy Way

## Operation Classification

| Class | Examples | Default Approval |
|-------|----------|------------------|
| **READ** | Query status, get logs | Autonomous |
| **WRITE_LOW** | Update ticket, add comment | Autonomous + logging |
| **WRITE_MEDIUM** | Restart service, scale replicas | Notify |
| **WRITE_HIGH** | Deploy code, modify IAM | Explicit approval |
| **DESTRUCTIVE** | Delete data, terminate instances | Multi-party approval |

## Risk Scoring Factors

| Factor | Low | Medium | High |
|--------|-----|--------|------|
| **Reversibility** | Instant undo | Manual recovery | Unrecoverable |
| **Blast Radius** | Single resource | Service | Cross-system |
| **Data Impact** | No change | Metadata | User data |

## Approval Levels

| Level | Action | Human Involvement |
|-------|--------|-------------------|
| **Autonomous** | Execute immediately | Log only |
| **Notify** | Execute, then inform | Post-hoc awareness |
| **Recommend** | Propose, await approval | Explicit approval |
| **Escalate** | Block, page on-call | Senior approval |

## Circuit Breakers

| Control | Purpose | Typical Config |
|---------|---------|----------------|
| Error threshold | Stop on failure rate | 3 failures / 5 min |
| Rate limit | Prevent runaway | 10 WRITE ops / min |
| Cooldown | Space operations | 30s between restarts |

## See Also

- proposals(itops) — proposals implement policy gates
- incident(itops) — incidents may bypass normal policy
