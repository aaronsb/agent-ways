---
contract: adr/v1
kind: decision
verb: change
capability: [install, cli]
amends: ADR-507#Decision
basis:
  - operator: aaronsb
    level: directed
    said: "one of the other things we had planned on doing was reducing the number of top level commands for lifecycle management. (basically, just have one, then subcommands to update, repair, uninstall, reinstall)"
    via: session chat, 2026-10-10
  - operator: aaronsb
    level: directed
    said: "ways manage? I don't like 'install uninstall' thats weird"
    via: session chat, 2026-10-10
  - evidence: 'ways --help at main 03eba8a5 lists four lifecycle entries at the top level: target, update, reconcile and uninstall (tools/ways-cli/src/main.rs Commands; tools/ways-cli/tests/cli_shape.rs:30-31 pins the list)'
  - evidence: 'the updater that runs is the installed release: tools/ways-cli/src/cmd/update.rs:529 and :656 spawn the newly installed binary with `reconcile`, and a failed step stops the update'
  - evidence: 'ways status (tools/ways-cli/src/cmd/status.rs:174-323) prints one Install line among engine, gate, language, binaries, model, corpus, calibration, way counts, toggles, settings findings and projects'
  - evidence: 'the settings screen spawns `target plan` on the running binary (tools/ways-cli/src/cmd/settings/tui/mod.rs:488); a bare `ways target` on a terminal opens the settings install tab (tools/ways-cli/src/main.rs:1309)'
  - evidence: 'ADR-198 item 3 deletes the app, its binaries included, last; a reinstall cannot run the deleted binary to finish'
  - evidence: 'issue #717, the consolidation round''s closing check (ADR-506 §5), is open at 03eba8a5'
  - precedent: ADR-507
  - precedent: ADR-144
  - precedent: ADR-184
  - precedent: ADR-198
  - precedent: ADR-179
  - precedent: ADR-506
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-10
deciders:
  - aaronsb
related: [ADR-507, ADR-506, ADR-144, ADR-184, ADR-198, ADR-179, ADR-186, ADR-150]
---

# ADR-509: Lifecycle commands sit under one ways manage group; the updater's reconcile stays a hidden fixed name

## Summary

- **Decided:** every agent-ways lifecycle command moves under one top-level group, `ways manage`, with the subcommands `update`, `repair`, `uninstall`, `reinstall` and `target`. `reinstall` is new: a plan, then an uninstall that keeps what the operator owns, then a fresh install. The old top-level `update`, `uninstall` and `target` remain for one release as hidden aliases that print the new form, and are then removed. `reconcile` is the exception. It leaves `ways --help` but stays a hidden fixed name, because the previous release's updater calls it on the new binary. Top-level `status` stays as engine health, and there is no `manage status`.
- **Trades away:** operators lose `ways update` and `ways target` as two-word commands. For one release the binary carries alias code, a small, bounded exception to ADR-506's no-compatibility rule. The one top-level command is not literally one, because `reconcile` stays callable for the updater.
- **One-way?** No. The group can be renamed or flattened in one session. The only lasting commitment is the fixed name `reconcile`, which ADR-507 already made.
- **Probes:** *Not confident (target):* you think of choosing which Claude Code directories agent-ways is active in as part of managing the install, so `ways manage target add <dir>` reads right and is not one level too deep. *Confident (window):* one release of the old names still working, with a note naming the new form, is long enough for your habits and the agents' habits to move.
- **Inversion:** one end keeps every lifecycle verb at the top level, where nothing is renamed and the top level mixes operator lifecycle with daily use. The other end moves every lifecycle name at once with no aliases and no fixed names, so an update from the previous release fails at its last step. This decision groups everything a person types and keeps the one name another release's process calls.

## Context

ADR-507 cut `ways --help` from 37 commands to 14. Four of those 14 are about the install, not about using ways: `target` (ADR-184), `update`, `reconcile` (ADR-144) and `uninstall` (ADR-198). A reinstall has no command at all. Today it is `ways uninstall --yes` followed by the installer one-liner, and the operator has to know that the binary they just ran is gone.

The operator asked for one lifecycle command with subcommands, and named it `ways manage`, with `install uninstall` rejected as a pairing.

Who calls each name matters for any move, as ADR-507's Context set out:

