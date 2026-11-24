---
contract: adr/v1
kind: decision
verb: change
capability: [attend, install]
amends:
  - ADR-406#Decision
  - ADR-403#3-triggers-fire-on-transitions
  - ADR-403#3a-lanes
  - ADR-403#3b-sensor-state-declared-typed-checked
  - ADR-136#Decision
  - ADR-182#Decision
  - ADR-113#Decision
  - ADR-122#Decision
basis:
  - operator: aaronsb
    level: directed
    said: "so I was thinking of a new feature for agent-ways. basically, we have an extensible attend awareness module that should let us run arbitrary scripts and the contract for the scriupt definition gets ran on cadence. I realized that a great way to check for updates to agent-ways is to have one of these attend messages come not from another agent but from attend system, in this case it could send a message to the agent on start that ways is out of date."
    via: session 2026-11-21
  - operator: aaronsb
    level: directed
    said: "so, we previously had a hook for this, and it might still exist actually - it was supposed to tell what version of ways was running etc. I believe it required at least git if not github. that legacy code might still be there and it's probably time to remove it."
    via: session 2026-11-21
  - operator: aaronsb
    level: guided
    said: "ok so maybe what has happened is we built a claude code mode to give attend awareness, and we've only ported over that component into the claude code mod"
    via: session 2026-11-21
  - operator: aaronsb
    level: directed
    said: "I thihnk we have an opportunity to simplify and consolidate on the claude code mod path and probably remove unneeded code after that. if all awareness functions come through the mod, then those channels make it a lot easier to have a consistent session"
    via: session 2026-11-21
  - operator: aaronsb
    level: directed
    said: "the awareness is furnished through the mod. otherwise it starts to smell like an inferior fallback path that requires exstra work tomaintain"
    via: session 2026-11-21
  - operator: aaronsb
    level: directed
    said: "wouldn't the attend mod itself be able to invoke an instance of the attend binary under that claude code instance, and therefore the attend host is just that instance?"
    via: session 2026-11-21, relayed verbatim by the coordinating session; it settles who runs the sensor host (item 2)
  - operator: aaronsb
    level: directed
    said: "do we really need a daily sentinel for this? for example claude code itself if I launch multiple copies all of them might say 'new version available restartart claude code to get it' and it's not really annoying., in this case, if agent-ways is out of date (which attend is part of) then every session gets an awareness with a supression for the rest of that session, which is an ok cadence"
    via: session 2026-11-21, relayed verbatim by the coordinating session; it sets the update notice's cadence (item 6)
  - operator: aaronsb
    level: directed
    said: "no I thinkj we can literallyu run 'ways' and it has a status tool built in. thats why I was thinking (attend script definition that calls ways->logic to parse) -> refire once per session in signal definition -> attend host"
    via: session 2026-11-21, relayed verbatim by the coordinating session; it makes the update check an ordinary command sensor (item 6)
  - operator: aaronsb
    level: directed
    said: "(where attend host runs both builtin and external defined items)"
    via: session 2026-11-21, relayed verbatim by the coordinating session, following the previous answer
  - evidence: "the mod calls these attend verbs and no others (plugins/attend/hooks/lib.ts, register.tsx, 2026-11-21): message send, reply, inbox --drain --json [--wake], unread --json; channel join; listen on --host mod, off, beat; peer list; keepwarm check [--policy --json --autocompact], reset, wrapped, compacted. It runs no sensor poll and uses only $.process.run, never $.process.spawn"
  - evidence: "sensors run only under `attend run` (tools/attend/src/cmd/run/, 1,728 lines over mod.rs, tick.rs, governor.rs, stamps.rs and rekey.rs). The same loop carries a second message poller with its own cache-lapse gate (CacheGate in tick.rs), a reload plan that holds lines on a lapsed cache (reload_plan), the Monitor startup banner, and a re-exec on a session-id change after /clear (rekey.rs). `attend sensor` offers list, poll, fire and permissions"
  - evidence: "command and mcp sensors are parsed and linted but not yet run: attend/sensors/example-command.yaml and tools/attend/src/sensors/mod.rs say they read `degraded` until attend runs them; tools/attend/src/sensors/hosted.rs keys a sensor's store by a hash of its name and the project, and says trigger memory joins that file when command and mcp sensors run. ADR-403 §3 and §3b keep trigger memory across restarts, and a fresh store's first poll sets a baseline and fires nothing"
  - evidence: "at a turn's end the mod schedules a forced tick 250 ms after turn.complete (register.tsx), which runs the wake drain: it delivers all that is pending only when something pending would wake the session. On the operator's prompt it attaches pending messages only when the cache had lapsed or a held-back note waits. A non-waking #open post therefore reaches a warm session through the Stop-hook drain (hooks/ways/attend-drain-stop.sh, settings.json Stop entry) or with the next waking delivery"
  - evidence: "the legacy update check, hooks/check-config-updates.sh (SessionStart entry in settings.json; PROJECTED_FILES in tools/ways-cli/src/cmd/manifest.rs:34; fixtures in reconcile.rs, tests/hooks-test.sh and tests/installer-report-test.sh), fetches the app dir at most hourly and counts commits in HEAD..origin/main, writing /tmp/.claude-config-update-state-<uid>. `ways show core` (tools/ways-cli/src/cmd/show/mod.rs:1101) prints git_version(projection_root()), update_status_text() and dirty_status_text() from show/metrics.rs. git_version runs `git describe --match v*` on ~/.claude, which since ADR-142 is not a git repository and whose release tags are ways-v*, so the footer reads `_Ways version: unknown_` and the dirty check prints nothing"
  - evidence: "`ways manage update --what-if` resolves the channel and target in tools/ways-cli/src/cmd/update/whatif.rs: gather() runs `git fetch origin` in the app dir, takes the branch's upstream (origin/main when detached), reads each component's version at that revision from its Cargo.toml, and compares it with the installed binary's; release notes come from the GitHub REST API only when a component moves. It has no --json. `ways status --json` (ways 1.54.2) reports installed versions and no available release"
  - evidence: "the shipped sensor files are context, disclosure, git, keepwarm, peers and processes, plus two disabled examples (attend/sensors/). keepwarm.yaml rides the message lane through lane_exception; peers.yaml rides it with author: sender; disclosure re-prints tools/attend/src/sensors/disclosures/messaging.md on token distance"
  - upstream: "Claude Code 2.1.329 plugin types (claude-code.d.ts, headed EARLY ACCESS), $.process: run is one-shot, 30 s by default and ten minutes at most; spawn starts a child by argv and streams { stream, text } pieces then { code, signal }, and leaving the loop, return(), an aborted next.signal or the module unloading kills the child, and nothing else does. Its documented example starts a child for the session's life on session.start"
  - upstream: "Claude Code 2.1.329 plugin types: $.process.hookEnv returns CLAUDE_CODE_SESSION_ID, CLAUDE_PROJECT_DIR and the other session variables for a child's env; run and spawn set none of them"
  - precedent: ADR-406
  - precedent: ADR-403
  - precedent: ADR-136
  - precedent: ADR-182
  - precedent: ADR-172
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Keep Stop-hook drain (Recommended)"
    via: session 2026-11-21, question tool, option (a) of the turn-end probe
    covers: [turn-end]
  - operator: aaronsb
    said: "Version only (Recommended)"
    via: session 2026-11-21, question tool, option (a) of the dormant-footer probe
    covers: [dormant footer]
  - operator: aaronsb
    said: "let's accept it and implement"
    via: session 2026-11-21, after the turn-end and dormant-footer answers
  - operator: aaronsb
    said: "Wait for the tag (Recommended)"
    via: "session 2026-11-22, question tool, review of #1319 finding 2"
  - operator: aaronsb
    said: "Silent, state exposed (Recommended)"
    via: "session 2026-11-22, question tool, review of #1319 finding 3"
  - operator: aaronsb
    said: "Keep it until #1316"
    via: "session 2026-11-23, question tool, #1313 review"
  - operator: aaronsb
    said: "#1316+#1317, then release"
    via: "session 2026-11-23, question tool, #1316 plan"
  - operator: aaronsb
    said: "Host registers"
    via: "session 2026-11-23, question tool, #1316 plan"
  - operator: aaronsb
    said: "Delete, use config"
    via: "session 2026-11-23, question tool, #1316 plan"
