# Working on agent-ways

These instructions are for contributors working in a checkout. The install projects nothing under `.claude/` into `~/.claude`, so this file never reaches a user's sessions. The root `CLAUDE.md` describes the product.

## Build cadence

A local build for this host takes about a minute. A tagged release takes a bump PR, CI, a tag and a four-platform build on GitHub runners. Evaluate on this host first.

1. **Evaluate locally.** On the branch, or on main:

   ```bash
   make dev-install
   ```

   It builds every binary in `tools/suite-bins` for this host and points the live install's links at this checkout's `bin/`: `~/.local/bin/<bin>`, and `~/.claude/bin/<bin>` where the projection has a link. It writes nothing in the app dir. Running processes keep the old binary, so start a new Claude Code session and run `ways-agent unload`. Run `make dev-install` again after each change. `way-embed` is not rebuilt.

   To run a branch's attend mod against that build, start the session with `claude --plugin-dir <checkout>/plugins/attend`. The mod calls `attend` on `PATH`.

2. **Restore the released binaries.**

   ```bash
   make dev-restore
   ```

   It points the same links back at `$XDG_DATA_HOME/agent-ways/bin/`. Run it before removing a worktree the links point into.

3. **Go to CI once the behaviour holds.** Push and open the PR. The PR runs `test.yml`, `clippy.yml`, and `build-<bin>.yml` for each binary the change touches, which builds linux-x86_64, linux-aarch64, darwin-x86_64 and darwin-arm64. Fix platform-specific breakage on the branch.

4. **Release in batches.** Cut a release after a set of merged changes has been evaluated locally and main is green. One fix is not a release. The steps are under Release Mechanics in the project-health way (`.claude/ways/meta/project-health/`).

## Gates

Run these before the push. The cargo workspace is `tools/Cargo.toml`.

| Command | Checks |
|---|---|
| `make lint` | clippy over the workspace, `-D warnings` |
| `cargo test --manifest-path tools/Cargo.toml -p <crate>` | the crate you changed; `make test-unit` runs every crate |
| `cargo test --manifest-path tools/Cargo.toml -p attend --no-default-features`, then again with `--features sensor-keepwarm` | attend's feature variants, as CI runs them |
| `make test-hooks` | the hook scripts and the Makefile targets |
| `CLAUDE_CODE_TYPES=<path>/claude-code.d.ts make test-plugins` | the mods; the type check needs the `claude-code.d.ts` that Claude Code bundles, and skips without it |
| `ways author lint <way-dir>`, `scripts/check-register.sh` | a changed way |
| `make test` | lint, smoke, unit, simulation, ADR tool, statusline and hook suites |

A TUI change gets its scenario script: `tools/ways-cli/shots/verify.sh` or `tools/attend-chat/shots/shots.sh`. The tui-verification way has the procedure.

`docs/development.md` covers the checkout, the sandbox and the build plumbing.
