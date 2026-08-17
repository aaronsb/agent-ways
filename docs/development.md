# Developing agent-ways

> **The 1.0 shift:** before 1.0, *the repo **was** your install* — you cloned into
> `~/.claude` and edited it live. 1.0 dissolves that identity. `~/.claude` becomes a
> thin **projection** of an XDG application, and the app source lives in
> `$XDG_DATA_HOME/agent-ways`. So development now starts from a **separate checkout**,
> and you *choose* when your changes reach your install — they no longer leak in by
> default. (Background: [ADR-142](architecture/system/ADR-142-agent-ways-1-0-xdg-application-distribution.md).)

## The three roles that used to be one directory

| Role | Where it lives | Do you edit it? |
|---|---|---|
| **Your install** | `~/.claude` (projection) + `$XDG_DATA_HOME/agent-ways` (the app) | **No.** `$XDG_DATA` is replaced wholesale on update; editing it is undone on the next update. |
| **Your dev checkout** | a standalone clone, e.g. `~/src/agent-ways` (**not** `~/.claude`, **not** `$XDG_DATA`) | **Yes.** Branch, edit, commit, PR here. |
| **A sandbox** | a throwaway `$HOME`/`$XDG_*` under `/tmp` | Only the test harness writes here. |

## Setup

```bash
git clone https://github.com/aaronsb/agent-ways ~/src/agent-ways   # or your fork
cd ~/src/agent-ways
cargo build --release --manifest-path tools/Cargo.toml -p ways      # build the binary
cargo test -p ways                                                  # 180+ unit + integration tests
```

The built binary is `tools/target/release/ways`. Run it by path, or alias it while
developing — **don't** put it ahead of your installed `ways` on `PATH` unless you mean to.

## Testing your changes — pick by blast radius

1. **Sandbox (default, zero-risk).** Point `$HOME` and the `$XDG_*` vars at a tmpdir and run
   your binary against it. Nothing touches your real install. This is how the whole test
   suite and every demo works:

   ```bash
   SB=$(mktemp -d)
   HOME="$SB" XDG_DATA_HOME="$SB/.local/share" XDG_CONFIG_HOME="$SB/.config" \
     XDG_CACHE_HOME="$SB/.cache" XDG_STATE_HOME="$SB/.local/state" \
     ./tools/target/release/ways <subcommand>
   ```

   `ways reconcile` honours these env vars too, so a fake install under `$SB/.claude`
   exercises the projection engine without ever touching `~/.claude`.

2. **Dogfood via reconcile.** Project your dev tree into your live install to run your own
   code for real:

   ```bash
   ways reconcile --source ~/src/agent-ways --dest ~/.claude
   ```

   Revert by reconciling from the released app: `ways reconcile --source $XDG_DATA_HOME/agent-ways --dest ~/.claude`.

3. **Worktree (parallel branches).** `git worktree add` from your **standalone clone** —
   never from `$XDG_DATA/agent-ways`. The app dir is replaced wholesale on update, which
   would orphan a worktree hung off it (its gitdir link dies).

## Conventions

- **ADR-driven:** architectural changes get an ADR first (`docs/scripts/adr new …`); reference
  the ADR number in the branch and commits. Status flips to `Accepted` when the implementation
  lands, not when the ADR is written.
- **Branch → PR → review → merge.** Even solo. The `code-reviewer` pass earns its keep — it has
  caught real "the code claims X but does Y" bugs that green tests didn't.
- **Releases are tag-driven:** bump `tools/ways-cli/Cargo.toml`, push a `ways-v*` tag, CI builds
  the per-platform artifacts. See the release way / `docs/architecture` for the pipeline.
- **The transition fallbacks are load-bearing, not legacy cruft.** `paths::cache_root()` and
  `events_log()` prefer the new XDG location but fall back to the pre-1.0 one so an un-migrated
  install keeps working. When you touch cache/state paths, preserve the fallback. ADR-179
  kept these when it removed the migrator — they are what lets a current binary read a
  legacy install correctly.

## See also

- [ADR-142](architecture/system/ADR-142-agent-ways-1-0-xdg-application-distribution.md) — the XDG application distribution (why dev changed)
- [ADR-143](architecture/system/ADR-143-three-root-way-runtime-core-user-project.md) — core / user / project way roots
- [ADR-144](architecture/system/ADR-144-install-repair-migrate-as-one-manifest-reconciler.md) — the reconciler, migrator, and deprecation lifecycle
- `CONTRIBUTING.md` — contribution norms and the security bar for changes