status: accepted
date: 2026-11-21
deciders:
  - aaronsb
related: [ADR-401, ADR-172, ADR-113, ADR-199, ADR-414, ADR-415, ADR-413, ADR-142]
observable:
  - "In a listening interactive session, `attend peer status` (or the process table) shows one `attend sensor host` child of the Claude Code process, started with the session's id and directory; it is gone once Claude Code exits or the session stops listening"
  - "A dormant session has no host child"
  - "A branch switch in the project of an idle, listening session starts no turn; the line reaches the model with the operator's next prompt or the next delivery"
  - "With the install a release behind, each newly started listening session gets one notice that agent-ways has a newer release, naming installed and target versions, and no second notice in that session; a second session started beside it gets its own"
  - "`attend run` is not a command; `/attend` in a session without the mod says the mod is not loaded and how to enable it, and starts no Monitor"
  - "The session-start footer reads `_Ways version: <the ways binary's version>_`"
  - "No SessionStart hook runs check-config-updates.sh, and ~/.claude/hooks/check-config-updates.sh is not linked after a repair"
---

# ADR-416: Awareness reaches Claude Code only through the attend mod, whose sensor host writes system signals to the store

## Summary

- **Decided:** awareness reaches a Claude Code session through one path, the attend mod (ADR-406). The Monitor path (`attend run` under Monitor, `/attend` launching it, and its guidance) is removed, not kept as a fallback. Each listening session's mod spawns one sensor host, a long-lived `attend` child process, for the session's directory and id. The host runs the compiled sensors and the file-defined `command` and `mcp` sensors of ADR-403. It writes what they report into the signal store as signals from a new sender kind, `system`, addressed to its session. The mod delivers them through the drain it already uses for messages, with the same seen-set, wake gate and unread count. Environment reports ride the event lane and never start a turn. The first new sensor is a shipped `command` sensor that runs `ways` to compare the install with the latest release on its channel and tells each session once. It replaces the legacy update hook, and the session-start footer reads the version from the `ways` binary.
- **Trades away:** a session without the mod gets no awareness at all: a `claude -p` run or SDK session that does not load plugins, a machine whose policy blocks user-installed plugins, and any session after a Claude Code update breaks the early-access mod API. Monitor was the mitigation for that last case, and it goes. A release that lands mid-session is noticed by the next session. Several sessions starting together each fetch the app source once and each say the notice once. Two sessions in one project each run their own git and process polls.
- **One-way?** No. Reversible until the deletions land. After that, restoring Monitor delivery means restoring about 1,700 lines of the run loop and three copies of guidance from git, which is expensive but not a rewrite.
- **Probes:** *Not confident (turn-end):* when a message or a notice arrives while Claude is in the middle of a turn, should Claude read it at the end of that same turn, as the turn-end hook does today, or is it fine for it to arrive just after the turn as a new one, or with your next prompt? *Not confident (dormant footer):* in a session that is not listening, should the start of the session still say that agent-ways is out of date, at the price of a network fetch at every start, or only show the installed version?
- **Inversion:** between several delivery paths, each with its own gate and its own guidance (Monitor, the turn-end hook, the mod), and one path that all awareness takes. The decision takes one path and gives up every host that cannot run it.

