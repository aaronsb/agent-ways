---
scope: user
settings:
  permissions:
    allow: ["Bash(git:*)"]
  model: sonnet
---
# Base permissions + model
First fragment: git allowed, model sonnet. The next fragment adds to the allow
list (concat) and overrides the model (last wins).
