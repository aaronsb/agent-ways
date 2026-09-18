# Live integration fixture

A Debian container with Claude Code installed, a home seeded with a user's own files, and the agent-ways installer run unattended. ADR-186 sets the two tiers; this directory holds tier 1.

```bash
make test-live TIER=1                    # branch flavor: this checkout, your built binaries
make test-live TIER=1 FLAVOR=release     # the documented one-liner against the latest release
make test-live TIER=1 CLAUDE_VERSION=latest CLAUDE_INSTALLER=npm
```

The branch flavor needs the four suite binaries under `tools/target/release`:

```bash
cargo build --release --manifest-path tools/Cargo.toml -p ways -p ways-audit -p attend -p attend-chat
```

The branch flavor clones the checkout, so it tests the committed HEAD. Commit before running to test an edit to a hook or a script. The mounted binaries are whatever is on disk.

The release-asset downloads go through `gh`, so the wrapper exports `GH_TOKEN` from `gh auth token` when it is unset.

## Files

| Path | Role |
|------|------|
| `Dockerfile` | Debian trixie, the installer prerequisites, `gh`, an unprivileged user, Claude Code at `CLAUDE_VERSION` through `CLAUDE_INSTALLER` |
| `compose.yaml` | The `tier1` service: mounts the checkout at `/src`, the binaries at `/binaries`, this directory at `/fixture` |
| `test-live.sh` | Host-side entry point behind `make test-live` |
| `run-tier1.sh` | The runner inside the container: seed, install, assert |
| `seed/claude/` | What `~/.claude` holds before the install: a real `skills/` directory, three user hooks, a `model` key |
| `seed/claude-work/` | A second Claude Code config directory, asserted untouched |
| `payloads/` | Synthetic hook payloads for `SessionStart`, `UserPromptSubmit`, and `PreToolUse` |

## What tier 1 asserts

1. The installer runs unattended, exits 1 on the real `skills/` directory, and leaves the skill, `settings.json`, and the hooks directory as they were.
2. `ways reconcile --force` moves the directory to a timestamped sibling with the skill intact, links the projection roots, and merges `settings.json` with every user hook kept by identity and the `model` key kept.
3. The target is recorded in the user config. `ways config targets --json` and `ways status --json` report one enabled target, active, with the embedding engine up.
4. `ways reconcile --dry-run` twice prints identical output and reports up to date. A bare reconcile changes nothing.
5. The second config directory hashes the same as its seed.
6. `attend status` and `claude --version` exit 0. The version is the pinned one unless `latest` was requested.
7. Hooks run the way Claude Code runs them. The runner reads each event's commands out of the merged `settings.json`, keeps the entries whose matcher matches the source or tool name, expands `${HOME}`, and pipes the payload to stdin. It asserts the ways table and core guidance on `SessionStart`, the ADR way on an ADR prompt, the gitconfig way on `git config --global`, the validate way on an edit to `README.md`, the user's own hooks still running, and a zero exit from every hook.

The payloads name `/home/tester/project` as the working directory. The runner creates it as a git repository before any hook runs.

## Tier 2

Tier 2 exercises a model with an API key and is not built yet. `make test-live TIER=2` exits 2 until it is.
