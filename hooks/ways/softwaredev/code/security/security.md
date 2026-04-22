---
description: security overview, secure coding defaults, security review checklist
vocabulary: security vulnerability protect defense secure harden owasp
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: convention -->
# Security Way

## Defaults

- Parameterized queries for all database access
- Escape output for its context (HTML, URL, SQL)
- Validate at system boundaries (user input, external APIs)
- Principle of least privilege for permissions

## When Reviewing Existing Code

Flag these as security issues:
- Hardcoded secrets or credentials
- SQL string concatenation
- Unsanitized user input in templates or commands
- Missing authentication/authorization on endpoints
- Sensitive data in logs

## See Also

- code/security/auth(softwaredev) — authentication requirements
- code/security/injection(softwaredev) — injection prevention
- code/security/secrets(softwaredev) — credential management
- code/supplychain(softwaredev) — dependency security
