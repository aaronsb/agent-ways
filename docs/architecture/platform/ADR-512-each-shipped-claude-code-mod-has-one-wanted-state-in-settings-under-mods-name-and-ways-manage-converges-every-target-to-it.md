---
contract: adr/v1
kind: decision
verb: change
capability: [install, config, cli]
amends:
  - ADR-406#Decision
  - ADR-708#Decision
basis:
  - operator: aaronsb
    level: guided
    said: "should we make 'ways manage ...' lifecycle the mod?"
    via: "chat in the agent-ways session, 2026-10-22; relayed verbatim by the calling session"
  - operator: aaronsb
    level: guided
    said: "right now option 7, mod only configures settings of agent ways. we should add another level, so we can manage the mods (install/uninstall)"
    via: "chat in the agent-ways session, 2026-10-22; relayed verbatim by the calling session"
  - operator: aaronsb
    level: guided
    said: "(or repair/check too)"
    via: "chat in the agent-ways session, 2026-10-22, following the previous line; relayed verbatim by the calling session"
  - operator: aaronsb
    level: guided
    said: "I think mods.attend.* (mods.name.*) is the right way"
    via: "chat in the agent-ways session, 2026-10-22; relayed verbatim by the calling session"
  - evidence: "2026-10-22, reported by the calling session: on a host reinstalled that day, `gate.engine: claude-code` was set while Claude Code was below the ways-judge mod's floor (2.1.304); no `ways-judge-installed` record was written, so neither a later `ways manage repair` nor `ways manage update` installed the mod after Claude Code was upgraded, and the relevance gate failed open until `ways judge-setup` was run again by hand"
  - evidence: "the code at 86d6c2e6: `Mod.record` and the `ways-judge-installed` file with its `pending` line keep ways-judge installed (tools/ways-cli/src/cmd/attend_mod.rs:55-57, 333-393); reconcile converges ways-judge only where that record exists (tools/ways-cli/src/cmd/reconcile.rs:514); `converge` checks no Claude Code floor, and `host_can_load` is called only from `ways judge-setup` (tools/ways-cli/src/cmd/judge.rs:342); a ways-judge removed with Claude Code's own CLI is forgotten and not reinstalled (attend_mod.rs:500-518)"
  - evidence: "the code at 86d6c2e6: repair already keeps per-target state beside the settings merge base (`state_file`, tools/ways-cli/src/cmd/attend_mod.rs:319-321; `base_path_for`, tools/ways-cli/src/cmd/reconcile.rs:260-267): the `attend-mod-marketplace` record, written after each successful marketplace call (attend_mod.rs:323-327, 443-458), and the record's `pending` line, set after each install, uninstall or marketplace removal this app makes (attend_mod.rs:345-365, 436-441), which is how today's code tells a removal it made from one the operator made. The merge base itself holds only hooks and permissions (tools/ways-cli/src/cmd/settings_merge.rs:75-82), and Claude Code drops a plugin's `enabledPlugins` entry on uninstall, so neither says a plugin was ever installed"
  - evidence: "the code at 86d6c2e6: `gate.engine` is a key of the agent file, read from the user layer only (tools/ways-agent-core/src/settings.rs:12, 58), while the `attend_mod` section and `attend_mod.install` (Scope::Machine, user and target layers) are in the ways config (tools/ways-core/src/settings.rs:117-125, 358-369); the settings gate screen and `ways settings set` write `gate.engine` with no change to the mod"
  - evidence: "the code at 86d6c2e6: `ways settings fix` leaves a top-level key no section owns for a hand edit (tools/ways-cli/src/cmd/settings/maintain.rs:171-172), and the retired-key table only names a successor in lint's message (tools/ways-core/src/settings.rs:131-135, tools/agent-settings/src/load.rs:276-283)"
  - evidence: "the code at 86d6c2e6: the attend mod and attend's keepwarm policy read their settings by spawning `ways settings list attend_mod --json --effective` (plugins/attend/hooks/lib.ts:106, tools/attend/src/cmd/keepwarm.rs:179); when the section is missing the mod keeps its defaults, dormant and keepalive off (lib.ts:345-365), and the policy check reads nothing and drives no wake"
  - upstream: "Claude Code's plugin CLI: `claude plugin install|uninstall|update|disable` at user scope, recorded in `enabledPlugins` of the config directory's settings.json (ADR-406 item 8's scratch run, 2026-10-10)"
  - precedent: ADR-406
  - precedent: ADR-708
  - precedent: ADR-509
  - precedent: ADR-511
  - precedent: ADR-184
  - precedent: ADR-506
  - precedent: ADR-507
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Yes, keep retrying"
    via: "choice tool in the agent-ways session, 2026-10-22, relayed verbatim by the coordinator; option label and text written by the agent, chosen by the operator; option text: \"A wanted mod stays pending below the floor, and every repair or update tries it again.\""
    covers: [heal]
  - operator: aaronsb
    said: "No, respect a by-hand uninstall"
    via: "choice tool in the agent-ways session, 2026-10-22, relayed verbatim by the coordinator; option label and text written by the agent, chosen by the operator; option text: \"A by-hand uninstall makes repair stop keeping that mod, which is how ways-judge behaves today.\" The record was revised for it: repair writes install false in that target's config (Decision item 3)"
    covers: [by-hand]
  - operator: aaronsb
    said: "Report only"
    via: "choice tool in the agent-ways session, 2026-10-22, relayed verbatim by the coordinator; question on files that still hold attend_mod, not a Summary probe; option label and text written by the agent, chosen by the operator; option text: \"Lint and update name every file that holds it; you run `ways settings fix` yourself.\" The record was revised for it (Decision item 2)"