| Name | Callers at 03eba8a5 |
|---|---|
| `update` | The operator; the ways-update skill; the Makefile's transitional `update` target (`./bin/ways update`, already due for removal by #717); hint text in `attend` and in several ways |
| `reconcile` | The **previous release's updater**, which spawns it on the newly installed binary (`update.rs:529,656`); the installer (`install.sh:338,401`); repair text in `check-setup.sh`; the live fixture; the operator |
| `uninstall` | The operator |
| `target` | The operator; the settings screen, which spawns `target plan` on its own binary; the live fixture |

The updater is the only caller from another release. ADR-507 §2 fixed `reconcile` for that reason. An alias in the new binary covers an update from the release just before. Once the alias is removed, an install that skipped the alias release would run its own updater, which spawns `reconcile` on a binary that no longer has it. The binary would be installed, but the projection would not be refreshed and the update would report a failure.

## Decision

1. **One group.** `ways manage` holds the lifecycle commands:

   | Command | Was | What it does |
   |---|---|---|
   | `ways manage update` | `ways update` | Pull the app source, refresh the binaries, rebuild the corpus, reproject (ADR-150) |
   | `ways manage repair` | `ways reconcile` | Converge every enabled target and withdraw from every disabled one (ADR-144, ADR-184). Same flags: `--force`, `--dry-run`, `--quiet`, `--source`, `--dest` |
   | `ways manage uninstall` | `ways uninstall` | ADR-198, unchanged: plan without `--yes`, keep the operator's config and state unless `--purge` |
   | `ways manage reinstall` | none | Item 3 |
   | `ways manage target …` | `ways target …` | ADR-184's verbs, unchanged: `list`, `plan`, `add`, `enable`, `disable`, `remove` |

   `ways --help` lists `manage` once, in place of ADR-507's `target`, `update`, `reconcile` and `uninstall`. The top level drops from 14 commands to 11.

2. **Bare `manage`.** On a terminal, a bare `ways manage` opens the settings screens on their install tab, which is what a bare `ways target` does today. In a pipe it prints its help and exits 2, per ADR-507 §7. The call-site check lists `manage` among the groups a script must give a verb.

3. **Reinstall.** `ways manage reinstall` puts the same version back cleanly while keeping what the operator owns. Use it when an install is damaged, or when a cache or a stale projection is suspected.
   - Without `--yes` it prints the plan and changes nothing. The plan is ADR-198's uninstall plan (withdraw, unlink, delete, keep), followed by the install it will run and the version and source it will use.
   - It reinstalls the version that is installed, from the source the install came from: the release, or the git remote and ref of a source build. Moving to another version is `manage update`.
   - It fetches and verifies the new copy into a staging directory before it removes anything. If the fetch or the verification fails, nothing has been removed.
   - It then runs ADR-198's uninstall without `--purge`, and then the staged installer, which projects every target the kept config lists. `--purge` is not accepted, because a reinstall that deletes the operator's ways and keys is an uninstall followed by a fresh install, and those are separate commands.
   - If the install step fails, the staging directory is kept and the command prints the one line that finishes the install from it.

