---
scope: user
settings:
  permissions:
    allow: ["Bash(git:*)", "Bash(gh:*)"]
    deny: ["Bash(rm -rf *)"]
  includeCoAuthoredBy: false
---
# Git & GitHub permissions
Let Claude run git/gh unprompted — constant use, prompts are pure friction.
`rm -rf` stays denied: destructive, never worth auto-allowing.