status: accepted
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-144
  - ADR-198
  - ADR-414
  - ADR-501
  - ADR-503
  - ADR-504
  - ADR-510observable:
  - "`ways settings list mods` shows `mods.attend.*` with attend's keys and `mods.ways-judge.install`; `ways settings list attend_mod` reports no such section"
  - "A user config, a target config and a known project's `.claude/ways.yaml` each holding `attend_mod`: `ways settings lint` and `ways manage update` (what-if and `--commit`) name all three files and the fix command, and none of the files changes; `ways settings fix mods.attend` (with `--project <dir>` for the project) then leaves `mods.attend` holding the same values and no `attend_mod`, with the file's comments kept"
  - "On a host whose Claude Code is below 2.1.304, `ways settings set gate.engine claude-code` writes `mods.ways-judge.install: true` and `gate.engine: claude-code`; `ways manage repair --commit` reports the ways-judge mod pending with the floor and changes nothing for it; after Claude Code is upgraded, `ways manage update --commit` installs it with no other step"
  - "Choosing an Anthropic key in `ways judge-setup` while `gate.engine` is claude-code writes `gate.engine: anthropic`, then `mods.ways-judge.install: false`, and the next repair uninstalls the mod from every enabled target"
  - "After `claude plugin uninstall attend@agent-ways` by hand in one target, `ways manage repair --commit` does not reinstall it, writes `mods.attend.install: false` in that target's config, reports the write, and leaves every other target's attend installed; `ways manage mod install attend --commit` then removes that override and installs it again"
  - "On a fresh target where the attend mod was never installed, or where its install failed, repair installs it: absence alone is never read as a removal by hand"
  - "After `claude plugin uninstall ways-judge@agent-ways` by hand with `gate.engine: claude-code`, repair writes `mods.ways-judge.install: false` in that target's config and leaves `gate.engine` as it is; that target's sessions resolve as ADR-708 item 2 says for no judge host, and `ways manage mod check ways-judge` reports the disagreement, exits 3, and names the fix"
  - "`ways manage mod check` on a host with Claude Code below the ways-judge floor and `gate.engine: claude-code` prints one ⚠ line naming the installed version, the floor and the fix, and exits 3; on a converged host it prints every check passing and exits 0"
  - "`ways manage mod uninstall attend` without `--commit` prints the calls it would make in each enabled target and changes nothing; with `--commit` it writes `mods.attend.install: false` in the user config and uninstalls the plugin"
  - "`ways manage mod uninstall ways-judge` while `gate.engine` is claude-code refuses, names `ways judge-setup` and changes nothing"
  - "After `claude plugin disable attend@agent-ways`, repair leaves it installed and disabled and `ways manage mod check attend` names the enable command"
  - "The settings screens' mod tab lists one node per shipped mod; opening one shows its state in each enabled target, its settings, and the check, install or uninstall, and repair actions; the scenario script captures it at 80x25 and 120x35"
---

# ADR-512: Each shipped Claude Code mod has one wanted state in settings, under mods.<name>, and ways manage converges every target to it

## Summary

