---
contract: adr/v1
kind: decision
verb: add
capability: matching
basis:
  - operator: aaronsb
    level: directed
    said: "it seems like we could create a tool abstraction mapping dictionary (or catalog or something) that allows us to remain less coupled to claude code as it changes"
    via: "the operator in the head session, 2026-10-22, relayed verbatim by the coordinator while this record was drafted"
  - operator: aaronsb
    level: directed
    said: "let's do a full survey. it might just be as straightforward as enumerating the types of tools to afford trigger types on"
    via: "the operator in the head session, 2026-10-22"
  - evidence: "ADR-705, the survey of Claude Code 2.1.308's tool surface: 73 built-in tools in the bundled declarations, 46 deferred, MCP tools named per server, a legacyToolNames table (Task to Agent, KillShell and KillBash to TaskStop), primary fields that differ in spelling (LSP's filePath), and tools present in the binary but absent from the build's tool set (Glob, Grep)"
  - upstream: "Claude Code permissions reference (code.claude.com/docs/en/permissions): a rule's specifier targets one primary field per tool, and rules group tools into families (Edit rules cover every file-editing tool; Read rules cover Grep and Glob, best effort)"
  - evidence: "the Monitor tool takes a `command` field (ADR-705), so classifying a tool by the shape of its input would file it with Bash"
  - operator: aaronsb
    level: directed
    said: "I think using tldr, it's corpus data (kept in the ways install dir) adding the cc license attributation for it, and using it as a module is a fair way to do this. I don't want to try some sort of call to a package that needs to be installed on a machine"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "it feels like theres a risk of building an unmaintainable matcher monolith here. let's be careful to keep concerns seperated"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - evidence: "ADR-707, tldr coverage over the 24,079 operative segments of 8,016 Bash calls: 13.2% resolve to a subcommand page's matching example, 7.2% to a subcommand page's summary, 35.6% to a program page's matching example, 38.5% to a program summary only, 5.5% unresolved; this project's binaries (adr, dotfiles, ways, doc, attend), all 607 `make <target>` segments and 2,537 interpreter and script segments get nothing specific; an extract of summaries, example descriptions and templates from common, linux and osx holds 7,092 pages and 30,909 examples, 3.68 MB raw and 0.98 MB gzipped"
  - evidence: "tldr-pages `pages.en/LICENSE.md`, read 2026-10-22 from the local tealdeer cache: Copyright © 2014—present the tldr-pages team and contributors; the work is licensed under the Creative Commons Attribution 4.0 International License (CC-BY); the contents of the repository's scripts/ directory are MIT-licensed"
  - standard: "Creative Commons Attribution 4.0 International, legal code §3(a): a licensee who shares the material retains the creator's identification, the copyright notice, a notice referring to the license and a URI or hyperlink to the material, indicates whether the material was modified, and indicates the license with its text or a link to it"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Report, don't block"
    via: multiple-choice question in the agent-ways session, 2026-10-22; options were report don't block, or block the release
    covers: [release]
  - operator: aaronsb
    said: "1131 reads as we discussed I am ok with it"
    via: "chat in the agent-ways session, 2026-10-22, after reading PR #1131 (the PR body listed the three agent-made calls: Bash lane judges every candidate in batches of 8; tool_event default [PreToolUse, UserPromptExpansion]; no-judge setup writes gate.mode off)"
status: proposed
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-188
  - ADR-415
  - ADR-705
  - ADR-707