4. **`reconcile` stays a hidden fixed name.** `ways reconcile` keeps running the same code as `ways manage repair`, with the same flags and exit codes. It prints no notice, is absent from `ways --help` and from shell completion, and joins `hook`, `init`, `corpus` and the other fixed names of ADR-507 §2. The updater keeps spawning `reconcile`, because that name works on every successor binary. Everything a person reads (help, hints, skills, ways, docs, the installer's messages) says `ways manage repair`. Removing `reconcile` would need its own decision and a release in which nothing calls it, as ADR-507 §2 says. No such release exists while installs update themselves with their own updater.

5. **One-release aliases.** `ways update`, `ways uninstall` and `ways target …` stay as hidden aliases for the release that ships `manage`. Each one prints one line to stderr, naming the new form (``note: `ways update` is now `ways manage update` ``), and then runs it with the same arguments and exit code. They are absent from `ways --help` and completion. The next release removes them. Under ADR-506 §2, the PR that adds them names the issue that removes them, in its body and in a code comment. If the consolidation round's closing check (#717) runs before that release, it lists the aliases as known transitional code with that issue as their removal, and does not treat them as leftover compatibility. This follows ADR-179's retirement of `ways migrate`: the old form is announced, then removed, and the release notes carry the mapping.

6. **`status` stays top-level.** `ways status` reports engine health: the engine, gate, model, corpus, calibration, way counts, toggles, settings findings and projects. The install is one line of that report, so the command serves more than the install, and it stays where it is. Its Install line remains a one-line summary and points to `ways manage target list` for the per-target detail. A `manage status` would print a subset of `status` beside `target list`, so it is not added.

7. **Every caller moves in the same change.** Rust, scripts, skills, ways, docs, tests and the live fixture move to the new names in the PR that adds `manage`. The aliases in item 5 are for people's habits, not for callers in the repository. The call-site test of ADR-507 fails on any caller left on an old name, apart from the updater's `reconcile` spawn and the alias tests themselves.

This amends ADR-507's Decision: item 1's top-level table replaces `target`, `update`, `reconcile` and `uninstall` with `manage`, and item 3 adds `reconcile` to the hidden commands. It renames the commands that ADR-144, ADR-184 and ADR-198 introduced and does not change what they decide.

## Consequences

### Positive

- One place to look for anything that changes the install: `ways manage --help`.
- Reinstall becomes one command, which fetches before it deletes, so a failed fetch no longer leaves a machine with no `ways` at all.
- `ways --help` drops to 11 commands, all of them for using ways except `manage`.
- An update from the previous release, or from any older one, still completes, because the name its updater calls never goes away.

### Negative

- `ways manage target add <dir>` is three words where `ways target add <dir>` was two. A target is changed rarely, so the cost is small, but it is the first probe.
- For one release the binary carries alias code that ADR-506 would otherwise forbid. It is bounded by a named removal issue.
- `reconcile` exists twice, as `manage repair` and as a hidden name. The two run one function, so they cannot drift, but a reader of the source sees both.
- Release notes, the install guide and every agent's habit have to move. A user on the release after the alias release who types `ways update` gets a usage error.
- `reinstall` needs the network and enough disk for a staged copy beside the installed one.

### Neutral

- The `Bash(ways:*)` permission covers every new name.
- `corpus` and `init` stay top-level and fixed (ADR-507 §2). They maintain the engine and the project, not the install.

### Call sites to change (inventory at 03eba8a5)

Found with `grep -rnE '\bways (update|reconcile|uninstall|target)\b'` across the tree, then checked by hand. Counts are lines.

**Executable callers**

- `tools/ways-cli/src/main.rs`: the `Commands` enum and the bare-group dispatch at :1309 (`target` becomes `manage`).
- `tools/ways-cli/src/cmd/update.rs:525-529, 652-656`: keep spawning `reconcile` (item 4). Its 13 messages naming `ways update` or `ways reconcile` move.
- `tools/ways-cli/src/cmd/settings/tui/mod.rs:488`: spawns `target plan` and becomes `manage target plan`.
- `scripts/install.sh:338, 401`: run `bin/ways reconcile --source … --dest …` and become `manage repair`. The binary comes from the same checkout. Its messages at :14, 62, 194-201 and 348 move as well.
- `Makefile:6, 44, 146, 150, 168, 172-173, 180`: comments, help text and the transitional `update` target, which #717 removes anyway. Until then it runs `./bin/ways manage update`.
- `hooks/ways/check-setup.sh:29, 88`: repair text that runs `make setup` and then the fresh binary, which becomes `manage repair`.

**Messages in other code**

- `tools/attend/src/lib.rs:171, 183` (`ways update` hint); `hooks/ways/softwaredev/environment/attend/macro.sh:7`.
- `tools/ways-cli/src/cmd/`: `agent.rs`, `judge.rs`, `mcp_register.rs`, `memory_seed.rs`, `reconcile.rs` (5), `scan/sidecar.rs`, `settings_merge.rs`, `show/metrics.rs` (3), `status.rs` (5), `target.rs` (6), `template.rs`, `uninstall.rs` (3), `settings/tui/build.rs` (5), `settings/tui/flows.rs` (5).
- `tools/ways-core/src/settings.rs:88, 90, 313`; `config.rs:264`, `paths.rs:224` (comments).
- `tools/agent-tui/src/app/response.rs`.
- `scripts/update.sh:2`, `scripts/fix-way-embed-signature.sh:57`, `tools/scripts/prebuilt-lib.sh:9`, `hooks/check-config-updates.sh:11` (comments and hints).

**Skills and ways**

- `skills/ways-update/SKILL.md:3, 21, 26, 31, 109`, including its guarded `"$APP/bin/ways" reconcile` step.
- `hooks/ways/meta/deployment/deployment.md` (12 lines, including the targets section).
- `hooks/ways/meta/knowledge/knowledge.md`, `hooks/ways/meta/skills/skills.md`, `skills/attend/SKILL.md`, `.claude/ways/meta/project-health/project-health.md`.

**Tests and the live fixture (ADR-186)**

- `tools/ways-cli/tests/cli_shape.rs:30-31, 65`: the top-level list and the group-verb check. It gains `manage`, and an alias test that checks the notice and the forwarded exit code.
- `tools/ways-cli/tests/one_ways_roots.rs`, `tools/ways-cli/tests/settings_cli.rs`, `tools/agent-tui/src/app/tests.rs`, `tools/ways-cli/src/cmd/settings/tui/tests.rs`.
- `tools/ways-cli/tests/fixtures/settings-tui/install-{apply-error,browse,plan-error,plan-report,review}.frame`: golden frames that show the commands.
- `tests/hooks-test.sh:227-228, 291`, `tests/make-link-test.sh:4`.
- `tests/fixtures/docker/run-tier1.sh:173-174, 210, 216, 231-239` and `tests/fixtures/docker/README.md`. CI reaches them only through `make test-live TIER=1` (`.github/workflows/live-fixture.yml:39`, `portability.yml:91`). No workflow names a lifecycle command directly.

**Docs**

- `README.md` (10), `docs/install-guide.md` (12), `docs/reference/ways-cli.md` (30), `docs/reference/ways-cli-renames.md` (gains the four rows), `docs/development.md` (4), `docs/hooks-and-ways.md` (3), `docs/hooks-and-ways/extending.md` (2), `docs/hooks-and-ways/meta.md`, `docs/architecture.md`, `docs/attend-and-monitor/first-sensor.md`, `docs/how-to/fix-way-embed-sigkill-macos.md`, `docs/reference/statusline.md` (2).

**Left as they are**

- Accepted records under `docs/architecture/` and the changelogs, which are append-only.
- `tests/fixtures/docker/scenarios/adr-migrate/v0/*`, which are frozen copies of records used as test input.
- `tools/scripts/fire-panel.json` and `docs/research/yesno-relevance-gate/scripts/data/session_labels.py`, which quote past prompts as data.

## Alternatives Considered

- **Keep the lifecycle verbs at the top level (ADR-507 as it stands).** Not chosen. The operator asked for one lifecycle command, and the four verbs are the part of ADR-507's top level a person uses least often.
- **`ways install` as the group, with `install uninstall`, `install update` and so on.** Rejected by the operator: `install uninstall` reads wrong. `manage` names the act without implying one direction.
- **Move `reconcile` like the others: alias for one release, then remove.** Rejected. After removal, an install that skipped the alias release fails its next update at the last step, with the new binary installed and the projection unrefreshed. Recovery would need the operator to know to run `ways manage repair`. Keeping the name costs one hidden entry.
- **Have the updater spawn `manage repair` and fall back to `reconcile`.** Not chosen. The updater that matters is the old one, which cannot be changed, and a fallback in the new updater is the compatibility path ADR-506 removes.
- **No aliases, as ADR-507 §5 chose.** This was a real option, since every caller in the repository moves in the same change. Not chosen, because these four are typed by people and quoted in agents' habits and in a lot of external text, more than ADR-507's authoring commands. The window is the second probe.
- **Leave `target` at the top level.** This was a real option: targets are configuration as much as lifecycle, and the settings screen already presents them on its install tab. Not chosen, because activation is the install's state (ADR-184 item 1), and the operator listed lifecycle as one group. It is the first probe.
- **Add `ways manage status`.** Not chosen (item 6). It would repeat `status`'s Install line and `manage target list`.
- **Reinstall as uninstall followed by the installer one-liner, run from the shell.** Not chosen. It deletes before it fetches, so a network failure leaves no `ways` on the machine.
