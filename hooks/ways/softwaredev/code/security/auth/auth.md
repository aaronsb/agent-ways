---
description: authentication, authorization, access control, middleware guards, RBAC, permissions
vocabulary: authentication authorization middleware guard permission role rbac access control login session jwt csrf cors
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: constraint -->
# Auth Way

## Endpoint Protection

Every endpoint that accesses user data or performs mutations must have an authentication check. Missing auth is a security issue — flag it.

## Access Control

- **Least privilege** — grant the minimum permissions needed
- **Role-based** — prefer RBAC over ad-hoc permission checks
- **Middleware/guards** — enforce auth at the routing layer, not inside handlers
- **Fail closed** — if the auth check fails or errors, deny access

## Common Gaps

- Admin endpoints without auth middleware
- API routes that check authentication but not authorization (user A accessing user B's data)
- Frontend-only auth checks without server-side enforcement
- Missing CSRF protection on state-changing endpoints
