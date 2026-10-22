---
tools: ^ExitPlanMode$
tool_phase: post
when:
  file_exists: spe/README.md
scope: agent
refire: 0.15
---
# Plan Approved

Save the approved plan and its prediction before the eval runs.
