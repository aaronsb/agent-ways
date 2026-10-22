---
tool_name: ^ExitPlanMode$
tool_event: [PostToolUse]
when:
  file_exists: spe/README.md
scope: agent
refire: 0.15
---
# Plan Approved

Save the approved plan and its prediction before the eval runs.
