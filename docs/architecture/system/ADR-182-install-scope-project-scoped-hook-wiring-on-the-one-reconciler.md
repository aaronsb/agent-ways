---
status: Draft
date: 2026-09-16
deciders:
  - forayconsulting
  - aaronsb
related:
  - ADR-142
  - ADR-144
  - ADR-148
  - ADR-169
  - ADR-179
---

# ADR-182: Install scope: project-scoped hook wiring on the one reconciler

## Context

ADR-142 made `~/.claude` a projection of the XDG application, and `ways reconcile` merges the hooks block into the user's `~/.claude/settings.json`. That is the only place hooks land, so an install is active in every Claude Code session in every directory. There is no supported way to enable ways for one repository and leave the rest of the machine as it was.

Claude Code reads hooks from three settings files and concatenates them: the user's `~/.claude/settings.json`, the project's `.claude/settings.json`, and the project's `.claude/settings.local.json`. The `.local.json` layer exists only at project scope (ADR-169 records this at lines 72 to 79), Claude Code writes it itself, and `ways init` already gitignores it as developer-local. A hooks block placed there fires only in sessions under that directory. Nothing in the binary or the docs uses this.

The case that surfaced the gap. An operator runs two Claude Code profiles, switched per folder by direnv through `CLAUDE_CONFIG_DIR`. The second profile symlinks `skills/`, `agents/`, `commands/`, and `settings.json` back into `~/.claude`, so skills and hooks are shared while sessions and credentials stay apart. About twenty hand-built skills live in `~/.claude/skills`, and three custom hooks live in `settings.json`. They wanted ways active in one repository. Before the guard in #501, the stock install would have deleted the skills directory; before #502, the first merge would have removed the three hooks. What they installed by hand, and what works, is this: the hook tree and the binaries linked into `~/.claude`, and the hooks block written into `<repo>/.claude/settings.local.json`. That shape is the decision below, made a supported mode of the one reconciler.

Two facts constrain the shape. Every shipped hook command in `settings.json` is written as a literal `${HOME}/.claude/...` path (32 commands), and the hook scripts locate the binary the same way (`require-ways.sh:10`, `check-setup.sh:8` and `:21`). So the hook tree and the binaries have to be reachable at `~/.claude` regardless of where the hooks block lives. Relocating them is a separate, larger change.

## Decision

**Install scope is a flag on the one reconciler (ADR-144 section 2): `ways reconcile --scope project [--project DIR]`.**

Project scope does four things.

1. **Projects only the hook roots.** `hooks/ways`, `hooks/check-config-updates.sh`, and `bin/*` are linked into `~/.claude` as today. `skills/`, `agents/`, and `commands/` are left alone. `manifest::hook_roots` is `projection_roots` filtered to those units, so no new root is invented.
2. **Merges hooks only.** The hooks block goes into `<project>/.claude/settings.local.json` through the same three-way merge, backup, atomic write, and self-audit as user scope (`settings_merge::apply_to_files_with(..., Slices::HooksOnly)`). The `permissions.allow` entries and the secret-path `permissions.deny` baseline are user-level policy and are not written to a per-repo file.
3. **Keeps a per-project base.** The last-applied record lives at `$XDG_STATE_HOME/agent-ways/projects/<key>/settings-applied.json`, keyed the way the corpus keys projects (`util::encode_project_key`), with the original path recorded beside it. A shared base would misattribute one project's entries to another. First-apply seeding claims only entries that are structurally ours (#502), so a project file the user already authored keeps its hooks.
4. **Remembers the scope.** A bare `ways reconcile`, which is what `ways update` runs, infers project scope when no user-scope base has ever been written and project bases exist, and re-merges every recorded project. Without this, an update would silently convert a project-scope install into a user-scope one, the outcome the operator chose against. `--scope user` and `--scope project` override the inference.

The installer passes `--scope=project [--project=DIR]` through.

**Reversibility:** reversible. Removing the hooks block from `settings.local.json` and the three links from `~/.claude` restores the prior state; nothing else was written. It becomes expensive only if a later decision moves the hook roots out of `~/.claude`, which would change the command paths this scope relies on.

## Consequences

### Positive

- Ways can be enabled per repository. The rest of the machine is untouched, including every other project's sessions.
- `~/.claude/skills`, `agents`, and `commands` remain the user's. This follows ADR-169's reading that `~/.claude` is dotfiles-class and agent-ways should own as little of it as it can.
- No new topology and no second engine. The same manifest, the same reconciler, the same merge; ADR-140's subdirectory install stays superseded.
- The user-scope `settings.json` is never written in project scope, so a machine can run project scope without agent-ways appearing in user settings at all.

### Negative

- A project-scope operator forgoes the shipped skills, agents, and commands (`ways-update`, `ways-tests`, `ways-localize`, `/ways`, `/project-init`, `/project-audit`, the skeptic agent) unless they copy what they want into `<project>/.claude/skills/`. Updates run as `make update` in the app dir followed by `ways reconcile`.
- `bin/` and `hooks/ways` still land in `~/.claude`. The projection is smaller, not absent.
- `settings.local.json.bak` appears in the project's `.claude/`. `ways init` gitignores it for new projects; existing projects add the line themselves.
- The scope inference in a bare `ways reconcile` is a heuristic over state. Its edge is a machine that had a user-scope install, removed it by hand without deleting the user base, and then wants project scope; the explicit flag handles that.

### Neutral

- `check-setup.sh` still finds `~/.claude/hooks/ways` and reports engine health as before.
- `ways init` still scaffolds `.claude/ways/` in every git repo at session start; this ADR does not change it.
- `ways corpus`, the way roots (ADR-143), and the matching engine are unchanged.

## Alternatives Considered

- **A user-config allowlist of project paths** (`config.yaml`). Every hook would still run in every session and return early; `core.md` injection and `ways init` would each need their own gate; the allowlist would be machine-specific. Rejected: it gates output where this decision gates installation.
- **`when: project:` on each way** (`frontmatter-schema.yaml:106` to `:114`). Per way, exact path, baked into the way file, and undocumented in the guides. It is a way-author tool, not an install switch. Rejected.
- **A marketplace-plugin bootstrap** (ADR-144 alternatives). Plugin enablement is per user, so it would still be a whole-machine switch, and it still writes into `~/.claude`. Rejected for this decision; still viable as a distribution channel.
- **Relocate the hook tree and binaries into `<project>/.claude`.** Fully self-contained, but it rewrites all 32 shipped command strings and about twenty hook scripts, and it puts hook paths inside a tree that is replaced wholesale on update, the case ADR-144 section 3 warns about. Deferred; project scope as decided here does not preclude it.
- **Honor `CLAUDE_CONFIG_DIR`.** Orthogonal: it changes where the projection lands, not which sessions the hooks fire in. Tracked as a follow-up (#503).

## Follow-ups

- `CLAUDE_CONFIG_DIR` for the projection root, the transcript lookups, and the hook command paths (#503).
- Copy mode (ADR-142 section 2) so a real directory holding only shipped files can be replaced safely instead of refused (#501 trade-off).
- An uninstall or relinquish path for both scopes, per the peer-writer contract's ownership handoff section.
- A user-scope home for personal skills alongside the projection; ADR-143 gives the operator a user root for ways only.
