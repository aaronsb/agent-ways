# Working in the live fixture

This directory is the tier 1 live install fixture from [ADR-186](../../../docs/architecture/system/ADR-186-live-integration-fixture-install-path-test-levels-and-the-tier-2-gate.md). [README.md](README.md) here says what it asserts. This file says how to run it, change it, and read a failure.

## Run it

```bash
cargo build --release --manifest-path tools/Cargo.toml -p ways -p ways-audit -p attend -p attend-chat
make test-live TIER=1                    # branch flavor, about four minutes
make test-live TIER=1 FLAVOR=release     # the one-liner against the latest release
```

- The branch flavor clones the checkout inside the container, so it tests the committed HEAD. Commit a hook or script edit before running. The binaries are whatever sits under `tools/target/release`, so rebuild after a Rust change.
- Exit code 2 means the wrapper stopped before Docker ran: no Docker, a missing binary, or `TIER=2`. Exit code 1 means assertions failed. The image is cached per installer and version, so a second run skips the build.
- `GH_TOKEN` comes from `gh auth token` when unset. The release-asset downloads fail without it.

## Read a failure

The runner does not stop at the first failed assertion. Every assertion runs, and the summary at the end lists the failures by name, then dumps the installer output, the `reconcile --force` output, the merged hook commands, `ways status`, and the last 60 lines of hook stderr. Read the summary from the bottom. A failure in section 2 usually cascades through sections 3 to 7, so fix the earliest one first.

## Change it

Assertions live in `run-tier1.sh` under numbered sections. Three helpers:

| Helper | Passes when |
|--------|-------------|
| `assert NAME cmd...` | the command exits 0 |
| `assert_eq NAME expected actual` | the strings match |
| `assert_contains NAME needle haystack` | the needle is a substring |

Add a new assertion to the section it belongs to. Name it as a sentence that reads true when it passes.

**To assert a new hook event or trigger**, add a payload under `payloads/` with `cwd` set to `/home/tester/project` and call `run_event EVENT NAME PAYLOAD`. `NAME` is the `SessionStart` source or the tool name and is matched against each hook's `matcher`. The driver expands `${HOME}` and nothing else, so a hook command that relies on another variable will not run the way Claude Code runs it. Check `nonzero_hooks` after every event.

**The seed is the #501 shape.** A real `skills/` directory, three user hooks across three events, a `model` key, and a second config directory. Assertions in sections 2, 4, and 8 hash against it. Do not add files to `seed/` to make a new assertion convenient. Add a new seed directory and its own section instead.

**The Claude Code version** is pinned in `compose.yaml` and the `Dockerfile` default. Bump both together. Section 9 asserts the exact version unless `CLAUDE_VERSION=latest`.

**The toolchain in the image** (`cmake`, `g++`) exists so `make setup` can build `way-embed` from source. It comes out once #516 ships a `way-embed` release with `match --batch`.

## CI

The branch flavor runs as the `live fixture (tier 1)` job in `portability.yml` on every pull request. The release flavor runs from `live-fixture.yml` nightly and on dispatch. A red release job means drift between `main`, the latest release, or Claude Code's newest version, and it is never a check on a PR.

## Tier 2

Not built. `test-live.sh` exits 2 on `TIER=2`. ADR-186 item 3 describes it: `claude -p` under `CLAUDE_CONFIG_DIR` with an API key, transcripts read through `ways introspect`, two containers for the attend peer test, dispatch or schedule only.
