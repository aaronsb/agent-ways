---
status: Accepted
date: 2026-09-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-131
  - ADR-142
  - ADR-144
  - ADR-185
---

# ADR-184: Installation and activation are separate states: targets as the unit of activation

## Context

ADR-142 made `~/.claude` a thin projection of an application that lives under `$XDG_DATA_HOME/agent-ways`. The projection has one destination, and that destination is a constant: the installer, the reconciler's default, and all 32 shipped hook commands name `~/.claude` by path. Installing and activating are one act. The installer stages the source, builds the binaries, and immediately projects into the one place it knows.

Claude Code does not have one place. `CLAUDE_CONFIG_DIR` relocates its whole config directory, and operators use that to keep profiles apart, switched per folder. A first install on such a machine landed in the default directory, replaced a real skills directory there, and dropped the user's own hooks from settings, because the installer chose a destination without saying so and the reconciler treated whatever it found there as its own. PRs #501 and #502 fixed the two data-loss paths. They did not change the fact that the destination is chosen silently.

Two further needs came with the same report: wiring ways into one profile or one repository rather than everywhere, and turning the system off to judge it. Neither has a form today. There is no uninstall.

## Decision

**Installation and activation are separate states. A target is a Claude Code config directory named in the user config. The reconciler converges every enabled target and withdraws from every disabled one. The installer never activates.**

1. **Three states.** Absent: nothing staged. Installed: source staged under XDG data, binaries built and on PATH, no target enabled. Active: at least one enabled target. Every user-owned file is untouched in the first two states. `ways status` names the state and the targets.

2. **Targets live in the user config.** `config.yaml` under the agent-ways config root gains a `targets` list. Each entry names a config directory and carries `enabled` and `observe` flags. When the key is absent the list is one implicit entry, the default config directory, enabled. An install that predates this decision keeps working unchanged, and its next bare reconcile records the implicit entry as the explicit one, so an existing install becomes explicit without an operator act. Writing the key makes the list explicit and the implicit entry stops.

   **Each target carries its own configuration set.** A target's `config.yaml`, at `targets/<key>/config.yaml` under the agent-ways config root or at the path its entry names, holds the same keys as the user config and is layered over it for every session running under that target's config directory. The session names its target through `CLAUDE_CONFIG_DIR`, with the default directory as the fallback. The `targets` key itself is read from the user layer only, so a target cannot redirect the list. Two profiles on one machine can run different languages, disabled domains, or thresholds without sharing them.

3. **Verbs on `ways config`.** `targets` lists entries with their converged state. `target add`, `enable`, `disable`, and `remove` edit the list and reconcile. When the list is empty, `targets` runs the bootstrap: discovery, candidates, the plan per target, confirmation. The installer's last act is to invoke it or, unattended, to print it.

4. **Reconcile converges per target.** An enabled target receives the projection roots and the hooks merge into its `settings.json`, with a merge base kept per target under the state root. A disabled target is withdrawn: our symlinks are unlinked, our hooks block is removed through the same three-way merge base that wrote it, and nothing else in the directory is touched. Reconcile is idempotent in every state, so `ways update` is safe wherever the operator stopped.

5. **`observe` is orthogonal to `enabled`.** Transcript-derived readers resolve their roots from the targets with `observe` true, defaulting to the value of `enabled`. A disabled target can stay in reports; an enabled one can stay out of them.

6. **Per-project off is a one-line switch.** `enabled: false` in a project's `ways.yaml` (the file ADR-131 already uses for per-way disable) makes the hook entry point exit before any scan. Hooks still spawn; nothing is injected.

7. **Discovery is heuristic and never a verdict.** Candidates come from three signals: `CLAUDE_CONFIG_DIR` in the environment, shell rc files, and direnv's allow records; directories under the home and XDG config roots with the shape of a Claude Code config dir; and the environment of running `claude` processes. Attended mode confirms them. Unattended mode with more than one candidate and no explicit list exits with the literal command that would resolve it.

8. **One bus across targets.** Attend's signals, channels, instance roster, heartbeats, and state live under the user cache, keyed by the user, never by a config directory. Agents in sessions under different targets are peers on the same bus. What attend resolves through a config directory, its own session identity and its peers' from the session records, walks every target's directory plus the one `CLAUDE_CONFIG_DIR` names, so a session under a relocated profile is a full peer and never a fallback identity.

9. **Unattended stays.** The installer keeps a flag for pipelines. It stages and builds, reads the targets list, and reconciles. With no list and one candidate it activates that one. With no list and several it stops with the hint. It never guesses.

Reversibility: expensive. The implicit-target compatibility in item 2 keeps every existing install on the old behavior until it writes the key, so the model can be withdrawn by removing the verbs and leaving the key ignored. After the installer hands off to the bootstrap, reversing means restoring the installer's own projection step.

This decision amends ADR-142's single projection destination and ADR-144's manifest, which described the desired state of one directory and now describes the desired state of each target. Neither is superseded whole.

## Consequences

### Positive

- The installer makes no choice on the operator's behalf. Nothing user-owned changes until a target is named.
- Withdrawal exists. Disabling a target is a verb, and it restores the directory through the same base that changed it.
- Per-profile and per-repository installs become entries in one list. Issue #503 and PR #504 reduce to a second target and a hooks-only slice on a target.
- Leaving the installer early leaves the system installed and inactive, which is a valid state with a one-line resume.
- Judging the system is a switch and a query: `observe` plus the config-dir stamp on events makes sessions with ways on and off comparable.

### Negative

- A second target is not honest until the hook commands and scripts stop naming `~/.claude`. Issue #503 is the prerequisite, and until it lands the only honest target is the default directory.
- Withdrawal through the merge base cannot remove hooks written before a base existed. The first apply after this decision seeds one, and PR #502 narrowed what that seed claims.
- Discovery reads shell rc files and the process table. Both are heuristics, and the attended flow has to make that visible rather than present a candidate as a fact.

### Neutral

- The deployment way loses its clobber branch. On a fresh install there is no decision for a guiding Claude to make, only the bootstrap to point at.
- The statistical readers change one resolver from singular to plural. Events gain a config-dir field so the filter in item 5 has something to key on.
- ADR-185 sets the output contract the new verbs follow.

## Alternatives Considered

- **Infer scope from state files on a bare reconcile.** PR #504's design. Rejected: a mode inferred from disk on every update is a new failure class, and the review found it flips when the recorded directories are gone. The targets list is the same state made explicit.
- **Keep the installer projecting, with a prompt.** Rejected: the prompt lives in bash, `make setup` and the one-liner stay two code paths, and an unattended run still has to guess.
- **A separate `ways target` noun.** Rejected in favor of `ways config`: the list is user-scope configuration that survives updates, and it belongs in the file that already holds the other user-scope switches.
- **Uninstall as a script.** Rejected: withdrawal through the merge base is the only method that knows what we wrote, and it already exists for the forward direction.
- **Honor `CLAUDE_CONFIG_DIR` only, no list.** Rejected: it gives one destination per invocation and no record of which directories are active, so update, status, and withdrawal have nothing to iterate.
