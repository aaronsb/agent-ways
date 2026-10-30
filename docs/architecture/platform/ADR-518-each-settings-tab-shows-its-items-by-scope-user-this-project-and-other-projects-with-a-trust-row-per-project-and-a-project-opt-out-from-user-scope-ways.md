---
contract: adr/v1
kind: decision
verb: change
capability: [config, matching, attend]
amends: [ADR-516#6-the-card, ADR-403#5-two-surfaces-one-model, ADR-131#scope, ADR-116#replaces-trusted-project-macros]
basis:
  - operator: aaronsb
    level: directed
    said: "I think other project sections should not be editiable, but viewable. the trusted model you describe is useful because it's a continuation of ways steering in project scope. we have both ways guidance (text) and macros (executable scripts) that can exist in projects. by default we don't trust any project scope ways macro."
    via: session 2026-10-30
  - operator: aaronsb
    level: directed
    said: "So maybe it's each section in the menu gets a couple things: user scoped items, (always can modify) then 'this project' items (can modify based on where 'ways settings' was invoked) and 'all projects' settings for that item (read only). we don't duplicate 'this project' scoped items in the all projects section. this way major scope  across the top, (ways, sensors) gets to see by user and project scope."
    via: session 2026-10-30
  - operator: aaronsb
    level: directed
    said: "b) is what I'm thinking this allows a per-project exclusion of user scopeed ways (an override of sorts). a user could elect to deactivate injection of all shipped user scoped ways and write project scoped ways and macros (and turn on that projects trust for macros) effectively isolating that project from typical ways guidance."
    via: "session 2026-10-30, choosing among agent-written options; b) placed builtin items in the user section with an origin tag"
  - evidence: "the ways tab's other projects section reads and writes each other project's .claude/ways.yaml (tools/ways-cli/src/cmd/settings/tui/others.rs), and finds those projects from Claude Code's projects directory"
  - evidence: "project macros and postchecks run only when the project is listed in ~/.claude/trusted-project-macros (ways_core::paths::trusted_project_macros, read by show::is_project_trusted and hook/post_tool.rs::roots); ways author permissions reports that file as deprecated"
  - evidence: "attend.project_sensors is a user-scope list in $XDG_CONFIG_HOME/attend/config.yaml that the sensors tab reads for its cards (tools/ways-cli/src/cmd/settings/tui/sensors.rs::trusted), and its row sits on the attend tab"
  - evidence: "per-way switches are the project-scope key ways.project.* over the ways: map of .claude/ways.yaml, with /* prefix keys covering a subtree before ADR-710 (tools/ways-core/src/settings.rs); ways.enabled: false in a project's file turns every way off there"
  - evidence: "#825: ADR-116 deprecates trusted-project-macros in favour of requires:, and the runtime still enforces the list"
  - evidence: "#1218: ADR-516 slice 3 plans project, user and builtin sections on the ways tab"
  - precedent: ADR-516
  - precedent: ADR-403
  - precedent: ADR-199
  - precedent: ADR-143
  - precedent: ADR-131
  - precedent: ADR-116
  - precedent: ADR-504
  - precedent: ADR-506
  - precedent: ADR-710
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "isolation seems to make sense, as long as we can then configure things within that project scope"
    via: session 2026-10-30, answering the probes in chat after the drafts were summarized; the ADR-518 figure was shown first
    covers: [isolation reach]
  - operator: aaronsb
    said: "this implies that the registry of allowed ways is a two way situation. ways could be disabled at the user scope, but selectively enabled at the project scope, which I like."
    via: session 2026-10-30, answering the probes in chat after the drafts were summarized
    covers: [user switch]
observable:
  - 'see: on the ways tab opened in a project, the sections read user, this project and other projects in that order; a shipped way reads like adr  builtin  on · off here in the user section, and the project is not listed under other projects'
  - 'see: an other project''s node shows its switches, its inherit value and its trust row, and no row in it takes an edit'
  - 'see: the sensors tab''s this project section holds the project''s sensor files and a trust row naming $XDG_CONFIG_HOME/attend/config.yaml, and the attend tab no longer lists attend.project_sensors'
  - 'run: with inherit: none and softwaredev/delivery/commits: true in .claude/ways.yaml, a prompt that matches a shipped way other than that one fires nothing from the shipped corpus, and the discard log records the way with filter isolation'
  - 'run: in a project with a macro and no entry in ways.project_macros, the way''s text fires, the macro does not run, and the discard log records it with filter untrusted'
status: accepted
date: 2026-10-30
deciders:
  - aaronsb
related: [ADR-710, ADR-503, ADR-504, ADR-510, ADR-512, ADR-516, ADR-403, ADR-199, ADR-143, ADR-131, ADR-116, ADR-184, ADR-506]
---

# ADR-518: Each settings tab shows its items by scope: user, this project and other projects, with a trust row per project and a project opt-out from user-scope ways

## Summary

- **Decided:** The settings screens have two axes. The tabs across the top say what: ways, matching, gate, install, attend, sensors, mod, theme. Inside a tab, sections say where, in this order: `user`, editable; `this project`, the project `ways settings` was opened in, editable; `other projects`, one collapsible node per other project with content for the tab, view only. Builtin items sit in the user section, each row tagged with its origin and its state in this project. On the ways and sensors tabs, each project section carries a trust row for the project's code. Trust is stored in the user's own file, and no project is trusted by default. A project can stop inheriting builtin and user ways with `inherit: none` in its `.claude/ways.yaml`, with switches as exceptions. Per-way switches may also sit in the user's file, and for one way a matching project switch wins over any user switch. Project macro trust is a user-scope list per project, and a way's `requires:` grants no trust, which settles #825. The record amends ADR-516 §6, ADR-403 §5, ADR-131 and ADR-116, and changes the ways-tab sections #1218 planned.
- **Trades away:** The screens no longer write another project's files: today the ways tab edits other projects' switches. The `~/.claude/trusted-project-macros` file goes, with no legacy reader, so a project listed only there runs no macros until it is added to the new key. `requires: ["*"]` stops standing for a trusted project. ADR-131's limit of per-way switches to project scope is lifted, which adds a place a way can be turned off.
- **One-way?** No. Reversible: the change covers the screens' layout, two new keys, and one trust list moved between files. The move fails closed.
- **Probes:** *Not confident (isolation reach):* in an isolated project no shipped or personal way reaches the session by any route. None fires on a prompt or a tool call, none reaches a subagent, and the agent's own search and read find only the project's ways. The base posture, `core.md`, still loads at session start. *Confident (user switch):* from the user section you can switch a shipped way off for every project, and a project can switch it back on for itself.
- **Inversion:** One end lists items by where their files live, each tab a flat list of origins. The other shows only what is in effect here and hides the layers. This record lists each item where it is set and shows on its row what holds in this project.

## Context

The settings screens (ADR-503, ADR-504) group settings into tabs by what they configure. Where a setting lives is shown three ways. The ways tab has sections by origin, which ADR-516 §6 names `project`, `user` and `builtin` and #1218 implements, and an `other projects` section whose switches write each other project's `.claude/ways.yaml`. The sensors tab lists every sensor with a card that names its origin. The other tabs show one row per key with the layer it resolves from. `attend.project_sensors`, the list that lets a project's command and mcp sensors load (ADR-403 §2), is a loose row on the attend tab, away from the sensors it gates.

Projects hold two kinds of ways content: guidance text, and code the ways hook runs, which is a way's `macro.sh` and `postcheck.sh`. A project's code runs only when the project is listed in `~/.claude/trusted-project-macros`. ADR-116 deprecated that file in favour of each way's `requires:` field, with `requires: ["*"]` standing for a trusted project, but the runtime still reads only the file (#825). A way's `requires:` is written by the repository whose code the gate checks.

A project can turn one way off (ADR-131) or every way off (`ways.enabled: false`, ADR-184). It cannot keep its own ways and drop the builtin and user ones, which a project with its own complete guidance and macros needs.

Project sensor files under `.claude/attend/sensors/` load and layer today. Command and mcp sensors do not run yet (ADR-403 slices 2 and 3) and read `degraded` (ADR-516, addendum item 2).

## Decision

### 1. Tabs say what, sections say where

The tabs stay as they are. Each tab shows up to three sections, in this order:

| Section | Holds | Edits |
|---|---|---|
| `user` | builtin items, the user's own items, and keys set in the user's files | yes, writing the user's file |
| `this project` | the items and keys of the project `ways settings` was opened in, which ADR-510 resolves | yes, writing the project's file |
| `other projects` | one node per other project, collapsed, with that project's items and keys for the tab | no |

A tab shows the two project sections when its family has a project origin (ADR-516 §3) or its registry holds a key of project scope. Install and theme have neither and show the user section only. No separate `project` tab is added.

The other projects are those `ways projects` lists (ADR-504), filtered to projects with content for the tab: `.claude/ways/` for ways, `.claude/attend/sensors/` for sensors, and an overlay file that sets a key under the tab's prefix for the key tabs. The project `ways settings` was opened in is not listed there. A node is read when it is first opened. A change to another project's settings is made with `ways settings` opened in that project, or with `--project`.

The figure shows the ways tab opened in a project that has its own ways, a switch on a builtin way, and trust for its macros.

![The ways tab of ways settings. The user section lists builtin and user ways, each row with its origin word and its state: adr builtin on · off here; commits builtin on; my-review user on. The this project section lists the project's own ways with origin project, a switch that turns adr off here, a row inherit all, and a trust row reading macros trusted, stored in the user's config.yaml. The other projects section shows two collapsed nodes, one per project with ways, each marked view only.](figures/ADR-518-scope-sections.svg)

### 2. Builtin items sit in the user section with an origin tag

Each item row in the user section shows its name, its origin word (`builtin` or `user`, ADR-516 §1), its user-level state, and its state in this project where that differs: `adr  builtin  on · off here`. Editing a builtin item writes the user's file: a sensor gets an `extends` override, a theme is copied, and a way gets a user switch.

ADR-131 kept the per-way switch map in the project overlay only. The same `ways:` map may now sit in `$XDG_CONFIG_HOME/agent-ways/config.yaml`, so a way can be switched off for every project from the user section. Its keys are ADR-710's toggle patterns in both files.

A builtin or user way's state in a project resolves in this order:

1. A key in the project's `ways:` map that matches the way decides, whatever any user key says and however specific the user key is.
2. Otherwise, under `inherit: none` (§4), the way is off.
3. Otherwise, a key in the user's `ways:` map that matches the way decides.
4. Otherwise, the way is on.

Specificity (ADR-710 §1) chooses among the keys of one file only. A project can switch on a way the user switched off, or switch off one the user left on.

A project's switch on a builtin or user way is a project key. It is listed under `this project`, and the way's row in the user section can set or clear it, writing the project's file.

This amends ADR-516 §6 for the ways tab and changes the sections #1218 planned. `builtin` is an origin tag and no longer a section. `prefixes` and `not found` stay, under the section whose file holds them. The card and the descriptor of ADR-516 are unchanged.

### 3. Trust is a row in each project section

Each project section of a tab whose items run code carries a trust row:

| Tab | The row lets run | Stored in |
|---|---|---|
| ways | the project's `macro.sh` and `postcheck.sh` | `ways.project_macros`, a list of project paths in `$XDG_CONFIG_HOME/agent-ways/config.yaml` |
| sensors | the project's command and mcp sensor files | `attend.project_sensors` in `$XDG_CONFIG_HOME/attend/config.yaml` (ADR-403 §2) |

When ways sensors land, `ways.project_sensors` (ADR-199 §7) is a second trust row on the sensors tab.

- The keys are user scope only, so a project's file cannot set them.
- No project is trusted by default.
- The row is editable in `this project` and view only under `other projects`. An edit writes the user's file, and the row names that file.
- Each host keeps its own list (ADR-403 §2), so trusting a project's macros does not trust its sensors.
- An untrusted project's ways still fire as text. Their macros and postchecks do not run.
- `attend.project_sensors` leaves the attend tab.

This settles #825 and amends ADR-116's "Replaces `trusted-project-macros`". The per-project list is the trust gate. `requires:` declares the tool permissions a way needs, for the audit against `settings.json` (ADR-116), and grants no trust. `requires: ["*"]` no longer stands for a trusted project. The list moves from `~/.claude/trusted-project-macros` to `ways.project_macros` with no legacy reader (ADR-506). While the old file exists, `ways author permissions` and `ways settings lint` report it and name each project in it with the key to add it to.

### 4. A project can stop inheriting user-scope ways

`.claude/ways.yaml` gains `inherit`, the settings key `ways.inherit`, of project scope only:

- `all`, the default: builtin and user ways apply in the project, and a switch set to `false` turns one off (ADR-131).
- `none`: no builtin or user way applies in the project. A switch set to `true` brings one back, and a pattern such as `softwaredev/delivery/**: true` brings back every way under `softwaredev/delivery`. The project's own ways apply as usual.

Under `none`, the project's ways are the only guidance, and their macros, once the project is trusted, are the only macros that run. The set applies to every route that puts way text into the session: prompt and tool matching, subagent injection, and `ways_search` and `ways_read`. `core.md` is the base posture and still loads. `ways.enabled: false` remains the switch that turns every way off.

A way that would have fired and that isolation removes is logged as `way_discarded` with the filter `isolation` and the scope `project`. A project macro that trust refuses is logged with the filter `untrusted`, and its way's text still fires. ADR-710 §4 defines the event and leaves these two filters to this record.

On the ways tab, `inherit` is a row in `this project`, and each other project's node shows its value. Under `none`, user-section rows read `off here` for each way it removes and that no switch brings back.

### 5. The sensors tab

- `user`: the builtin sensor files and the user's own, each row with its origin tag and its state in this project.
- `this project`: the project's files under `.claude/attend/sensors/`, and the trust row.
- `other projects`: each project's sensor files, view only. Their state is read from the files: `refused`, `invalid`, `off` or `degraded`. A live state is shown only where attend polls for that project.

This amends ADR-403 §5, where the tab lists every sensor of every kind in one list. The tab still shows both prefixes, `attend.sensors` and `ways.sensors`.

## Consequences

### Positive

- Every tab shows where a setting lives in the same sections, in the same order.
- A project's trust sits beside the code it lets run, in a section that lists that code.
- A project can carry its own complete guidance without the builtin and user ways, and take back single ways or subtrees.
- The records and the runtime agree on one trust gate for project macros, and #825 can close.
- A screen opened in one project writes no other project's files.

### Negative

- Changing another project's settings takes a session in that project, or the CLI with `--project`.
- Operators with projects in `~/.claude/trusted-project-macros` lose those projects' macros on upgrade until they add them to `ways.project_macros`.
- A way can now be turned off in four places: `disabled_domains`, a user switch, a project switch, and isolation. `ways scan --explain` and the discard log must name which one applied.
- An isolated project gets no new way that a later release ships, unless it switches the way back on.
- Finding the other projects for each tab probes the filesystem. Reading a node when it is opened bounds the cost.

### Neutral

- #1218's ways-tab sections become `user`, `this project` and `other projects`. Its other items stand.
- The origin tag is the descriptor's `origin` field (ADR-516 §1). No field is added.
- `inherit` and `ways.project_macros` are new keys in the ways registry.

## Alternatives Considered

- **Sections by origin, `project`, `user` and `builtin`, as ADR-516 §6 and #1218 had them.** A builtin item and the user's switch on it sat in different sections, and a section did not say which file a change writes.
- **A separate `project` tab for every project-scope setting.** One family's items would be split across two tabs. The operator dropped it once every tab had a project section.
- **Other projects editable, as the ways tab is today.** A write from here lands in a repository the session is not working in. The operator chose view only.
- **Trust through `requires:`, as ADR-116 planned.** The field is written by the repository whose code the gate checks.
- **One trust list for all of a project's code.** attend never reads a ways file and the ways hook path never reads attend's (ADR-503 §5), so each host keeps its own list.
- **Copy the old trust file into the new key on upgrade.** That is a legacy reader kept for one release, which ADR-506 rules out.
- **Isolation by switching each user-scope way off in the project's file.** Every way a later release ships would fire in the project until someone switched it off as well.
- **Isolation that leaves `ways_search` and `ways_read` on the full corpus.** The agent's own lookups would bring builtin guidance into a project that opted out of it.