observable:
  - "`ways catalog check` run against the declarations of the installed Claude Code reports each unmapped tool, each catalogued tool the build no longer has, and each catalogued primary field missing from a tool's declared input; it exits 0 and prints them as warnings"
  - "A way with `agents: Explore` fires when the model calls Agent with subagent_type Explore, and fires the same way on a payload that names the tool by its legacy name Task"
  - "Moving one tool to another type in the user's catalog override changes which ways fire on it in the next tool call, with no rebuild of `ways` and no edit to any way"
  - "`ways status` prints the catalog's source build beside the installed Claude Code version, and lists tool names seen in payloads that the catalog does not map"
  - "A payload for a tool the catalog does not list fires `tool_name:` ways only, and its session event record (ADR-415) carries `tool_type: unmapped` with the raw tool name"
  - "On a host with no `tldr`, `man` or `whatis` installed, the command describer turns `git log --oneline | grep fix | head` into three ordered segments, each with a description from the shipped tldr data"
  - "`make test`, `docs/scripts/adr lint` and `ways corpus` resolve to authored entries, not to make's summary, a shell's, or nothing"
  - "The shipped tldr data sits beside a NOTICE naming tldr-pages, its copyright line, CC BY 4.0 with its link, the source release, and that the data is a reformatted extract"
---

# ADR-706: A shipped tool catalog maps the harness's tool names to abstract tool types, so ways and sensors depend on types and not on Claude Code's names

## Summary

- **Decided:** a data file shipped with the ways content, the tool catalog, is the one place that names Claude Code's tools. It maps each tool name, legacy aliases included, to an abstract tool type (shell, file-modify, file-read, web, delegation, skill, mcp, and the field-less types), and maps the tool's own input field names to the type's abstract field (`command`, `path`, `url`, `agent`, `skill`, `tool`). It also maps the one non-tool event that stands for a tool call, a typed `/name` skill's UserPromptExpansion, into the skill type. It records which build a tool was last seen in and whether its type is wired by default. For the shell type it also holds command entries, programs with their subcommands, flags, classes and descriptions, drawn from tldr-pages data shipped in the install directory with its CC BY 4.0 attribution and from authored entries for this project's binaries, Make targets and scripts; the command describer of ADR-188 reads them, with no tldr client or other package installed. Way frontmatter, the tool-call lane (ADR-188), the session event record (ADR-415) and attend's sensor speak only types and abstract fields; the record also keeps the raw tool name. A generator drafts a catalog from the declarations Claude Code bundles, a maintainer assigns types, and a drift check compares the shipped catalog with the installed Claude Code in lint, `ways status` and release prep. Drift warns and never blocks: a tool the catalog does not map reaches ways only through `tool_name:`.
- **Trades away:** a second artifact to keep current with Claude Code, and an indirection a way author has to learn: a way says `files:`, not "Edit or Write". A tool the catalog has not mapped gets no type key until a catalog update ships. Type assignment for a new tool is a human call, so the generator does not close the loop alone. The command data adds about 1 MB compressed to the install, an attribution to keep with it, and authored entries to keep current as binaries and Make targets change.
- **One-way?** No. The catalog is data and the lookup is one function. The type names and abstract field names become a contract at the first release that ships them in way frontmatter and in event records read by consumers outside this repo; renaming one after that is a migration.
- **Probes:** *Confident (rename):* when Claude Code adds, renames or drops a tool, the fix is one edit to a catalog file that ships with the ways content, with no edits to your ways and no new binary; until the edit lands, a new tool can be reached only by naming it, and `ways status` says it is unmapped. *Not confident (release):* a catalog that has drifted from the installed Claude Code is reported in the release PR and does not stop the release.
- **Inversion:** at one end, ways name Claude Code's tools directly, with no indirection, and every rename upstream means editing ways. At the other end, a fully harness-neutral vocabulary with no way to name a tool, where a new tool is unreachable until someone catalogues it. This decision takes the vocabulary and keeps one named exit: `tool_name:`.

## Context

The ways lanes name Claude Code's tools in three places today. The `settings.json` matchers list them (`Edit|Write`, `Bash`, `Task`). The engine assumes them: the command lane reads `tool_input.command` because the tool is Bash, and the file lane reads `tool_input.file_path` because the tool is Edit or Write. ADR-704, on main, adds a `tool_name:` key whose regex matches the tool name itself, and its wired lists in the way schema name EnterPlanMode, ExitPlanMode and Workflow.

