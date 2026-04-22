---
description: project-specific gated configuration
vocabulary: gated project specific configuration
pattern: (?i)(code|project|gated|configuration|quality)
scope: agent
when:
  project: /tmp/test-project-sim
refire: 0.15
---
# Gated Way

Only fires when the project is /tmp/test-project-sim.
