# Working in the live fixture

This directory is the tier 1 live install fixture from [ADR-186](../../../docs/architecture/platform/ADR-186-live-integration-fixture-install-path-test-levels-and-the-tier-2-gate.md). [README.md](README.md) here says what it asserts. This file says how to run it, change it, and read a failure.

## Run it

```bash
cargo build --release --manifest-path tools/Cargo.toml -p ways -p ways-audit -p attend -p attend-chat
make test-live TIER=1                    # branch flavor, about four minutes
make test-live TIER=1 FLAVOR=release     # the one-liner against the latest release
```

- The branch flavor clones the checkout inside the container, so it tests the committed HEAD. Commit a hook or script edit before running. The binaries are whatever sits under `tools/target/release`, so rebuild after a Rust change.
- Exit code 2 means the wrapper stopped before Docker ran: no Docker, a missing binary, or for `TIER=2` a missing key or an unwritable `TIER2_OUT`. Exit code 1 means assertions failed. The image is cached per installer and version, so a second run skips the build.
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

**To assert a new hook event or trigger**, add a payload under `payloads/` with `cwd` set to `/home/tester/project` and call `run_event EVENT NAME PAYLOAD`. `NAME` is the `SessionStart` source or the tool name and is matched against each hook's `matcher`, with empty and `*` as match-all. The driver expands `${HOME}` and nothing else, so a hook command that relies on another variable will not run the way Claude Code runs it. Exit codes land in a file, not a variable, because the call sits inside command substitution. Check `nonzero_hooks` after every event.

**The seed is the #501 shape.** A real `skills/` directory, three user hooks across three events, a `model` key, and a second config directory. Assertions in sections 2, 4, and 8 hash against it. Do not add files to `seed/` to make a new assertion convenient. Add a new seed directory and its own section instead.

**The Claude Code version** is pinned once, in `test-live.sh`. `compose.yaml` and the `Dockerfile` take it from there. Section 9 asserts the exact version unless `CLAUDE_VERSION=latest`.

**The image has no C++ toolchain.** Section 2 asserts that `way-embed` arrived as a release download. Do not add `cmake` or `g++` to make a failing run pass. A source-build fallback means the download script or the release is broken, which is what the assertion is for.

## CI

The branch flavor runs as the `live fixture (tier 1)` job in `portability.yml` on every pull request. The release flavor runs from `live-fixture.yml` after each ways release finishes building, and on dispatch (ADR-508). A red release job means drift between `main`, the latest release, or Claude Code's newest version, and it is never a check on a PR.

## Tier 2

Tier 2 runs tier 1 as its precondition, then drives scenarios through `claude -p` with a real key. It spends tokens, so CI runs it from `live-fixture.yml` after each ways release and on dispatch, never on a pull request (ADR-508).

```bash
ANTHROPIC_API_KEY_FILE=~/path/to/key make test-live TIER=2
TIER2_SCENARIOS="adr-way" TIER2_MODEL=claude-sonnet-5 ANTHROPIC_API_KEY_FILE=... make test-live TIER=2
```

- The key comes from `ANTHROPIC_API_KEY` or from the file named by `ANTHROPIC_API_KEY_FILE`. It reaches the container through the environment only. It is not a build arg, so no image layer holds it, and nothing prints it.
- `test-live.sh` creates `TIER2_OUT` (a temp dir by default) as the host user and prints its path. Each scenario leaves `result.json`, `introspect.json`, `fired.txt`, `worktree.txt` and `claude.err` there. A rootful daemon would create a missing bind source as root, which the container user cannot write, so the wrapper creates the directory first.
- The branch flavor clones the committed HEAD, but `/fixture` (this directory: runners, scenarios, checks) is mounted live from the working tree. Stay on the branch until the run finishes; switching branches mid-run swaps the checks out from under it.
- A scenario's `max_turns` file raises its turn cap. The adr scenarios need 20 to 40 turns and cost $0.30 to $1 each on `claude-sonnet-5`; the way-firing scenarios cost about $0.06.

**To add a scenario**, make a directory under `scenarios/` with a `prompt.txt`, an optional `setup.sh` that runs in the fresh project first, and a `check.sh` that `run-tier2.sh` sources. `check.sh` asserts with:

| Helper | Passes when |
|--------|-------------|
| `fired WAY_ID` | the way fired in the session (hard) |
| `not_fired WAY_ID` | the way stayed silent (hard) |
| `rubric DESC REGEX` | scores one item against the answer (soft) |
| `rubric_threshold N` | at least N rubric items hit (hard) |

Ways are asserted hard and the answer's wording is scored against a threshold, because wording varies run to run. `check.sh` sees `$PROJ`, `$ANSWER`, `$FIRED` and `$OUT`. agent-ways scaffolds `.claude/` into every project it opens, so a worktree check reads `$OUT/worktree.txt` and ignores lines under `.claude/`.

A scenario that needs the operator to reply adds a `prompt2.txt`. The runner resumes the first turn's session with `claude -p --resume` and that prompt. An optional `check1.sh` is sourced between the turns and sees the first turn's `$ANSWER`; `check.sh` sees the second turn's. `$FIRED` holds the ways fired in either turn, and the second turn's files carry a `2` suffix (`result2.json`, `worktree2.txt`, ...). Each check file keeps its own rubric count.

The attend peer test from ADR-186 item 3 is not built yet. Attend discovers peers through the local process table and cache, which two containers do not share.
