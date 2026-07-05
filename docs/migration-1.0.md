# Migrating to agent-ways 1.0

> **agent-ways 1.0 changes where your install lives.** Before 1.0, *the repo **was**
> your install* — you cloned into `~/.claude` and ran it in place. 1.0 turns `~/.claude`
> into a thin **projection** of an XDG application whose source lives in
> `$XDG_DATA_HOME/agent-ways`. This guide is for adopters with a **pre-1.0 in-place clone**
> who want to move to the new layout. Fresh installs don't need it.

The move is performed by one gated, backup-first command — `ways migrate` — and it is
**opt-in**: 1.0.0 ships the migrator but nothing migrates automatically. You choose when.

---

## What changes

| Location | Holds | Durability |
|---|---|---|
| `$XDG_DATA_HOME/agent-ways/` | **The application** — exactly what's on GitHub: ways, skills, hooks, `bin/`, docs. | Replaced wholesale on update. Losing it is a re-install, not data loss. |
| `$XDG_CONFIG_HOME/agent-ways/` | **Your own** ways and macros, plus `ways.json`. | Durable; never touched by update. |
| `$XDG_STATE_HOME/agent-ways/` | **Session substrate** — ledger, memory, focus. | Durable; survives a `~/.claude` wipe. |
| `$XDG_CACHE_HOME/agent-ways/` | **Derived** — corpus, embeddings, model (renamed from `claude-ways/`). | Regenerable; safe to delete. |
| `~/.claude/` | **The projection** — a merged `settings.json` plus symlinks to the projected tree, and the files Claude Code owns (`projects/`, credentials). | The Claude-Code-owned floor; regenerable from the manifest. |

The headline consequences:

- **`~/.claude` is no longer a git repo.** There's no `.git` there to `git pull` anymore — the app source moved to `$XDG_DATA_HOME/agent-ways`.
- **Your session history is preserved in place.** `~/.claude/projects/` is Claude-Code-owned; the migrator never moves or rewrites it.
- **Your `settings.json` is merged, not replaced.** A three-way merge manages only the hooks block and ways permissions; your model, theme, plugins, and credentials are left exactly as they are.

Background: [ADR-142](architecture/system/ADR-142-agent-ways-1-0-xdg-application-distribution.md)
(the XDG layout), [ADR-144](architecture/system/ADR-144-install-repair-migrate-as-one-manifest-reconciler.md)
(the reconciler and migrator).

## Do I need to migrate?

You're a **legacy in-place** install — the one population this targets — if `~/.claude/.git`
exists and its `origin` points at `aaronsb/agent-ways` (or your fork). That's the
clone-in-place model 1.0 supersedes.

If you started fresh on 1.0, or never made `~/.claude` a git repo, there's nothing to migrate.

## Before you start

The migrator is built to be safe — it **backs up `~/.claude` first** (to
`~/.claude.backup-<epoch>`), reconciles the new shape **before** removing the old one, and
is **resumable**: if a phase aborts it stamps a marker, and re-running `--execute` continues
where it stopped rather than starting over.

That said, the one genuinely irreplaceable thing is **`~/.claude/projects/`** — your session
transcripts. It's preserved in place, but snapshotting it yourself costs nothing and is
cheap insurance for any migration:

```bash
cp -a ~/.claude/projects ~/claude-projects-keepsake
```

## Migrate

**1. Upgrade your in-place install to 1.0.0.** From your existing clone:

```bash
cd ~/.claude
git pull
make update          # rebuilds the 1.0 ways binary (in-place topology)
```

You now have the 1.0 `ways` binary, still in the legacy in-place shape. Nothing has moved yet.

**2. Preview — read-only, no mutations.** Two depths of preview:

```bash
ways migrate            # the plan (default): what would change, read-only
ways migrate --what-if  # dry-run the executor: assert every phase's contract without writing
```

`--what-if` is the stronger check — it runs the executor's logic against your real install
and verifies each phase *could* succeed, without touching anything. Read its output before
proceeding.

**3. Execute.** Gated, backs up first, resumable:

```bash
ways migrate --execute
```

When it finishes, `~/.claude` is the projection and the app lives in
`$XDG_DATA_HOME/agent-ways`. Verify:

```bash
ways status            # binary, model, corpus, project detection — all should resolve
```

`ways status` should show the `ways` binary resolving through `~/.claude/bin/ways` to a real
file under `$XDG_DATA_HOME/agent-ways/bin/`, the corpus under `$XDG_CACHE_HOME/agent-ways`,
and your core ways count intact.

## If something goes wrong

The migrator fails safe. On an aborted phase it **prints the exact restore command** and
leaves your backup untouched. To roll back manually:

```bash
rm -rf ~/.claude
mv ~/.claude.backup-<epoch> ~/.claude
```

Because it's marker-resumable, you can also just fix the cause and re-run `ways migrate
--execute` to continue.

Hit a migration bug? **Please report it.** Real-world migrations on varied installs are
exactly how correctness fixes land in the 1.0.x line — the same way the first round of fixes
did before 1.0.0 shipped.

## The migration window — don't wait indefinitely

`ways migrate` is a **transitional** command with a deliberate end of life:

| Version | `ways migrate` |
|---|---|
| **1.0.0 → 1.2.x** | Present. Migrate anytime in this window. |
| **1.3.0 and later** | **Removed.** The binary no longer carries the migrator. |

(The removal was originally slated for 1.1 and **deferred to 1.3** to widen the migration window — the migrator is still present in 1.1.x and 1.2.x.)

After 1.3, an un-migrated install can still migrate — but only by first checking out a
**pre-1.3 release** that still ships the command (the last `ways-v1.2.x` tag), running
`ways migrate --execute`, and *then* updating to current:

```bash
# After 1.3, to migrate a still-legacy install:
git checkout ways-v1.2.x   # the last release that carries `ways migrate`
make update
ways migrate --execute
# then update to the current release
```

The transition fallbacks in 1.0.x mean an un-migrated install keeps *working* in the
meantime — but the supported, friction-free path is to migrate while the command is still in
the binary.

## After migrating

Your mental model for developing on and updating agent-ways changes with the layout — the
install, your dev checkout, and a sandbox are now three different places. See
[development.md](development.md) for the post-1.0 workflow.

## See also

- [ADR-142](architecture/system/ADR-142-agent-ways-1-0-xdg-application-distribution.md) — the XDG application distribution
- [ADR-143](architecture/system/ADR-143-three-root-way-runtime-core-user-project.md) — core / user / project way roots
- [ADR-144](architecture/system/ADR-144-install-repair-migrate-as-one-manifest-reconciler.md) — the reconciler, migrator, and deprecation lifecycle
- [development.md](development.md) — developing agent-ways after the 1.0 shift
- [install-guide.md](install-guide.md) — installation paths (being updated for the 1.0 layout)
