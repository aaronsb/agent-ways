---
scope: user
settings:
  allowManagedHooksOnly: true
---
# Managed-only key at the wrong scope
`allowManagedHooksOnly` only works at managed scope. Authored here at user
scope Claude Code ignores it — a scope-legal ERROR.
