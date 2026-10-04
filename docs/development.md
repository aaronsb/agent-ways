# Developing agent-ways

`~/.claude` is a thin **projection** of an XDG application whose source lives in `$XDG_DATA_HOME/agent-ways` ([ADR-142](architecture/platform/ADR-142-agent-ways-1-0-xdg-application-distribution.md)). Development happens in a **separate checkout**, and you choose when your changes reach your install.

## Three places

| Role | Where it lives | Do you edit it? |
|---|---|---|
| **Your install** | `~/.claude` (projection) + `$XDG_DATA_HOME/agent-ways` (the app) | **No.** `ways update` stashes local changes, fast-forwards, and pops them back, so an edit there can conflict with an update or block it. |
| **Your dev checkout** | a standalone clone, e.g. `~/src/agent-ways` (not `~/.claude`, not `$XDG_DATA`) | **Yes.** Branch, edit, commit, PR here. |
| **A sandbox** | a throwaway `$HOME`/`$XDG_*` under `/tmp` | Only the test harness writes here. |

## Setup

```bash
git clone https://github.com/aaronsb/agent-ways ~/src/agent-ways   # or your fork
cd ~/src/agent-ways
make setup     # every suite binary into bin/, way-embed, the model, the corpus
make test      # every suite; see Checks below
```

There is no `Cargo.toml` at the repo root. The Rust workspace is `tools/Cargo.toml`, so cargo commands take `--manifest-path tools/Cargo.toml`:

```bash
cargo build --release --manifest-path tools/Cargo.toml -p ways
cargo test --manifest-path tools/Cargo.toml --workspace
```

`cargo build` writes `tools/target/release/<name>`. A Makefile source build symlinks `bin/<name>` to that file (on Linux and macOS), and the projection and the `PATH` links point at `bin/<name>`.

## Workspace crates

Each suite binary in `tools/suite-bins` is a package of the same name, so `-p <name>` builds it. The `ways` package lives in `tools/ways-cli`. The rest are libraries and dev tools.

| Crate | Kind | What it holds |
|---|---|---|
| `ways` (`tools/ways-cli`) | binary | The CLI and every hook entry point (`ways hook <event>`) |
| `ways-core` | library | Way discovery, frontmatter, paths, config |
| `ways-audit` | binary | Compliance claims and findings (ADR-151, ADR-200) |
| `ways-agent` | binary | The resident daemon: relevance judge and key custody (ADR-196, ADR-502) |
| `ways-agent-core` | library | What the agent shares with its clients: profiles, the key store, the protocol. No network code, so the hook links no TLS |
| `ways-mcp` | binary | The agent-ways MCP server (ADR-501) |
| `attend` | binary | The awareness loop and its sensors |
| `attend-chat` | binary | The chat terminal on the signal bus (ADR-120) |
| `attend-config` | library | attend's settings schema |
| `attend-groups`, `attend-instances`, `attend-presence`, `attend-state` | library | attend's focus groups, instance registry, session identity and heartbeat, and per-session sensor state |
| `sensor-trait`, `sensor-peers`, `sensor-processes`, `sensor-keepwarm` | library | The sensor interface and the built-in sensors |
| `agent-settings` | library | The settings registry: layered load, lint, emit, the atomic writer (ADR-503) |
| `agent-tui`, `agent-theme` | library | The ratatui app shell and the theme engine every screen uses (ADR-504) |
| `agent-fmt`, `agent-identity` | library | Shared terminal formatting; stable nicknames and the colour palette |
| `claude-sessions` | library | Finds Claude Code config directories, projects, sessions and transcripts |
| `tui-harness` | dev binary | Drives a TUI in a detached tmux pane and screenshots it as PNG. Dev only, never released |

`way-embed` (C++, `tools/way-embed`, built from the bundled `llama.cpp`) sits outside the workspace and has its own Makefile.

## Build plumbing

- `tools/suite-bins` lists the suite binaries. The Makefile builds and links each one, `scripts/install.sh` puts each on `PATH`, and each has a `build-<name>.yml` release workflow. Add a binary here and it joins all three.
- `make <name>` keeps a working `bin/<name>`, else runs `tools/scripts/download-prebuilt.sh <name>`, else builds with cargo. The downloader needs a logged-in `gh`. It picks the newest `<name>-v*` release for your platform and checks it against the release's `checksums.txt`, installing with a warning when the release has none; `<NAME>_RELEASE` (for example `WAYS_AUDIT_RELEASE`) pins a tag. Its logic lives in `tools/scripts/prebuilt-lib.sh`.
- `make <name>-rebuild` forces a source build. `make update-binaries` rebuilds every suite binary and `way-embed`.
- `make site` builds the published manual into `site/`, and `make site-serve` previews it with live reload. MkDocs renders `docs/`, with the nav taken from `scripts/docs-site/nav.md`. `scripts/docs-site/gen_ways.py` adds a page for every way under `hooks/ways/`, and `scripts/docs-site/links.py` points repository links outside the site at GitHub. `.github/workflows/pages.yml` deploys the site to GitHub Pages when a component release tag is pushed, and builds it without deploying on pull requests that touch these sources.
- `make deps` installs cmake, a C++ compiler and git through the system package manager, with `sudo`. Only `way-embed` needs them.