- **Decided:** each mod agent-ways ships to Claude Code (today `attend` and `ways-judge`) has one wanted state, `mods.<name>.install`, in the ways config, and its other settings sit beside it under the same name, so `attend_mod.*` becomes `mods.attend.*`. `ways manage repair`, and `update` and `reinstall` through it, converge every enabled target to each mod's wanted state. A wanted mod on a Claude Code below the mod's floor stays pending, is reported with the floor, and is tried again at every later repair. A mod that repair installed and that is later uninstalled by hand is not put back: repair writes `install: false` for it in that target's config and says so, so the operator's removal becomes a visible setting. The per-target record file that kept ways-judge installed is removed. Choosing a judge engine writes `gate.engine` and `mods.ways-judge.install` together, through one step that every writer of `gate.engine` calls. `ways manage mod list | check | install | uninstall | repair` manages the mods, reporting until `--commit`, and the settings screens' `mod` tab gains one node per mod with its state per target, its settings and those actions. `ways settings lint` and `ways manage update` name every file that still holds the old `attend_mod` section; nothing rewrites those files on its own, and `ways settings fix` moves the values when the operator runs it.
- **Trades away:** an engine switch now installs or uninstalls a plugin, and switching back costs a reinstall from the local marketplace. Repair now writes a setting on its own, the `install: false` that records a removal by hand, and keeps a list of the mods it installed in each target so that it can tell a removal by hand from a mod it never installed. A by-hand removal of ways-judge while `gate.engine` is `claude-code` leaves the engine and the mod disagreeing in that target until the operator picks again. Attend loses ADR-406's reinstall on every repair. Every file that holds `attend_mod` keeps it until the operator runs fix, and until then a project that set `attend_mod.listening: active` starts its sessions dormant. A ways-judge install is refused by `ways manage mod` on its own: it follows the engine.
- **One-way?** No. Reversible: the change is settings keys, one convergence rule, a per-target list and a CLI group, and can be undone in one session. The rename becomes more work to undo once operators have run `ways settings fix` on their files, since undoing it needs the same carry-over in the other direction.
- **Probes:** *Confident (heal):* you want to pick Claude Code internal once and have agent-ways install its mod whenever Claude Code can run it, so that after a Claude Code upgrade `ways manage update` is the only step. *Not confident (by-hand):* if you uninstall one of these mods with Claude Code's own `claude plugin uninstall`, repair leaves it uninstalled and writes `install: false` for it in that target's settings, which `ways manage mod install` or the mod tab undoes.
- **Inversion:** at one end Claude Code's plugin list is the truth, and agent-ways installs a mod only when asked and remembers nothing. At the other end agent-ways' settings are the only truth, and repair overrides whatever the operator does in Claude Code. This decision takes installation from the settings, so a wanted mod is installed and kept current without anyone asking again. Removal and enablement sit at the Claude Code end: a removal made there is respected and written back into the settings, and enabling or disabling is left to Claude Code.

## Context

ADR-406 item 8 has `ways manage repair` install the attend mod in every enabled target from the app's local marketplace, governed by `attend_mod.install`. ADR-708 item 9 added the ways-judge mod, installed only when the operator picks Claude Code internal in `ways judge-setup`. Because that install is on request, the code keeps a record file per target beside the settings merge base, and repair keeps ways-judge installed only where the record exists. The judge engine itself is a separate setting, `gate.engine`, in the agent file. The settings gate screen and `ways settings set` write it with no change to the mod, and only `ways judge-setup` writes the record.

There were two sources of truth for one fact: whether this machine should be running the ways-judge mod. They could disagree. On one host, `gate.engine: claude-code` was set while Claude Code was below the mod's floor. No record was written, so no later repair or update installed the mod, and the gate failed open until setup was rerun by hand after Claude Code was upgraded. The repair path also never checks a mod's floor; only setup does.

The settings screens have a `mod` tab that edits attend's settings and nothing else. The operator asked for the lifecycle of the mods (install, uninstall, repair, check) to sit under `ways manage`, for the tab to gain a level that manages each mod, and for the settings to be named `mods.<name>.*`. Asked about a mod removed by hand, the operator chose to have repair respect the removal, as ways-judge does today. Asked about files that still hold the old section, the operator chose to have them reported, with the operator running the fix.

## Decision

The figure shows the writers of each mod's wanted state, and how a repair converges one mod in one enabled target from it.