## Context

ADR-406 moved attend's agent-facing side into a Claude Code mod: a plugin whose function hooks run inside the session, poll attend's store, and submit deliveries as turns. It kept Monitor as the fallback until the plugin API left early access and SDK hosts could load mods (item 9). The mod took over messages and keepwarm. Sensors did not move: the git, process, presence and context sensors still run only inside `attend run`, which Monitor hosts. So a session listening through the mod hears peers but not its environment, and `attend run` still carries a second message poller with its own cache gate, reload rules and output format that only Monitor needs.

Meanwhile ADR-403 defined `command` and `mcp` sensors as YAML files, so that a new observation needs no change to attend. Those kinds parse and lint but are not run yet. The first use the operator wants is an update check: a notice from attend itself, not from a peer, that the install is out of date. A legacy SessionStart hook was meant to do this. It still counts commits behind `origin/main` rather than releases, and the footer that reads its result has printed `_Ways version: unknown_` since `~/.claude` stopped being a git clone (ADR-142).

The operator set the direction: awareness is furnished through the mod, and a second path is an inferior fallback that costs maintenance. Claude Code's plugin API offers what a host needs: `$.process.spawn` starts a child that lives until the module unloads or the loop ends.

## Decision

1. **One awareness path.** Every inbound delivery to a Claude Code session goes through the attend mod: peer messages, sensor reports, the update notice and keepwarm's wake. `attend run`, the Monitor launch in `/attend`, and the guidance for both are removed. A session the mod has not enrolled gets no delivery. This includes a session that ran `attend listen on` from a shell. The CLI stays for what a person or agent does by hand: `attend message send|reply|inbox|unread`, `attend channel`, `attend peer`, `attend keepwarm`, `attend sensor`. ADR-406 item 9's condition for removing Monitor (the API leaving early access) is not waited for; the operator's direction replaces it.

