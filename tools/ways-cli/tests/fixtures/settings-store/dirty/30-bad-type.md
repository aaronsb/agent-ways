---
scope: user
settings:
  cleanupPeriodDays: "soon"
---
# Wrong type
`cleanupPeriodDays` expects a number; a string is a schema ERROR — this is the
class of mistake the console's raw textarea silently accepts.
