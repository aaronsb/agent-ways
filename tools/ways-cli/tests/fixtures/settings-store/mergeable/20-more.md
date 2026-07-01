---
scope: user
settings:
  permissions:
    allow: ["Bash(gh:*)"]
  model: opus
  env:
    FOO: bar
---
# More permissions + overrides
Adds gh to the allow list (concatenates with 10), overrides model to opus, and
introduces an env block (deep-merge).