2. **The host is the mod's child.** When a session starts listening, and the session may poll (ADR-406 item 11: interactive, or a `-p` run the config allows), the mod spawns one host with `$.process.spawn`: `attend sensor host --session <id> --cwd <dir>`. The mod passes the session id and directory as arguments rather than leaving the child to find them by process ancestry. The host runs until the mod ends its loop, which the mod does when the session stops listening, or until the module unloads at exit. A dormant session spawns no host.
   - The host runs every enabled sensor its layers declare for that directory: the compiled `crate` sensors and the `command` and `mcp` files, with ADR-403's cadence, thresholds, triggers, state and project trust.

     > Amended 2026-11-23: a process the host starts to run a command a sensor file or a repository can choose does not act as the session, even after it detaches from the host. The host sets `ATTEND_SENSOR_HOST` in the environment of each such process, naming itself by pid and start time, and an `attend` whose environment names a live attend that way refuses every verb that acts as the session, as one with a host among its ancestors does (ADR-403 §1, amendment of 2026-11-23). This guard prevents accidents. Outside its scope are a sensor command that unsets the marker on purpose, and one that has a process already running outside the host, such as a service manager, a multiplexer's server, `at`, `ssh` or `sudo`, start attend for it. Project trust of the sensor file (ADR-403 §2) is the control against intent.
   - Its stdout carries one short line per signal it writes, and nothing else. The mod reads a line as a cue to run its poll at once. Its stderr goes to the mod's debug log. A host that exits on its own is spawned again after a delay, at most three times in ten minutes. After that the row says the host stopped, and the session keeps its messages.
   - A `/clear` or an in-session `/resume` keeps the module loaded, so the child survives it and keeps the old session id. The mod already follows the move to the new id (`watchMove`). When the move settles, it ends the old host's loop and spawns a new one with the new id. Signals the old host addressed to the old id are still delivered, because an id the session had before a `/clear` counts as its own (ADR-406 item 2). The host has no rekey logic of its own.

     > Amended 2026-11-23: the mod passes two more arguments, and how it treats the host's end is settled (#1312). `--parent <pid>` names Claude Code's pid, which the mod reads as `CLAUDE_PID` from `$.process.hookEnv()`. The host exits once that pid is gone, checked before its sensors start and on every pass of its loop, so a Claude Code that exits between the spawn and attend's `main` leaves no host behind. On a Claude Code without `process.hookEnv` the mod passes no `--parent`, and the host watches its own parent alone. The host the mod spawns after a `/clear` carries `--moved-from <old id>`, and moves the old id's sensor stores and session trigger memory to the new id as it starts (item 8). Where a `/resume` came before the `/clear` in one move, the old id is the one the `/clear` ended, not the id the last host ran for. That store move is the host's only rekey. A `/resume` spawns a host without it, which goes on with the stores of the session it resumed. The mod keeps the host it wants in its state, so a reload during a `/clear`, once the mod has recorded the move, also spawns the first host under the new id with `--moved-from`. A reload in the moment between the `/clear` and the mod's first write of the move, a timer of 0 ms, finds no record and spawns that host without it; this gap is known. The respawn delay is five seconds. A host that exits with attend's usage code 2, or whose spawn a policy denies, stops at once with no respawn, since another spawn gets the same answer. A stopped host starts again when the agent calls `attend_listen`. The respawn window keeps its history, so a host that keeps dying stops again after the cap. A reload of the mod, or a new Claude Code process, also starts it.
   - On a change to the attend binary, the host re-executes itself in place. The pid and pipes stay the same, so the mod's loop does not notice.

3. **Transport is the store.** The host writes each report as a signal in the store, addressed to its own session. It does not stream reports to the mod on stdout. The store path keeps one delivery path. The drain marks a signal read once whichever reader takes it. The wake gate, the cold-cache hold, the unread count on the row and `attend message unread` already apply to anything the drain returns. A report written while the mod is busy or reloading waits, as a message does. Content on stdout would need a second gate, a second seen-set and a second unread count in the mod's TypeScript, and a report would be lost whenever the mod restarted. The cost is one file write per report and the poll's latency, which the stdout cue removes.

   ```mermaid
   flowchart LR
     subgraph session[Claude Code session]
       mod[attend mod]
       model[model]
     end
     host[attend sensor host<br/>child of the mod] -- system signals --> store[(attend signal store)]
     peers[peers and attend-chat] -- messages --> store
     host -. one line per write .-> mod
     mod -- drain: seen-set, wake gate, unread --> store
     mod -- delivery --> model
   ```