Claude Code's tool set moves under that coupling (ADR-705). Tools are renamed and the old names live on as aliases (`Task` became `Agent`). Tools appear in the binary before they reach a build's tool set, and some leave it (`Glob` and `Grep` are named in 2.1.308's binary and absent from its tool set). The same idea has different field spellings (`file_path`, `notebook_path`, `path`, and LSP's `filePath`). MCP tools are named per server, with no fixed argument field. Each of these changes today means an engine edit, and NotebookEdit has never reached the `files:` lane because nobody added it.

Claude Code already groups its tools. Permission rules target one primary field per tool and treat Edit and Read as families. The operator asked for a full survey "enumerating the types of tools to afford trigger types on", and then for "a tool abstraction mapping dictionary (or catalog or something) that allows us to remain less coupled to claude code as it changes". The survey (ADR-705) found the grouping; this record decides where it lives and who may depend on harness names.

The shell type needs more than a tool name. One Bash call runs several programs, and ADR-188 describes each segment of a call for the matcher and the judge. ADR-707 measured tldr-pages as that description: about half of operative segments reach an example that describes the act, most of the rest reach a program summary, and this project's own binaries, Make targets and scripts reach nothing specific. The operator asked for the tldr data to ship as corpus data in the install directory, with its license attribution, used as a module, and ruled out calling a package that has to be installed on the machine. They also asked that concerns stay separate rather than grow into a matcher monolith; ADR-188 item 4 lays out the components, and this record holds the data they read.

## Decision

1. **The catalog is shipped data.** One YAML file per harness, `catalog/tools/claude-code.yaml` in the app directory, read by the `ways` binary at run time and not compiled into it. A user file at the same path under `$XDG_CONFIG_HOME/agent-ways/` merges over it by tool name, so a user can map a new tool before a release does. A missing or unreadable catalog leaves the lane on `tool_name:` only, and `ways status` names the file.

2. **What it holds.**
   - Its harness and the Claude Code build the generator last read (`built_from`).
   - Each tool by canonical name: its type; its legacy aliases; the input field that carries its abstract field, where the type has one; the last build it was seen in; and `removed_in` once a build drops it.
   - Name patterns for open-ended families: `mcp__*` is type `mcp`, and its abstract field `tool` is the name after the prefix (`server__tool`).
   - Events that stand for a tool call without one. A typed `/name` skill or custom command makes no Skill tool call; Claude Code fires UserPromptExpansion instead (ADR-705). The catalog maps that event with `expansion_type: slash_command` to type `skill`, its `command_name` to the abstract field `skill`. An `mcp_prompt` expansion stays unmapped until a decision gives it a type.
   - Per type, whether it is wired by default (ADR-188 item 6). The shipped matcher strings in `settings.json` are generated from that set, so the template names tools only through the catalog. A test fails when the template and the catalog disagree.

3. **The type vocabulary belongs to ways, not to the catalog.** The types, their abstract fields and their frontmatter keys are part of the way schema (`hooks/ways/frontmatter-schema.yaml`): shell (`command`, key `commands:`), file-modify (`path`, `files:`), file-read (`path`, `reads:`), web (`url`, `urls:`), delegation (`agent`, `agents:`), skill (`skill`, `skills:`), mcp (`tool`, `mcp:`), and the field-less types plan-mode, task-state, scheduling, messaging, user-interaction, extension, publish, memory and output. The Skill tool belongs to the skill type; the extension type keeps ToolSearch and the plugin and connector search tools. A catalog maps one harness onto that vocabulary. A type is added by a decision that changes the schema, not by a catalog edit.

4. **Who speaks harness names.** Only the catalog, the generated matcher strings, and `tool_name:` (ADR-188 item 2), which is the declared exception: a regex over the canonical tool name for tools no type key reaches. Way frontmatter otherwise names types through their keys. The lane's hit loop resolves `tool_name` through the catalog to a type and an abstract field before any way is consulted. The session event record (ADR-415) carries `tool_type` and the abstract field's value, and keeps the raw `tool_name` beside them for consumers that need it. attend's sensor and #843's `where:` conditions select on `tool_type` and the abstract field.

