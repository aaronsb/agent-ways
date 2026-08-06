---
description: developer safety when a build or task runs inside a container — non-root execution, host artifact ownership, least privilege, scoped bind mounts
vocabulary: container docker podman buildah nerdctl dockerfile containerfile compose image build stage bind mount volume rootless non-root user uid gid privileged capability drop artifact ownership blast radius developer safety
files: Dockerfile|Containerfile|(docker-)?compose\.ya?ml|\.dockerignore|\.devcontainer
commands: (docker|podman|nerdctl|buildah)\ (build|run|compose)
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: heuristic -->
# Container Safety

When our build or a dev task runs inside a container — Docker, Podman, Buildah, Compose, devcontainers — the definition is a **developer-safety surface**, not just a build recipe. The convenient default (everything as root) quietly widens the blast radius of ordinary mistakes on our *own* machine. This isn't about a remote exploit; it's about not building the setup where a routine cleanup command can do real damage.

The failure mode we're avoiding: a build runs as root, writes root-owned artifacts into a bind-mounted host path, cleaning them up then needs `sudo` — and `sudo` in our muscle memory around build output is one slice of Swiss cheese away from an `rm -rf` landing on the wrong target. Least privilege in the definition keeps ordinary mistakes ordinary.

## What we check in a container definition

| Concern | What to do | Why |
|---------|-----------|-----|
| **Who runs** | Set a non-root `USER` for the build/run stage | Root in the container is root on any bind-mounted host path |
| **Who owns artifacts** | Match the container uid/gid to the host user — `ARG UID`/`GID`, or run with `--user $(id -u):$(id -g)` | Root-owned build output forces `sudo` to clean, escalating blast radius |
| **How much privilege** | No `--privileged`, drop capabilities you don't need, don't bind-mount the docker socket into a build | A build almost never needs host-level power; grant the minimum |
| **What's mounted** | Mount the narrowest path, read-only where possible | Never mount `$HOME` or `/` into a build — scope the surface |
| **What ships** | Multi-stage: build in one stage, copy artifacts into a slim non-root runtime stage | Keeps root and build tooling out of the final image |

## The shape we prefer

```dockerfile
# build args let the image match the invoking host user
ARG UID=1000
ARG GID=1000

FROM builder AS build
# ... compile ...

FROM runtime AS final
RUN groupadd -g ${GID} app && useradd -u ${UID} -g ${GID} -m app
USER app                     # non-root from here on
COPY --from=build --chown=app:app /out /app
```

For a throwaway local build, the lighter move is to skip the in-image user and just run as the host identity: `docker run --user "$(id -u):$(id -g)" -v "$PWD/out:/out:rw" …` — artifacts land owned by you, `sudo`-free to clean.

## See Also

- environment/makefile(softwaredev) — the build task runner these commands usually sit behind
- code/security(softwaredev) — least privilege as a general code concern
- architecture/threat-modeling(softwaredev) — blast radius and the Swiss-cheese framing this leans on
