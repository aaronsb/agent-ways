---
description: verify a container build/run definition affords developer safety before writing or running it
vocabulary: dockerfile containerfile compose docker build docker run podman build bind mount volume root user uid gid privileged capability artifact ownership
files: Dockerfile|Containerfile|(docker-)?compose\.ya?ml|\.devcontainer
commands: (docker|podman|nerdctl|buildah)\ (build|run|compose)
scope: agent, subagent
---
## anchor
A container build/run definition is a developer-safety surface. Root in the container plus root-owned artifacts on a bind mount widens the blast radius of ordinary mistakes on the local machine.

## check
Before writing this definition or running this build:
- Does the build/run stage set a **non-root `USER`**, or is it silently running as root?
- Will artifacts land on the host owned by **root**? Match the container uid/gid to the host user so cleanup never needs `sudo`.
- Any `--privileged`, extra capabilities, or a **docker-socket bind mount** this build doesn't actually need?
- Is the bind mount scoped to the **narrowest path** (and read-only where it can be) — not `$HOME` or `/`?

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "It's just a local build, root is fine" | Local is exactly where root-owned artifacts turn routine cleanup into a `sudo rm` — the dangerous case. |
| "Setting up a user is extra work" | `--user $(id -u):$(id -g)` at run time is one flag and needs no image change. |
| "The base image already runs as root" | That's the default, not a decision. Override it — add a `USER` or pass `--user`. |
