---
description: threat modeling, STRIDE analysis, trust boundaries, attack surface assessment, security design review
vocabulary: threat model stride attack surface trust boundary mitigation adversary dread spoofing tampering repudiation elevation
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Threat Modeling Way

Threat modeling is security at design altitude. Where the Security Way catches code-level issues (injection, exposed secrets), this way maps adversaries, trust boundaries, and systemic risks.

## When to Threat Model

- New service or component with external-facing surface
- Authentication/authorization redesign
- Data flow changes crossing trust boundaries
- Third-party integration adding new attack vectors

## STRIDE Framework

Analyze each component interaction for:

| Threat | Question | Mitigation Pattern |
|--------|----------|--------------------|
| **S**poofing | Can an attacker impersonate a user or service? | Authentication, mutual TLS, signed tokens |
| **T**ampering | Can data be modified in transit or at rest? | Integrity checks, HMAC, immutable logs |
| **R**epudiation | Can actions be denied after the fact? | Audit trails, signed events, timestamps |
| **I**nformation Disclosure | Can sensitive data leak? | Encryption, access controls, data classification |
| **D**enial of Service | Can availability be degraded? | Rate limiting, circuit breakers, redundancy |
| **E**levation of Privilege | Can an attacker gain higher access? | Least privilege, role separation, input validation |

## Risk Register

Document accepted risks with expiration — risks don't stay accepted forever.

```markdown
| Risk | Likelihood | Impact | Mitigation | Status | Expires |
|------|-----------|--------|------------|--------|---------|
| API rate limiting absent | Medium | High | Planned for Q2 | Accepted | 2026-06-01 |
```

Expired accepted risks must be re-evaluated or mitigated.

## Trust Boundaries

Identify where data crosses trust levels:
- Browser to API gateway (untrusted → semi-trusted)
- API to internal service (semi-trusted → trusted)
- Service to third-party API (trusted → external)

Each boundary crossing needs: authentication, input validation, output encoding.

## Relationship to Security Way

- **Threat modeling**: "What could go wrong?" (design phase)
- **Security Way**: "Is this code safe?" (implementation phase)

Both may fire on security-related prompts. Threat modeling adds the systemic view.