5. **The generator drafts; a maintainer decides.** `ways catalog generate` reads the tool-input declarations Claude Code writes beside a loaded mod (`claude-code-tools/index.d.ts`, `BuiltinToolInputs`) and the hook payload declarations (`claude-code/index.d.ts`, the `HookInput` union), and writes a candidate catalog. It carries over every existing assignment, adds new tools as `type: unassigned` with their declared fields, and marks tools the declarations no longer list. The maintainer assigns types, using the permission-rule families as the reference, and the reviewed file ships. The generator does not guess types from field names: the Monitor tool takes a `command` field and is not a shell tool.

6. **The drift check reports and never blocks.** `ways catalog check` compares the catalog with the installed Claude Code and reports three kinds of drift: a tool the build has that the catalog does not map; a catalogued tool the build no longer has; a catalogued input field missing from the tool's declared input. It reads the declarations when they are on disk, and the run-time record otherwise: the lane logs `tool_unmapped` when a payload's tool resolves to no entry, and `tool_field_missing` when the catalogued field is absent from `tool_input`. It runs in three places:
   - `ways author lint` checks that the catalog parses, that every type key a way uses names a type in the schema, and that each `tool_name:` regex matches a catalogued tool or alias. A miss is a warning.
   - `ways status` prints the catalog's `built_from` beside the installed Claude Code version, the drift the declarations show when present, and the unmapped tools and missing fields logged in the last seven days.
   - Release prep (Release Mechanics in the project-health way) runs the check against the Claude Code installed on the release host, and the release PR states the result.

   On drift: an unmapped tool fires `tool_name:` ways only and its event record carries `tool_type: unmapped`. A tool whose catalogued field went missing fires no type key, since there is nothing to match, and the warning names it. No hook fails and no release is held.

7. **Command entries and the command describer's data.** The shell type's entries describe programs, not tools: each program with its subcommands and flags, a description for each, a class (publish, version-control write, service, package, build and test, file write, version-control read, search, read and inspect), and whether it acts as navigation or as a filter in a pipe. In ADR-188's component layout (item 4) this is data that depends on nothing, and the command describer is the only component that reads it. The sources, each overriding the one before by program and subcommand:
   - **tldr-pages.** An extract of `pages.en` from the common, linux and osx sections: each page's summary, and each example's description and command template, about 7,100 pages and 31,000 examples, 3.7 MB raw and about 1 MB gzipped. It ships as corpus data in the app directory (`catalog/commands/tldr/`) and is read by the describer as data. It is refreshed by a build step from a pinned tldr-pages release, never at run time. The describer runs no tldr client, `man` or `whatis`, and needs no package installed on the machine. A `NOTICE` beside the data carries the attribution CC BY 4.0 asks for: tldr-pages as the source, with its repository URL and the release taken; the copyright line from `pages.en/LICENSE.md` ("Copyright © 2014—present the tldr-pages team and contributors"); the license name with its URL; and a statement that the data is an extract reformatted for lookup. Nothing from the MIT-licensed `scripts/` directory is taken.
   - **Authored entries for this project's own commands.** The suite binaries' subcommands and flags, generated from their clap help at build time; the `docs/scripts/adr` and `docs/scripts/doc` verbs; Make targets, generated from the project's `make help`; and recurring project scripts, with a one-line description each. These cover the misses ADR-707 measured: this project's binaries, every `make <target>` and the project scripts.
   - **User and project overrides,** as for tool entries: a user file under `$XDG_CONFIG_HOME/agent-ways/catalog/commands/` and a project file under `.claude/`, merged by program and subcommand. `ways catalog commands --from-make` writes a project's Make targets into its project file.

   Coverage on the measured window: the tldr extract resolves 94.5% of operative segments, about half of them to an example at the level of the act. `ways status` and the reach report list the programs and Make targets seen in the session event records (ADR-415) that resolve to nothing or to a summary only, so the authored entries follow what agents actually run.