## Checks

| Command | What it checks |
|---|---|
| `make lint` | clippy on the workspace, warnings as errors |
| `make test` | lint, then the smoke, unit, simulation, ADR tool, statusline and hook suites |
| `scripts/check-register.sh` | `hooks/ways/core.md` has none of the register shapes ADR-178 bans; `--corpus` adds an advisory report over every way |
| `scripts/check-facts.sh [REV]` | Counts, paths, identifiers, headings and links that left a markdown file you reworded. Advisory |
| `scripts/check-portability.sh` | CRLF endings, non-portable shebangs, hard-coded home paths |
| `scripts/check-rust.sh` | Rust 1.89 or later; every source build runs it first |

`hooks/pre-commit` scans staged files for secrets and runs `check-portability.sh` and `check-register.sh`. Git does not run it until you link it in your clone:

```bash
ln -s ../../hooks/pre-commit .git/hooks/pre-commit
```

It needs `python3`, `bc` and a `grep` with `-P` (GNU grep; on macOS, `brew install grep` and put its gnubin first on `PATH`).

To look at a screen without a terminal of your own:

```bash
cargo build --manifest-path tools/Cargo.toml -p tui-harness
th=tools/target/debug/tui-harness
$th launch demo -- ways settings
$th send demo Tab
$th shot demo     # prints the PNG path
$th down demo
```

See `tools/tui-harness/README.md` for every command.

## Testing your changes, by blast radius

1. **Sandbox (default, zero-risk).** Point `$HOME` and the `$XDG_*` vars at a tmpdir and run your binary against it. Nothing touches your real install. The test suite and every demo work this way:

   ```bash
   SB=$(mktemp -d)
   HOME="$SB" XDG_DATA_HOME="$SB/.local/share" XDG_CONFIG_HOME="$SB/.config" \
     XDG_CACHE_HOME="$SB/.cache" XDG_STATE_HOME="$SB/.local/state" \
     ./tools/target/release/ways <subcommand>
   ```

   `ways reconcile` honours these env vars too, so a fake install under `$SB/.claude` exercises the projection engine without touching `~/.claude`.

2. **Dogfood via reconcile.** Project your dev tree into your live install:

   ```bash
   ways reconcile --source ~/src/agent-ways --dest ~/.claude
   ```

   This projects hooks, ways, skills and agents from the dev tree. Binaries are projected from `<source>/bin/`. After `make ways-rebuild` (or `make update-binaries`), `bin/<name>` is a symlink into `tools/target/release/`, so later `cargo build --release` runs reach the projection too. A `bin/<name>` that `make setup` downloaded stays the prebuilt until you run a `-rebuild` target. Revert by reconciling from the app: `ways reconcile --source $XDG_DATA_HOME/agent-ways --dest ~/.claude`.

3. **Worktree (parallel branches).** `git worktree add` from your standalone clone, never from `$XDG_DATA/agent-ways`. A worktree hung off the app dir ties your branches to the install, and a reinstall that replaces the app dir orphans it.

## Conventions

- **ADR-driven:** architectural changes get an ADR first (`docs/scripts/adr new …`); reference the ADR number in the branch and commits. Status flips to `Accepted` once the implementation lands.
- **Branch → PR → review → merge.** Even solo. The `code-reviewer` pass has caught real "the code claims X but does Y" bugs that green tests didn't.
- **Releases are per component, in two steps** (ADR-150). `make cut-release COMPONENT=<name> LEVEL=patch|minor|major` opens a version-bump PR. After it merges, `make publish-release COMPONENT=<name> PUSH=1` tags it, and CI builds the platform artifacts and the GitHub Release. Components are the six suite binaries. The `release` skill walks through it.
- **Paths have one location.** `paths::cache_root()` and `events_log()` resolve to the XDG location only; the pre-1.0 fallbacks were removed (ADR-506). Do not add a read of an old name or path for compatibility.

## See also

- [ADR-142](architecture/platform/ADR-142-agent-ways-1-0-xdg-application-distribution.md): the XDG application distribution
- [ADR-143](architecture/practice/ADR-143-three-root-way-runtime-core-user-project.md): core / user / project way roots
- [ADR-144](architecture/platform/ADR-144-install-repair-migrate-as-one-manifest-reconciler.md): the reconciler
- [CONTRIBUTING.md](../CONTRIBUTING.md): contribution norms and the security bar for changes
