---
status: Accepted
date: 2026-09-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-142
  - ADR-144
  - ADR-184
  - ADR-185
---

# ADR-186: Live integration fixture: install-path test levels and the tier 2 gate

## Context

The install path is the installer script, `make setup` with its prebuilt downloads, `ways reconcile` into a config directory that already holds a user's own files, and the hook scripts that Claude Code runs from the merged `settings.json`. The reviews of PRs #501, #502, #504 and #508 each found a defect on that path by reading the code, and each said the same thing: none of it had been run end to end on a clean machine.

The tests that exist stop short of it. The Rust unit tests and `session_sim` exercise the binary against fixture ways in a temporary directory. The reconcile tests put a fake source and a fake destination in a sandbox. The sandbox transcripts in the PR comments were run by hand and are not repeatable. Nothing runs the installer, downloads a release asset, seeds a home with the shape from #501 (a real `skills/` directory, three hand-written hooks, a second config directory), or feeds a hook script the payload Claude Code sends.

ADR-184 increment 2 (#509) changes the installer's last act from projecting into `~/.claude` to handing off to the targets bootstrap. Building that against no fixture repeats the pattern the reviews named.

## Decision

**The install path is tested at two levels. Tier 1 installs and configures with no API key and runs on every pull request that touches the path. Tier 2 exercises a model with a key, runs on manual dispatch or a schedule, and never runs unattended on a pull request.**

1. **Tier 1: install and configure, no key.** A Debian container with Claude Code installed at a pinned version. The home is seeded before the installer runs: a real `~/.claude/skills/` with a user's own skill, a `settings.json` with three user hooks and a `model` key, and a second config directory with its own settings and skills. The runner asserts:
   - the installer runs unattended and refuses the real `skills/` directory with nothing deleted and `settings.json` byte-identical;
   - the documented recovery (`ways reconcile --force`) moves the directory aside with the user's skill intact, links the projection roots, and keeps every user hook by identity in its event;
   - the target is recorded in the user config, `ways config targets` reports one enabled target, and `ways status` reports the active state with the embedding engine up;
   - `ways reconcile --dry-run` run twice prints identical output and reports no change;
   - the second config directory is byte-identical to its seed;
   - `way-embed` arrived as a release download. The image carries no C++ toolchain, so a source-build fallback is a failed assertion;
   - `attend status` and `claude --version` exit zero, and the version is the pinned one;
   - the hooks fire the way Claude Code fires them: the runner reads the hook commands out of the merged `settings.json`, pipes a synthetic `SessionStart`, `UserPromptSubmit`, and `PreToolUse` payload to each, and asserts on the additional-context output. Disclosure is proven with no model.

2. **Two flavors of tier 1.** The `branch` flavor mounts the checkout and the binaries CI built for the same commit, so a pull request is tested against its own binary and its own hooks. It is the gate, and it runs on every pull request that touches the path and on every push to `main`. The `release` flavor runs the documented one-liner, which clones `main` and downloads the latest release assets. It runs on dispatch and on the nightly schedule, with Claude Code at its `latest` version, to catch drift on either side.

3. **Tier 2: exercise with a key.** `claude -p` runs non-interactively under `CLAUDE_CONFIG_DIR` with prompts chosen to trigger named ways. The evidence is the transcript and the events log read through `ways introspect`, plus the answer scored against a rubric with a stated pass threshold. Two containers on one compose network carry the attend peer test: send from one, assert the other's inbox. Keepwarm is out of scope.

4. **The gate.** Tier 2 reads the key from a repository secret. The workflow runs it on `workflow_dispatch` and on `schedule` only. A pull request never triggers it, and a fork pull request cannot see the secret. A change to tier 2 is verified by dispatching it against the branch.

5. **Shape.** Everything lives under `tests/fixtures/docker/`: the compose file, one Dockerfile parameterized by Claude Code version and installer, the seed, the payloads, and one runner per tier. `make test-live TIER=1|2` is the entry point on a workstation and in CI. Tier 1 is a job in `portability.yml`.

6. **Sequence.** Tier 1 lands before #509. Tier 2 follows tier 1 as its own increment.

Reversibility: cheap. The fixture is additive. Removing the job removes the gate and nothing else.

## Consequences

### Positive

- A change to the installer, the reconciler, the settings merge, or a hook script is run on a clean machine before review reads it.
- The #501 shape is a fixture, not a memory. The refusal, the recovery, and the kept hooks are asserted on every pull request.
- #509 is built against a fixture that fails when it wires the wrong directory.
- A pinned Claude Code on the gate and `latest` on the schedule separate our regressions from upstream drift.

### Negative

- Tier 1 needs network: the Claude Code installer, the way-embed and model downloads, and the release assets through `gh`. A network fault fails the job. The branch flavor keeps the four suite binaries off the network; way-embed, mmaid, and the model stay on it.
- Docker on the runner adds minutes to the portability workflow.
- Tier 2 on a schedule spends tokens on a fixed cadence. The threshold and the prompt set have to be maintained.

### Neutral

- The runner drives hooks from `settings.json` rather than by path, so a hook that the merge drops is a failed assertion rather than a silent skip.
- `portability.yml` gains a job that builds all four suite binaries before the fixture runs, since the branch flavor needs `ways-audit` and `attend-chat` beside `ways` and `attend`. The existing cross-platform matrix is unchanged.
- ADR-185's `--json` views are what the runner parses.
- The first run of the fixture found two defects on the download path, both fixed on the same branch. The download scripts listed 20 or 30 releases before grepping for a component prefix, and the newest tags of three components sat past that window. The way-embed download script's capability probe ran under `pipefail` and rejected every binary, since a supporting binary also exits nonzero when asked for `match --batch` without a corpus. #516 had diagnosed that as a release that predates the primitive. The release supports it; the probe was wrong. The image carries no C++ toolchain, so the fixture fails if either defect returns.

## Alternatives Considered

- **Run the installer on the GitHub runner's own home.** Rejected: the runner's home is not clean, and the seeded state would have to be undone between steps. A container starts empty every time.
- **Mock Claude Code.** Rejected: `claude --version`, the installer path, and the settings file are the things under test. The binary is a pinned download and costs one step.
- **Tier 2 on every pull request.** Rejected: a fork pull request has no secret, so the check would pass by absence, and every pull request would spend tokens. Dispatch and schedule keep the run deliberate.
- **One tier with the key optional.** Rejected: a runner that skips assertions when the key is missing reports green for two different things. Two runners with two names keep the report honest.
- **A virtual machine per run.** Rejected: slower to start, and the container already isolates the home, the XDG roots, and the PATH.
