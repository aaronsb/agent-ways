---
description: Development environment — configuration, dependencies, debugging, SSH, and build tooling
vocabulary: environment setup config configure dependency install package build tool debug troubleshoot ssh remote connect server dev local machine workspace
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Environment

Children of this way cover the development environment:

| Aspect | Way |
|--------|-----|
| Configuration, .env | `environment/config` |
| Dependencies, packages | `environment/deps` |
| Debugging | `environment/debugging` |
| SSH, remote access | `environment/ssh` |
| Makefile targets | `environment/makefile` |
| Container build/run safety | `environment/container-safety` |
| Authoring host versus target host | `environment/hostparity` |
| Awareness / attend loop | `environment/attend` |
| Recovering from a failed attempt | `environment/recovery` |
| Long, interactive, or never-returning commands | `environment/bounded-execution` |

## See Also

- environment/config(softwaredev) — configuration management
- environment/deps(softwaredev) — dependency management
- environment/debugging(softwaredev) — systematic debugging
- environment/ssh(softwaredev) — SSH and remote access
- environment/makefile(softwaredev) — Makefile targets and conventions
- environment/container-safety(softwaredev) — developer safety in container build/run definitions
- environment/hostparity(softwaredev) — a green result on the authoring host is evidence about the authoring host only
- environment/attend(softwaredev) — the attend awareness sensor loop
- environment/recovery(softwaredev) — classify a failure, take its one allowed move, escalate after three attempts
- environment/bounded-execution(softwaredev) — background the never-returning command, flag the interactive one, kill by pid; the guard hook refuses the rest
