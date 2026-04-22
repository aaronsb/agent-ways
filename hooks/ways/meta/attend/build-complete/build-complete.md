---
description: Guide response to a completed background build
trigger:
  type: attend
  signals:
    - build-complete
scope: agent
refire: 0.15
---

A background build has finished. Consider:

1. **Check the result** — did it pass or fail? Look at the exit code and any warnings
2. **Run tests** if the build succeeded and tests are relevant to current work
3. **Note warnings** worth investigating — new warnings from your changes are signal
4. **Resume blocked work** — if you were waiting on this build, pick up where you left off

Only engage if the build is relevant to current work. The user may already be
aware via terminal output.
