# Working on agent-ways

These instructions are for contributors. The install does not project `.claude/` into `~/.claude`, so this file loads only for a session opened in a checkout or in the app dir (`$XDG_DATA_HOME/agent-ways`, itself a git clone). The root `CLAUDE.md` describes the product.

## Build cadence

A local build for this host takes about a minute. A tagged release takes a bump PR, CI, a tag and a four-platform build on GitHub runners. Evaluate on this host first.

1. **Evaluate locally.** On the branch, or on main:

   ```bash
   make dev-install
   ```

   It builds every binary in `tools/suite-bins` for this host and points the live install's links at this checkout's `bin/`: `~/.local/bin/<bin>`, and `~/.claude/bin/<bin>` where the projection has a link. It prints each link it repointed and fails when it repointed none. It writes nothing in the app dir. Running processes keep the old binary, so start a new Claude Code session and run `ways-agent unload`. Run `make dev-install` again after each change. `way-embed` is not rebuilt. The targets are Unix only.

   `dev-install` changes binaries only. Hooks, ways, skills and the attend mod still come from the app. To evaluate those, project the checkout with `ways manage repair --commit --source <checkout>`, or load a branch's mod for one session with `claude --plugin-dir <checkout>/plugins/attend`. The mod calls `attend` on `PATH`, so it runs against the dev-install build.

   Two commands undo part of a dev install when given `--commit`. `ways manage repair --commit` relinks `~/.claude/bin/<bin>` to the app. `ways manage update --commit` runs `make relink`, which relinks `~/.local/bin/<bin>` to the app. Without `--commit` both report the dev links they would move and change nothing. Do not run either with `--commit` during an evaluation, or run `make dev-install` again after them.

2. **Restore the released install.**

   ```bash
   make dev-restore
   ```

   It points the binary links back at `$XDG_DATA_HOME/agent-ways/bin/`. It does not undo a `repair --source <checkout>` projection; `ways manage repair --commit` with no `--source` projects the app again, binaries included. Restore before removing a worktree the links point into.

3. **Go to CI once the behaviour holds.** Push and open the PR. The PR runs `test.yml`, `clippy.yml`, and `build-<bin>.yml` for each binary the change touches, which builds linux-x86_64, linux-aarch64, darwin-x86_64 and darwin-arm64. Fix platform-specific breakage on the branch.

4. **Release.** When to cut a release and how are under Release Mechanics in the project-health way (`.claude/ways/meta/project-health/`).

## Gates

Run these before the push. The cargo workspace is `tools/Cargo.toml`.

| Command | Checks |
|---|---|
| `make lint` | clippy over the workspace, `-D warnings` |
| `cargo test --manifest-path tools/Cargo.toml -p <crate>` | the crate you changed; `make test-unit` runs every crate |
| `cargo test --manifest-path tools/Cargo.toml -p attend --no-default-features` | attend without its sensors, as CI runs it |
| `cargo test --manifest-path tools/Cargo.toml -p attend --no-default-features --features sensor-keepwarm` | attend with keepwarm only, as CI runs it |
| `make test-hooks` | the hook scripts and the Makefile targets |
| `CLAUDE_CODE_TYPES=<path>/claude-code.d.ts make test-plugins` | the mods; the type check needs the `claude-code.d.ts` that Claude Code bundles, and skips without it |
| `ways author lint <way-dir>`, `scripts/check-register.sh` | a changed way |
| `make test` | lint, smoke, unit, simulation, ADR tool, statusline and hook suites |

A TUI change gets its scenario script: `tools/ways-cli/shots/verify.sh` or `tools/attend-chat/shots/shots.sh`. The tui-verification way has the procedure.

`docs/development.md` covers the checkout, the sandbox and the build plumbing.