8. **Another harness is another catalog.** A later Claude Code that renames tools, or a different harness whose payloads carry a tool name and an input object, needs a new or edited catalog file. Ways, sensor rules and `where:` conditions written against types do not change. The hook adapters stay harness-specific; they are the other per-harness part.

## Consequences

### Positive

- A Claude Code tool rename, alias or new tool is a data edit that ships with a content update, without a binary release on four platforms.
- NotebookEdit joins `files:` and PowerShell joins `commands:` by catalog entries, and every later member of a family does the same.
- Ways, the spool and attend share one classification, so a sensor rule and a way trigger written against `file-modify` mean the same tools.
- Drift becomes visible on the release host and in `ways status` instead of surfacing as a way that silently stopped firing.
- A Bash call is described from data that ships with the install. The description does not depend on what is installed on the host, and the describer stays a module with no network or process calls.

### Negative

- The catalog is another artifact to maintain, and a new tool waits on a maintainer's type assignment.
- The generator depends on declarations Claude Code writes only where a mod has loaded, which needs the mods preview flag. Without them, drift is seen only once a payload arrives at run time.
- Authors learn a type vocabulary on top of Claude Code's tool names, and `tool_name:` remains a coupled escape hatch that this record cannot remove.
- The binary reads and parses one more file per hook invocation. The tool entries number about a hundred; the cost is not measured. The command data is larger, about 31,000 examples, and needs an index the describer can load without parsing the whole extract on every Bash call.
- The tldr extract adds about 1 MB compressed to the install, and its attribution must travel with every copy of it.
- tldr examples describe common invocations: 38.5% of segments reach only a program page's summary, 7.2% only a subcommand page's summary, and some lookups misread an invocation. Authored entries need upkeep as binaries, scripts and Make targets change.

### Neutral

- The settings matcher still has to name tools, since Claude Code matches on names; generating it from the catalog keeps those names in one source.
- The type vocabulary follows Claude Code's permission families today. A harness with a tool that fits no type needs a schema decision.
- The task lane's semantic match on a delegation prompt (ADR-188 item 5) reads the prompt, not a primary field, and does not use the catalog's field mapping.

## Alternatives Considered

- **A type table compiled into the engine, regenerated per Claude Code build.** The survey's first shape. Rejected: every Claude Code tool change would need a `ways` release, which builds four platforms. Data that changes with the harness ships with the content, and the user layer can patch it before a release.
- **Ways name harness tools directly.** The present state, extended by ADR-704's `tool_name:` regex. Rejected as the general form: every rename upstream becomes an edit to each way that named the tool, in this repo and in users' own ways. Kept as the `tool_name:` escape hatch.
- **Claude Code's permission-rule syntax as the abstraction** (`Bash(git *)`, `Edit(**/*.rs)`). It is Claude Code's own grouping and matches the hook `if` field. Rejected: each rule still names a harness tool, its specifiers are globs where existing ways use regexes, and the operator chose one key per type (ADR-188).
- **No shipped file; build the catalog from the installed declarations at run time.** Rejected: the declarations exist only where a mod has loaded, and assigning a type to a new tool needs judgment that no reading of the declarations supplies.
- **Infer the type from the payload's shape** (an input with `command` is shell). Rejected: Monitor takes `command`, LSP spells the path `filePath`, and MCP tools have arbitrary arguments. Shape is evidence for the generator to show a maintainer, not a classifier.
- **Describe commands with an installed tldr client, `man` or `whatis` at run time.** Rejected: what is installed varies by host, so the same command would be described differently on two machines, and the operator ruled out depending on a package installed on the machine. A process call per segment would also cost more than a lookup.
- **Fetch tldr pages at run time.** Rejected: a network call on the hook's path, and a description that changes under a session.
- **Compile the tldr data into the binary.** Rejected for the reason the tool entries are data: a refresh would need a release on four platforms, and users could not override an entry.
- **Fail closed on drift: refuse to fire, or block the release, until the catalog matches.** Rejected: a stale catalog loses coverage on new tools and keeps every mapped tool working, while a refusal would switch off guidance that still works. The release PR carries the report.
