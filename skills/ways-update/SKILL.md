---
name: ways-update
description: Update the agent-ways installation to the latest version — pull the app source in $XDG_DATA, rebuild the Rust binaries (ways/attend/way-embed) and corpus, and refresh the ~/.claude projection with `ways reconcile`. Use when the user wants to update agent-ways, pull the latest ways/hooks, or refresh their ways framework install. Not for editing or authoring individual ways (that is the ways skill) or upgrading project dependencies.
allowed-tools: Bash, Read
---

# ways-update: Update the agent-ways install

In 1.0, `~/.claude` is a thin **projection** of an XDG application whose source
lives in `$XDG_DATA_HOME/agent-ways` (ADR-142). Updating means refreshing *that*
checkout, rebuilding what depends on it, and reprojecting — not touching `~/.claude`
directly. This skill wraps the flow with the pre-flight checks a bare `git pull`
doesn't do.

## The update flow

```
cd "$XDG_DATA_HOME/agent-ways"
git pull --ff-only        # (or fetch/merge upstream for a fork — see step 2)
make update-binaries      # FORCE-rebuild ways/attend/way-embed (make setup skips existing binaries)
./bin/ways reconcile      # refresh the ~/.claude projection + re-merge settings.json
```

The pull is done here (step 1/2), so this uses `make update-binaries` — a force
rebuild — rather than `make setup`, which short-circuits any binary that already
exists and would silently leave you on the old build. (The all-in-one `make update`
bundles pull + rebuild + relink, but this skill does its own guarded pull first.)
The embedding corpus self-heals on the next session via the `ways corpus --if-stale`
`SessionStart` hook; regenerate it eagerly with `make -C "$APP" setup` if you want.

Because the projected roots are symlinks into the app dir, a pull is *live* for
existing files the moment it lands; `ways reconcile` is what catches **structural**
changes (a new skill/way directory to link) and re-runs the `settings.json`
three-way merge. A **running Claude Code session keeps the old hooks and ways in
memory** — always finish by telling the user to restart.

## Steps

Resolve the app dir once — the skill is global, so the working directory is
unknown at invocation:

```bash
APP="${XDG_DATA_HOME:-$HOME/.local/share}/agent-ways"
```

### 0. Guard: is this actually a projection install?

```bash
# Legacy pre-1.0 in-place clone? ~/.claude is itself the agent-ways repo.
# (Same triad as reconcile.rs is_legacy_in_place: own .git + ships tools/ + docs/.)
if [ -d "$HOME/.claude/.git" ] && [ -d "$HOME/.claude/tools" ] && [ -d "$HOME/.claude/docs" ]; then
  echo "~/.claude looks like a pre-1.0 in-place clone. Don't update it in place —"
  echo "migrate to the 1.0 projection first. The migrator was removed in 1.9.0 and"
  echo "lives at the ways-v1.8.3 tag; see docs/migration-1.0.md."
  exit 1
fi

# The app source must exist and be a git checkout.
grep -q 'agent-ways' "$APP/Makefile" 2>/dev/null && git -C "$APP" rev-parse --git-dir >/dev/null 2>&1 \
  || { echo "No agent-ways app source at $APP. (Re-run the installer to (re)stage it.)"; exit 1; }
```

### 1. Pre-flight: check the app checkout

`git pull --ff-only` **aborts** if tracked files have uncommitted changes or the
history diverged. Surface the state before pulling:

```bash
git -C "$APP" fetch origin --quiet
git -C "$APP" status --short --branch
```

- **Clean tree, behind remote** → safe to proceed to step 2.
- **Dirty tracked files** (common on a fork carrying custom ways) → tell the user
  what's modified. Offer to commit (`/merge`) or `git -C "$APP" stash` before the
  update and `git stash pop` after. Do **not** stash or discard without asking.
- **Diverged history** (local commits not on remote) → `--ff-only` will fail by
  design. Stop and explain; the user decides whether to push, rebase, or merge.

Untracked files are safe — the pull leaves them alone — but flag them.

### 2. Pull the update

**Direct install** (origin is agent-ways):

```bash
git -C "$APP" pull --ff-only
```

**Fork** (origin is your fork, `upstream` tracks agent-ways):

```bash
git -C "$APP" remote get-url upstream >/dev/null 2>&1 \
  || { echo "No 'upstream' remote. Add it: git -C \"$APP\" remote add upstream https://github.com/aaronsb/agent-ways"; exit 1; }
git -C "$APP" fetch upstream && git -C "$APP" merge upstream/main   # resolve conflicts in custom ways
```

### 3. Force-rebuild the binaries

```bash
make -C "$APP" update-binaries
```

`make update-binaries` **force-rebuilds** ways/attend/attend-chat/way-embed. Do
**not** use `make setup` here: `setup` skips any binary that already exists and runs
(`ways already installed`), so after a pull it would leave the compiled binary
**stale** — the whole point of the update. Individual targets are re-runnable if one
fails (`make -C "$APP" ways-rebuild`, `attend-rebuild`, …).

The embedding corpus regenerates lazily on the next session (`ways corpus --if-stale`
in the `SessionStart` hook). To refresh it now — e.g. ways changed — run
`make -C "$APP" setup` (binaries already fresh; this regenerates the corpus) or
`"$APP/bin/ways" corpus`.

### 4. Refresh the projection

```bash
"$APP/bin/ways" reconcile --source "$APP" --dest "$HOME/.claude"
```

Idempotent and silent when nothing structural changed; relinks any new projected
root and re-runs the `settings.json` merge. (Called by absolute path — like
`install.sh` does — so it works even if `~/.local/bin` isn't on `PATH`.)

### 5. Tell the user to restart

The running session won't pick up new hooks or ways until restart. End with an
explicit: **"Restart Claude Code for the update to take effect."**

## Verify (optional)

```bash
ways --version          # confirm the rebuilt binary is on PATH
ways status             # binary / model / corpus / project detection
make -C "$APP" test     # full suite: lint + smoke + unit + sim + lang (slow)
```

## Notes

- This skill is itself a **projected** file (`skills/` links into `~/.claude/skills/`),
  so it can update the app that defines it — it only ever runs `git`/`make`/`ways`,
  never edits repo contents.
- For a first-time install (not an update), run the installer one-liner instead —
  this skill assumes an existing app checkout at `$XDG_DATA_HOME/agent-ways`.
- See `docs/development.md` for the install-vs-dev-checkout distinction, and
  `docs/migration-1.0.md` for moving a legacy in-place clone onto the projection.