4. **A system sender and one lane rule.**
   - The envelope gains a third `from_kind`, `system`, for what attend itself writes. `from` is `system:<sensor>` and `from_id` is the sensor's name. Readers render it as a line from attend, not from a person or agent. ADR-401 is a spec. It is edited to match once this record is accepted, and names this record in `decided_by`.
   - A system signal carries its lane and, on the event lane, an expiry. Two tags join ADR-401's fenced set: `lane:` and `expires:`.
   - The lane rule from ADR-136, as ADR-403 §3a states it, has no exception. Words a person or agent composed ride the message lane. A sensor whose items carry an author, such as chat read through an MCP server, names it with `author`. Everything else rides the event lane, whether the environment or attend itself produced it.
   - An event-lane signal never starts a turn. It reaches the model at the end of the turn in progress through the Stop-hook drain, or with the next waking delivery or the operator's next prompt, whichever comes first. The drain drops it unread once it expires. The expiry is the sensor's decay window, one hour unless the file sets `expires`. The host applies the event lane's salience, refractory period and rate governor before it writes, as `attend run` did before it printed. A message-lane system signal wakes under the same gate as an addressed message.
   - Keepwarm's lane exception goes. Its wake is not a signal: the mod runs `attend keepwarm check` and submits the line itself (ADR-406 item 13, ADR-414). So the shipped `keepwarm.yaml`, which existed only for `attend run`, and the `lane_exception` field are removed.

     > Amended 2026-11-23 (#1316): the shipped `keepwarm.yaml` was also the switch `attend keepwarm check` looked for, so its removal moves the switch to the config key `mods.attend.keepwarm`, which the mod already read: `check` judges nothing where it is `off`. The compiled keepwarm sensor and its entry among the crate sensors go with the file, since nothing runs them. The documented opt-out was a user's or a project's `keepwarm.yaml` with `enabled: false`. For one release `check` still reads those files as the layers did, the project's over the user's, and treats an `enabled` that comes to `false` as off, so a session that opted out does not start waking; `attend sensor list`, `attend config lint` and `ways settings lint` name each file and the config key to move to. With keepwarm and the peers sensor's messages gone, no compiled sensor has a fixed lane.

5. **What the host runs, and what leaves.** The host keeps git, processes, context, and the presence half of peers: a session appearing or leaving, on the event lane. The peers sensor's message scanning goes, because the drain carries messages. This settles #139's split for Claude Code. The disclosure sensor and `disclosures/messaging.md` go, because the mod's preamble cadence covers what they re-printed (ADR-406 item 4).

   Of `tools/attend/src/cmd/run/`, these stay as the host: the sensor slots and their scheduling in `tick_iteration`, the disclosure governor (`governor.rs`), the stamp sweep (`stamps.rs`) and the in-place re-exec on a binary change. These go: the message-lane poller and its flush, `CacheGate`, `reload_plan` and its held lines, the startup banner and its fingerprint, `rekey.rs`, `--catchup`, and the Monitor formatting in `emit/`. In `attend-presence`, a run's enrollment lock and the "held by `attend run`" rules go. A session listens through the mod, or does not listen.

   > Amended 2026-11-23 (#1316, #1108): `attend run` was the only process that registered a session in the instance registry (ADR-129), so the host takes that over. It registers its session's instance slot as it starts and refreshes it with the tick loop's minute touch. A host started with `--moved-from` first moves the old id's slot to the new id, so the session's name keeps its suffix across `/clear`. A listening session with a host therefore shows its suffix, and a session with none shows its bare nickname.

6. **The update check is a shipped command sensor.** No update-specific code goes into attend. The check is a `command` sensor whose argv runs `ways`. attend's trigger logic reads the JSON and decides.
   - **The ways side.** `ways manage update --what-if --json` prints the check as one JSON document, built by `whatif::gather`, the resolver the update itself uses. In JSON mode the release notes are left out, so the check makes no GitHub REST call and needs no token. Its one network step is the `git fetch origin` that `gather` already runs in the app source. A plain `ways status --json` stays offline, which is why the check sits on the update verb and not behind a status flag.

     ```json
     {
       "channel": "release",
       "upstream": "origin/main",
       "target": "<commit>",
       "fetch_error": null,
       "behind": true,
       "summary": "ways 1.54.2 → 1.55.0, attend 0.40.1 → 0.41.0",
       "components": [
         { "name": "ways", "installed": "1.54.2", "target": "1.55.0", "moves": true }
       ]
     }
     ```

     `behind` is true when any component moves (`Component::moves`), that is, when an installed version is older than the version at the channel's target. `summary` lists the components that move. A failed fetch sets `fetch_error` and compares against the last fetched target.

     > Amended 2026-11-22: a suite binary counts as behind only once its release tag, `<name>-v<version>`, is fetched, since the update downloads a release and a bump on main without its tag cannot be installed. A mod counts under the tag of the release it ships with, `attend-v<version>` for the attend mod and `ways-v<version>` for the other known mods, because a release bumps each with its binary; an unknown mod counts by version alone. The tag is pushed before the release's binaries are built, so the notice can come a little before a download gets the new version. A checkout where `git pull --ff-only` would stop reports `behind: null` and a `blocked` field, `detached` when HEAD is on no branch, `no-upstream` when the branch has no upstream set, and `diverged` when history has split, and the sensor stays silent. An upstream that is set but not fetched yet is not blocked, and `behind` is null until a target resolves. Nothing moves while `blocked` is set. Anything built from source under `--ref` keeps the version rule.
   - **The sensor file**, shipped as `attend/sensors/ways-update.yaml`:

     ```yaml
     spec: 1
     name: ways-update
     description: tells the session once when agent-ways has a newer release on its channel
     kind: command
     interval: 604800          # the most the spec allows; the first poll is the one that counts
     min_interval: 604800
     threshold: 1.0
     decay_threshold: 1
     requires: ["Bash(ways:*)"]
     lane: event
     argv: [ways, manage, update, --what-if, --json]
     timeout: 60
     output: json
     triggers:
       - name: behind
         path: "$"
         on: enters
         where: { behind: true }
         memory: session
         expires: session
         magnitude: 3
         description: "agent-ways has a newer release: {summary}. `ways manage update --what-if` shows it."
     ```

     > Amended 2026-11-23: the host applies attend run's magnitude 3.0 cut on the event lane, so the shipped file's trigger uses `magnitude: 3`.

   - **Once per session.** ADR-403's `on: enters` over `where` does not give this alone. Trigger memory persists across restarts and is keyed by sensor and project, so a second session would never fire. A fresh store's first poll sets a baseline and fires nothing, so a session that starts behind would not fire either. Two fields are added to the spec. `memory: session` holds the trigger's memory in the host's process. It starts empty and has no baseline, so `enters` fires on the first poll whose result meets `where`, and never again in that host's life. Since the host lives as long as the session, that is once per session. `expires: session` keeps the notice until the session reads it, so the notice does not lapse with the one-hour event expiry. Both fields are allowed on any trigger.

     > Amended 2026-11-23: a session trigger's line is delivered at most once in the session. A line the loop holds, such as one the governor holds, is not offered again while it waits; a line the loop drops, by the refractory, the cache gate, a failed write or a restart before it went out, is offered again on the next poll that meets the trigger. A line under the event lane's cut counts as handled, and spends the trigger as a written line does. The checkpoint follows the write, so a kill between the write and the checkpoint can repeat the notice.

   - **Cadence.** Each listening session checks once at start and says it once. Several sessions each saying it is the intended cadence, as with Claude Code's own update notice. There is no machine-wide cache and no daily sentinel. Concurrent fetches in one app source can collide on git's ref lock. A session whose fetch loses compares against the last fetched target, which is the same release unless one landed in between.

7. **The footer reads the binary.** `ways show core` prints `_Ways version: <version>_` from the `ways` binary's own version. `git_version`, `update_status_text`, `render_update_status` and `dirty_status_text` in `show/metrics.rs` go, with the git-describe parsing that serves them. The footer shows the version only and makes no network call, in every session. A dormant session learns of a release from `ways status` or `ways manage update --what-if` when asked.

8. **Per-session sensor state.** The store and trigger memory of a sensor that uses `memory: store`, the default, are keyed by sensor, project and session, not by sensor and project. Each report goes to one session, so what a sensor last saw is what that session was told. A session's move after `/clear` carries its stores with it, as it carries membership. Two sessions in one project keep separate stores and each hears a change once.

### Removal and migration, in dependency order

1. **Envelope.** Add `system`, `lane:` and `expires:` to the one wire parser and writer (`tools/agent-identity/src/signal.rs`) and to each reader: the drain (`tools/attend/src/cmd/inbox.rs`), `sensor-peers`, and `tools/attend-chat/src/signal.rs`. attend-chat skips event-lane system signals. Edit ADR-401 to match and set its `decided_by`.
2. **Spec.** In `tools/sensor-spec`, add the trigger fields `memory` (`store` or `session`) and `expires` (seconds or `session`). Remove `lane_exception`, and its reader in `tools/attend-config/src/sensors.rs`. Key stores by session in `tools/attend/src/sensors/hosted.rs`.
3. **Host.** Add `attend sensor host --session --cwd`, built from the parts of `cmd/run/` that item 5 keeps. It runs crate, `command` and `mcp` sensors, which is ADR-403's slice 2, and writes system signals. `attend sensor poll` and `fire` deliver through the same writer.
4. **Mod.** In `plugins/attend/hooks/register.tsx` and `lib.ts`, spawn the host on activation, end it on `listen off`, respawn it on a settled move and after an unexpected exit, and turn a stdout line into a forced tick. Extend `plugins/attend/tests/`.
5. **Delivery.** Teach the drain that event-lane system signals do not wake and expire. Keep the Stop-hook drain (ADR-172) as the mod's turn-end conduit, and limit it to sessions the mod enrolled (`--host mod`).
6. **Update check.** Add `--json` to `ways manage update --what-if` (`tools/ways-cli/src/cmd/update/`), leaving notes out in that mode. Ship `attend/sensors/ways-update.yaml`. Change the footer in `tools/ways-cli/src/cmd/show/mod.rs` and `metrics.rs`.
7. **Delete the legacy update hook.** Remove `hooks/check-config-updates.sh`, its SessionStart entry in `settings.json`, its entry in `PROJECTED_FILES` (`tools/ways-cli/src/cmd/manifest.rs`), with the single-file projection kind if nothing else uses it, so repair withdraws the link from existing installs. Remove its fixtures in `tools/ways-cli/src/cmd/reconcile.rs`, `tests/hooks-test.sh` and `tests/installer-report-test.sh`.
8. **Delete the Monitor path.** Remove the `Run` command in `tools/attend/src/cli.rs` and what item 5 lists from `cmd/run/` and `emit/`. Remove `attend/sensors/keepwarm.yaml`, `disclosure.yaml`, the disclosure sensor and `disclosures/messaging.md`, and the peers sensor's message scanning. Remove the run's enrollment lock and its registration in `attend-presence` and `attend-instances`. Update `tests/make-targets-test.sh`.

   > Amended 2026-11-23 (#1316): removing `keepwarm.yaml` moves keepwarm's switch to `mods.attend.keepwarm`, with the one-release reading of the legacy files that item 4's note describes. The registration in `attend-instances` stays: the host registers its session now (item 5's note). Only the run's enrollment way and lock leave `attend-presence`; a record that still carries the run's lines reads as if it had none.

   > Amended 2026-11-23 (#1316): three things stay for one release, so an install updated under a running 0.35.2 session does not misbehave. `attend run` is a hidden command that writes nothing, prints that awareness comes through the attend mod and to call `attend_listen` or run `ways manage mod check attend`, and exits 2: a leftover run re-executes into it when the binary updates. The observable "`attend run` is not a command" still holds, since a usage error is what it gets. The sensor guard still counts an argv of `attend run` as a host, because a 0.35.2 run predates the sensor marker and its ancestry is the only thing that keeps a command it runs from acting as the session. A user's or a project's `disclosure.yaml` or `keepwarm.yaml` is left out of the catalog and the settings screens instead of reading as invalid, and lint and `attend sensor list` name the removed sensor, and for keepwarm the config key.
9. **Guidance.** Remove step 2 and the Monitor sections of `skills/attend/SKILL.md`, so that `/attend` calls `attend_listen` or says the mod is not loaded and how to enable it. Remove the "Without the mod" section and the `attend run` lines of `hooks/ways/softwaredev/environment/attend/attend.md` and its `macro.sh`. The synchronized copies drop from three to two. Rewrite or delete the `attend run` pages under `docs/attend-and-monitor/`, `docs/cli/attend.md`, `docs/cognitive-loop.md` and `docs/explanation/attend-messaging/`, and the `Makefile` help line.
10. **Records.** On acceptance, edit with dated notes: ADR-406 items 2, 9, 11 and 13 and its Consequences, where they keep Monitor; ADR-403 §3, §3a and §3b; ADR-136 item 1's lane sentence; ADR-182's Decision, which describes a sensor under `attend run`.

### Settled questions

- **Turn-end delivery (probe *turn-end*).** The Stop-hook drain stays as the mod's turn-end conduit. It continues the operator's turn with whatever is pending, waking or not, and delivers only to a session the mod enrolled (`--host mod`). Without it, the mod would deliver at a turn's end only what would wake the session, and non-waking `#open` posts and event-lane reports would wait for the next waking delivery. Removing the drain once the mod can continue a turn itself (a `turn.step` hook exists in the 2.1.329 types, unverified for plugins) stays possible later and is not part of this decision.

  > Amended 2026-11-23: until #1316 removes `attend run`, the Stop hook also delivers to a session whose `attend run` is live, so Monitor sessions keep turn-end delivery across the release.

  > Amended 2026-11-23: #1316 removed `attend run`, and this bridge with it, before a release carried either. The Stop hook delivers only to a session the mod enrolled, always in the mod's format; the CLI-format renderer and the block form for a Claude Code older than 2.1.163 went too.
- **Dormant footer (probe *dormant footer*).** The footer shows the version only, in every session, and makes no network call. A dormant session learns of a release when asked, from `ways status` or `ways manage update --what-if`. Running the check in the footer would add a fetch to every session start and a second notice in sessions that go on to listen.

## Consequences

### Positive

- One delivery path: one gate, one seen-set, one unread count and one set of guidance. Monitor's 30-minute expiry, its line ceiling and its entity escaping stop mattering.
- A listening session gets its environment again: git, processes, presence, context, and any `command` or `mcp` sensor a user writes. `command` and `mcp` sensors finally run.
- The host needs no election, service or lock. Its life is the session's, its directory is the session's, and Claude Code cleans it up.
- The update notice reports releases on the install's channel, with the same judgment `ways manage update` makes, and it needs no GitHub token. The footer prints a real version.
- About 1,700 lines of run loop shrink to the sensor scheduling, and two delivery stacks become one.

### Negative

- No mod, no awareness. A `-p` run that does not load plugins, an SDK host without mods, a managed install that blocks user plugins, and any break in the early-access API leave a session deaf. Only the mod's plugin tests against each Claude Code build guard that last case.
- N listening sessions run N hosts. Two sessions in one project run the same git poll twice, and each session's start fetches the app source once.
- A release that lands mid-session is noticed by the next session.
- Event-lane reports never wake an idle session. A branch switch while the session idles waits for the operator, unlike under Monitor, which woke a warm session for any line.
- Keying stores by session means a new session starts from a baseline. Something that changed while no session listened in the project is not reported to the next one.

### Neutral

- A host spawned by `$.process.spawn` gets none of Claude Code's session variables unless the mod passes them (`$.process.hookEnv`). The explicit `--session` and `--cwd` arguments avoid depending on either the variables or the ancestry walk, which attend memoizes per process.
- `attend sensor poll` and `fire` keep working from a shell and write through the same signal writer, so a test fire reaches a listening session as a `[test]` line.
- ADR-501's `ways-mcp` attend module stays on hold. Nothing here needs it.

## Alternatives Considered

- **Keep Monitor as the fallback (ADR-406 item 9 as written).** Rejected by the operator: a second path is an inferior fallback that costs maintenance. Its cost shows in ADR-406 itself: items 2 and 13 carry Monitor-only rules beside the mod's, and every change to delivery is made and tested twice.
- **The host streams reports on stdout and the mod delivers them.** Rejected for item 3's reasons: the mod would need its own gate, seen-set and unread count for a second source, and reports would be lost across a mod reload.
- **One host per machine, as a user service or elected by lock among listening mods.** Rejected once the mod could spawn a child. A shared host has to serve several directories and sessions, survive the sessions that started it, and be installed and supervised per platform. A child per session has none of that and costs a duplicate poll per extra session in a project.
- **A machine-wide cache or daily sentinel for the update check.** Rejected by the operator. Once per session is the cadence wanted, and the cache would need its own file, lock and expiry for a notice that several sessions may each give.
- **`git ls-remote --tags` or the GitHub releases API for the check.** `ls-remote` reads tags without fetching objects, but it would be a second resolver beside `whatif::gather`, and it ignores a `--ref` checkout. The unauthenticated releases API allows 60 requests an hour per address and fails under many sessions. `gather`'s fetch is the one the update runs anyway.
- **Put the update block in `ways status --json` behind a flag.** Rejected so that status stays a local, offline report. The check is a dry run of the update, and it belongs on that verb.
- **Keep the legacy hook and fix its version line.** Rejected. It counts commits, not releases, keeps its own hourly state file under `/tmp`, and is a second path for a notice the host now gives.
