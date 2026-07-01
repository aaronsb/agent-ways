---
scope: user
settings:
  includeCoAuthoredBy: true
---
# Duplicate scalar
`includeCoAuthoredBy` is also set in 10-permissions.md. Two fragments setting
the same scalar is a WARNING: last wins by filename order, silently dropping
the earlier value.