![Writers of the wanted state: the engine choice writes mods.ways-judge.install and gate.engine in an order that depends on the engine; ways manage mod writes a mod's install setting; repair itself writes install false in a target's config when a mod it installed there was removed by hand. Convergence: repair, update, reinstall or mod repair reads each mod's install setting per enabled target. False uninstalls it. True with the mod missing but on repair's list for that target means a removal by hand: repair writes install false there and reports it. True otherwise checks the host against the mod's floor, leaving the mod pending when the host is below, else installs or updates it and adds it to the list. A mod left disabled in Claude Code stays disabled and mod check names the enable command. A pending mod is tried again at every later repair.](figures/ADR-512-mod-convergence.svg)

1. **One settings section per shipped mod, `mods.<name>`.**
   - `mods.attend` holds what `attend_mod` holds today, under the same key names: `install`, `listening`, `keepwarm`, `poll_seconds`, `slow_poll_seconds`, `quiet_minutes`, `preamble_every_turns`, `headless_poll` and `keepalive.*`. Each key keeps its type, default and scopes, as ADR-406 item 11 and ADR-414 set them.
   - `mods.ways-judge` holds `install`, default false.
   - `install` is the wanted state. Its scope is the user config and a target's config (ADR-184 item 2), resolved per target with the target winning; a project cannot set it, since repair acts per target. `mods.attend.install` keeps its default of true.
   - The mods are the plugins `plugins/.claude-plugin/marketplace.json` lists. Each has a `Mod` entry in the ways binary, carrying its floor, and a `mods.<name>` section in the settings registry. A test fails when the marketplace file and the two lists disagree.
   - Every reader moves in the same change: the ways binary, the attend mod's `ways settings list` call, attend's keepwarm policy and status, attend-chat's settings editor, sensor-keepwarm, the attend skill and way, and the docs. A process that started before the update and still asks for `attend_mod` (a running mod module, a running `attend`) gets no section and falls back to its defaults, dormant with keepalive off, until it restarts or runs `/reload-plugins`. A caller that degrades quietly does not fix the name it calls (ADR-507 §2), so no alias is kept.

2. **The old section is reported where it lives, and moved only by the operator.** `attend_mod` joins the ways config's retired keys, and no code configures anything from it (ADR-506).
   - `ways settings lint` reports it in every file that holds it: the user config, each target's config, and the `.claude/ways.yaml` of every project `ways projects` knows. Each line names the file and the command that fixes it.
   - `ways manage update` names the same files, in its what-if report and in its `--commit` run, as one ⚠ section after the actions. It rewrites none of them.
   - `ways settings fix mods.attend` (with `--project <dir>` for a project file) moves the section when the operator runs it: each `attend_mod.<key>` value is written to `mods.attend.<key>` in the same file, and the old section is removed. Where a file holds both, the `mods.attend` value stays, the old one is removed, and fix says which values it dropped. This adds one repair to fix: today fix leaves a top-level key no section owns for a hand edit, because it cannot know what the key meant. A retired key that names its successor is the one case where it can.
   - Until a file is fixed, the values in it are not read. A project that set `listening: active` starts dormant, and user tuning reads as defaults. The release that ships this lists the move and the command (ADR-506 §4). The retired-key entry stays as long as the table keeps the entries it holds today.

3. **The wanted state replaces the record file; repair keeps a list of what it installed.** The `ways-judge-installed` record and the code that writes and forgets it are removed. Repair keeps its per-target state where it keeps it today, in the `attend-mod-marketplace` file beside the target's settings merge base. That file already records the folder a repair added the marketplace from. It now also lists the id of each mod a repair has installed in that target and not removed since. `execute` updates the list after each call, where it updates the `pending` line today: an install that succeeds adds the id; an uninstall, or a marketplace removal, made by repair removes the ids it takes out; a failed install adds nothing. The list records what repair did; what is wanted lives only in settings. The first repair after this change adds every mod of ours it finds installed from this app's marketplace in that target, and reads no older file.

   For each enabled target, repair (and `update` and `reinstall`, which run it) resolves each mod's `install` and:
   - **Not wanted:** uninstalls the mod where it was installed from this app's marketplace and removes it from the list. It removes the marketplace when no mod is wanted in that target, as ADR-406 item 9 does today.
   - **Wanted, installed:** updates an older copy by ADR-406 item 8's calls.
   - **Wanted, missing, on the list:** removed by hand. Repair does not reinstall it. It writes `mods.<name>.install: false` in that target's config, removes the id from the list, and reports one line naming the write and `ways manage mod install <name>` as the way back. The write goes to the target's config, not the user config, because the removal was seen in that one target; a write to the user config would take the mod out of every other target. It goes through the settings writer like any other write, so it is visible in `ways settings list`, in the mod tab and in `git diff` of a tracked config. Under `--what-if` it is reported and not made.
   - **Wanted, missing, not on the list, host able:** never installed, or an install that failed. `claude` is on PATH, its plugin CLI answers for the target, and `claude --version` is at or above the mod's floor (`min_claude_code`). It adds the marketplace and installs the mod, and adds the id to the list.
   - **Wanted, missing, not on the list, host not able:** pending. Repair changes nothing for it and reports one ⚠ line with the reason, the floor and the fix (upgrade Claude Code, then `ways manage update --commit`). Pending is derived each time from the setting, the list and the host, and is tried again at every later repair. It never fails the repair, as ADR-406 item 8 has it for a failed call.
   - **Disabled in Claude Code** (`enabledPlugins` false, through `/plugin` or `claude plugin disable`): left disabled. Enablement stays Claude Code's control, as ADR-406 item 8 promised the operator. `ways manage mod check` reports it and names the enable command.
   - **A plugin of the same name from another marketplace:** the operator's; reported and left alone, as today.

   A removal by hand of the whole `agent-ways` marketplace takes every mod on the list out with it, and repair reads it as a removal by hand of each. A disabled target is withdrawn as today: every mod is uninstalled before its links go, whatever its setting (ADR-184 item 4), and the list is emptied. The settings stay, so enabling the target again installs what is wanted.

   This applies to attend and ways-judge alike, and changes ADR-406 item 8 for attend: repair no longer reinstalls an attend mod the operator uninstalled. For ways-judge with `gate.engine: claude-code`, a removal by hand leaves the engine as it is. That target's sessions find no judge host and resolve as ADR-708 item 2 says for that case, and `ways manage mod check` reports the disagreement and names both fixes: `ways manage mod install ways-judge`, or `ways judge-setup` to pick a key engine.

4. **The engine choice writes the judge mod's wanted state.** One function writes a judge engine, and every writer of `gate.engine` calls it: `ways judge-setup`, the settings gate screen, `ways settings set gate.engine` and `unset`.
   - **`claude-code`:** writes `mods.ways-judge.install: true` in the user config first, then `gate.engine: claude-code` in the agent file.
   - **`anthropic`, `openrouter`, unset, or no judge** (`gate.mode: off` from setup): writes or clears `gate.engine` first, then `mods.ways-judge.install: false`.
   - The two settings live in two files, so the write cannot be atomic. The order makes a failure between them leave the mod installed under a key engine, where ADR-708 item 2 has it stand down. It never leaves `claude-code` selected without the mod wanted in the user config.
   - `ways judge-setup` and the gate screen run `ways manage mod repair ways-judge --commit` after the write. `ways settings set` only writes, and prints the repair command; the mod is installed at that repair or the next update.
   - A write of `mods.ways-judge.install` in the user config that is not part of an engine choice is refused, with the engine command as the fix, so the engine and the user config's wanted state always agree. A target's config may hold `mods.ways-judge.install: false`, written by the operator or by repair after a removal by hand (item 3), to keep the mod out of that one target; `ways manage mod check` reports it whenever the engine is `claude-code`.
   - `claude-code` stays selectable and is never the default (ADR-708, alternatives). `ways judge-setup` keeps ADR-708 item 9's check and does not offer it on a host below the floor. The gate screen and `ways settings set` accept it there, write both settings, and report the mod pending with the floor, so the upgrade heals it.

5. **`ways manage mod`.** A group under `ways manage` (ADR-509 item 1's table gains it):

   | Command | What it does |
   |---|---|
   | `ways manage mod list` | Each shipped mod: shipped version, wanted state, and per enabled target installed version, pending, removed by hand, disabled or absent. Read-only. |
   | `ways manage mod check [<mod>]` | Read-only diagnosis, per mod and target: Claude Code's version against the floor; the `agent-ways` marketplace present and this app's; the plugin installed, and its version against the shipped one; enabled in Claude Code; installed state against wanted state; for ways-judge, whether `gate.engine` agrees in every enabled target. Each problem is one ⚠ line naming its fix command. Exit 0 when every check passes, 3 with any problem, as `ways settings lint`. |
   | `ways manage mod install <mod>` | Writes `mods.<mod>.install: true` in the user config, removes an `install: false` from each enabled target's config, then converges that mod in every enabled target. `--target <dir>` acts on that target's config only. For ways-judge it is the engine choice of item 4 for `claude-code`, and its report shows the engine change. |
   | `ways manage mod uninstall <mod>` | Writes `install: false` in the user config (or with `--target <dir>` in that target's) and converges. For ways-judge while `gate.engine` is `claude-code` it refuses and names `ways judge-setup`, since removing the judge's host would leave the gate failing open or paying for a key. |
   | `ways manage mod repair [<mod>]` | Converges the mods, or one, in every enabled target, with no projection or settings merge. `ways manage repair` does the same as one of its stages. |

   `install`, `uninstall` and `repair` report what they would change until `--commit`, in ADR-511's report shape, and follow `install.manage_mode` as `repair` and `target` do (ADR-511 items 1 to 4). A bare `ways manage mod` opens the settings screens on the `mod` tab on a terminal, and in a pipe prints its help and exits 2 (ADR-509 item 2). `ways status` and `ways manage update`'s report name every shipped mod, pending ones with their floor and removed ones with the setting repair wrote, where today they name attend's state and ways-judge's version only.

6. **The settings screens' `mod` tab gains a level.** The tab's prefix becomes `mods`. Its top level is one node per shipped mod, labelled with its name, shipped version and a one-word state (`converged`, `pending`, `removed`, `disabled`, `off`, `drift`). Each node holds:
   - a read-only row per enabled target with that target's state and, for pending, the floor;
   - the mod's settings, `install` among them; for ways-judge, the user config's `install` is shown read-only with the gate tab named as where it is set;
   - its actions, queued as the CLI commands of item 5 through `agent_tui::tree::Action`: `check` (reads, verifies), `install` or `uninstall` (confirm; touches each enabled target's plugin list, settings.json and config), and `repair` (confirm; labelled `update` when an installed copy is older than the shipped one). ways-judge's `uninstall` is absent while `gate.engine` is `claude-code`, and its `install` names the engine change in its confirm text.

   ```
   mod
   ├─ attend       0.29.0  removed
   │  ├─ ~/.claude                  installed 0.29.0, enabled
   │  ├─ ~/.claude-work             removed by hand; install false in this target's config
   │  ├─ install                    true (default)
   │  ├─ listening                  dormant (default)
   │  ├─ keepwarm …  keepalive …  poll_seconds …
   │  └─ [check] [install] [uninstall] [repair]
   └─ ways-judge   1.43.0  pending
      ├─ ~/.claude                  pending: Claude Code 2.1.300 is below 2.1.304
      ├─ install                    true (set by gate.engine: claude-code)
      └─ [check] [repair]
   ```

   The scenario script for the `ways` screens gains the states this tab reaches (tui-verification way).

This amends ADR-406's Decision: item 8's install and update gain the floor check, the pending state, and the rule that a mod removed by hand stays removed and is written back as `install: false`; item 9's removal is unchanged; item 11's section is renamed to `mods.attend`. It amends ADR-708's Decision: item 8's engine choice and item 9's setup choice of Claude Code internal now write `mods.ways-judge.install` through item 4's step, and the mod is kept installed by that setting, not by a record. Earlier records that name `attend_mod.*` keys (ADR-406, ADR-411, ADR-414, ADR-510) read with `mods.attend.*` in their place; they are not amended for the name alone.

## Consequences

### Positive

- One setting decides whether a mod should be installed, and it lives in the config that reinstall keeps, beside the engine it serves. Engine and mod agree in the user config by construction.
- A host below a mod's floor heals by itself: upgrade Claude Code, run `ways manage update`, and every wanted mod installs. The operator never has to remember to rerun setup.
- Repair checks every mod's floor before it installs, where today only setup checks ways-judge's.
- A removal made in Claude Code is respected, and it is not hidden in a state file: it shows as a setting the operator can read, change, or undo with one command.
- The operator can see and change each mod's state in one place, from the CLI or the mod tab, and `check` names the fix for each problem.
- A third mod needs a marketplace entry, a `Mod` entry and a `mods.<name>` section, and the lifecycle, CLI and tab follow from those.

### Negative

- Switching the engine to or from Claude Code internal installs or uninstalls a plugin. Switching back costs a reinstall, which is quick from the local marketplace, and running sessions take the change at their next start or `/reload-plugins`.
- Repair now writes the ways config itself, for a removal by hand. A repair the operator thought of as converging projections can change a target's settings, and its report has to say so plainly.
- Repair depends on its list of installed mods. If the state root is lost or reset while a mod stays installed, the next repair re-adds the mod to the list, so nothing breaks. If the list is lost after a removal by hand, that removal reads as never installed and repair installs the mod again.
- Attend is no longer reinstalled on every repair. A target where the operator removed it stays without it, and attend's sessions there fall back to Monitor (ADR-406 item 9), until the operator installs it again.
- A by-hand removal of ways-judge under `gate.engine: claude-code` leaves the engine and that target's setting disagreeing. Sessions there judge on a key or not at all (ADR-708 item 2) until the operator acts on `mod check`'s line.
- Every file that holds `attend_mod` keeps it until the operator runs fix. Until then a project that had `listening: active` starts sessions dormant and the user's attend tuning reads as defaults. Lint and every update name the files, but nothing fixes them on its own.
- During an update, a running mod module or `attend` process asks for the old section and falls back to defaults until it restarts.
- `ways settings fix` learns one rename repair. It is code that knows the old name, kept only to rewrite files.
- The engine write spans two files with an order, and a direct write of `mods.ways-judge.install` in the user config is refused, which makes it the only `install` key that `ways settings set` will not take there.

### Neutral

- The marketplace folder record and ADR-406's foreign-marketplace rules are unchanged; the file that holds the folder gains the list.
- ADR-708's session resolution is unchanged; a target that keeps ways-judge out resolves through its no-host path.
- The hidden `ways reconcile` keeps applying with no flag (ADR-511 item 8), so the previous release's updater converges mods by the new rule once the new binary is installed.

## Alternatives Considered

- **Keep the record file and have every writer of `gate.engine` write it.** The smallest change. Rejected: the record lives in state beside the merge base and the engine in config, with different owners and lifetimes, so there would still be two sources of truth, and repair still would not check the floor.
- **Derive ways-judge's wanted state from `gate.engine`, with no `install` key for it.** The two could not disagree by construction, and there would be no two-file write. This is the strongest alternative. Not adopted: it gives ways-judge a lifecycle unlike attend's, so `mods.<name>.install`, the `mod` verbs and the tab would each need a special case for it. It also leaves no setting to record a removal by hand in one target. Item 4's single write step and its refusal in the user config give the same agreement with one shape for every mod.
- **Reinstall a wanted mod the operator removed by hand.** One rule, settings as the only truth. Rejected by the operator, who chose to have repair respect a by-hand removal, as ways-judge does today. Writing the removal back as `install: false` keeps that behaviour without a hidden record.
- **Record a removal by hand in a state file, as the `pending` line does today.** No settings write by repair. Rejected: the operator's choice would again live outside settings, where `ways settings list`, the mod tab and `check` cannot show it.
- **Tell a removal by hand apart using the settings merge base or Claude Code's `enabledPlugins`.** No new list. Not possible: the merge base holds only hooks and permissions, and Claude Code drops a plugin's `enabledPlugins` entry on uninstall, so neither shows that a plugin was installed before.
- **Write the `install: false` for a removal by hand in the user config.** Rejected: a removal seen in one target would take the mod out of every other target.
- **Keep `attend_mod` and add a `judge_mod` section.** No rename and no fix step. Rejected: the operator named `mods.<name>.*`, and a top-level section per mod grows the config's top level with every mod.
- **Read `attend_mod` as an alias of `mods.attend` for a release.** Running processes would keep their settings through the update. Rejected under ADR-506: the fix command moves the values, and the callers that would need the alias fall back safely.
- **Have `ways manage update` run the fix on every file it finds.** No dormant window. Rejected by the operator, who chose to have the files reported and to run the fix themselves.
- **Refuse to record a wanted mod on a host below its floor.** Rejected: that is the failure that prompted this record. A wanted state that waits for the host lets an upgrade heal it.
- **Make `ways settings set gate.engine` run the repair itself.** One command would finish the job. Rejected: `ways settings set` writes settings and calls no external CLI anywhere else. Setup and the gate screen run the repair, and `set` names it.
