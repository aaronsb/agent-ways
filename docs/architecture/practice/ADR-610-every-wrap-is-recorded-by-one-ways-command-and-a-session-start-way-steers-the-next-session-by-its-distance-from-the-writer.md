---
contract: adr/v1
kind: decision
verb: add
capability: loop
basis:
  - operator: aaronsb
    level: guided
    said: "I'd like to create a command style sensor that ships with ways, but is not declared as a builtin, just a user scope sensor. it's purpose is to sense a wrap continuation file that was written by the previous session increment. this means that we might need to figure out how to capture the lifecycle of a wrap file written. currently it's just set up as a skill, that is invoked by the user or by claude or by the keepalive towards the end of a session. what would be a better experience is to at session start we, should evaluate any session wrap file we've written, and using a sentinel, or flag or some sort of status, ideally, one that the agent doesn't have to 'manually write' evaluate the distance of session from the last session for that project directory invocation."
    via: session 2026-10-30
  - operator: aaronsb
    level: guided
    said: "if multiple claude codes get invoked in the same directory, then the first one invoked gets the steering. as a fence or guard against loss of context, we could probably afford some resiliance in turn distance rather than immediately adjacent, like (your turn is not the immediate sessions successor to whoever wrote the last wrap, you are x turns away so verify the wrap instructions before taking action on them)"
    via: session 2026-10-30
  - operator: aaronsb
    level: directed
    said: "I think a user scoped (builtin) way with a macro. attend is for awareness, ways are for 'how we operate' and while attend's keepwarm sensor can tell claude 'no more keepwarm reminders will happen, you should run the wrap skill' this means that that type of invocation of wrap needs to be compatible with a way and macro for wrap management."
    via: session 2026-10-30
  - evidence: "Compaction keeps the session id: transcript ~/.claude/projects/-home-aaron-Projects-ai-harness-agent-ways/55c9c23f-22df-457e-9446-90894eb6c3f4.jsonl holds a compact_boundary and one sessionId on all of its ~7500 lines. /clear and a fresh start give a new id. SessionStart's payload carries source: startup, resume, clear or compact"
  - evidence: "settings.json wires PostToolUse to ways for Edit|Write|Bash|Task (check-post.sh, check-queued.sh). SessionStart runs check-state.sh (ways hook state) on startup, compact, resume and clear, and clear-markers.sh (ways hook session-start) on startup, compact and clear. A macro's environment (tools/ways-cli/src/cmd/show/helpers.rs, macro_env) carries CLAUDE_SESSION_ID and CLAUDE_PROJECT_DIR and no SessionStart source"
  - evidence: "Two wrap writers exist. skills/wrap/SKILL.md Step 3 saves the continuation prompt to .claude/handoff.wrap.md at the repository root, and its allowed-tools lists no Write tool. The attend mod's keepwarm wrap (mods.attend.keepalive.wrap in tools/ways-core/src/settings.rs) routes the reply to transcript (the default, no file), file (the mod writes wrap_file, default {state}/wrap/{session}.md, itself), send (an attend message opening with handoff:) or off; plugins/attend/hooks/lib.ts reports the outcome as transcript, file, sent, refused or unfinished"
  - precedent: ADR-199
  - precedent: ADR-414
  - precedent: ADR-165
  - precedent: ADR-504
  - precedent: ADR-611
  - precedent: ADR-131
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "while the session runs, because otherwise a false start or a restart for some reason could loose it"
    via: session 2026-10-30, answering the probes in chat after the drafts were summarized; asked whether a claim holds only while its session runs or consumes the record
    covers: [first-wins]
  - operator: aaronsb
    said: "I'm not so sure about aging out over days. we could surface it still after 7 days and instead state something like it's d days old and could be invalid guidance"
    via: session 2026-10-30, answering the probes in chat after the drafts were summarized; the record was changed to add an age note past T in place of naming the file only
    covers: [reach]
observable:
  - 'run: /wrap in a session writes .claude/handoff.wrap.md with the Write tool, and `ways wrap status` lists a record naming that session, the HEAD and the file hash'
  - 'run: /compact the same session; the session-start line says it wrote the handoff before compacting'
  - 'run: start two sessions in the project within a second of each other; one gets the steer and the other one line naming the session that took it'
  - 'run: a keepwarm wrap with wrap: transcript leaves a reply file under {state}/wrap/ and a record, and the next session in the project is told to read it'
  - 'run: after a wrap, open, prompt and close four sessions one after another; with k = 3 the first is told to read and continue, the second and third to check the handoff against git with HEAD from, HEAD to and the commit count, and the fourth only where the file is'
  - 'run: edit the handoff after the wrap; the next session is told the file was edited after it was written'
  - 'run: resume the writing session with --resume; no wrap line appears'
status: accepted
date: 2026-10-30
deciders:
  - aaronsb
related:
  - ADR-131
  - ADR-165
  - ADR-182
  - ADR-199
  - ADR-414
  - ADR-504
  - ADR-611
---

# ADR-610: Every wrap is recorded by one ways command, and a session-start way steers the next session by its distance from the writer

## Summary

- **Decided:** every wrap leaves a record in ways' state directory, made by one command, `ways wrap record`. Its two callers are the ways PostToolUse hook, when a handoff file at the repository root is written, and the attend mod, after it routes a keepwarm wrap to any destination. A builtin way, `meta/wrap/continuity`, fires at session start on startup, clear and compact, never on resume. Its macro lets the first session to start in the project claim the newest record for as long as that session runs. It works out how many sessions separate the claimer from the writer and scales its steer to that distance: continue, read and continue, check against git first, or only the file's name. No agent writes the record or the claim.
- **Trades away:** the first session to start takes the handoff, even when the operator meant a later one for that work. Every session start reads the first lines of the project's newer transcripts. The wrap skill gains the Write tool, and the attend mod gains a call into `ways`. A handoff written by a Bash command that names its path only through a variable goes unrecorded.
- **One-way?** No. Records and claims are files in ways' state directory, and the handoff files stay where they are. Switching the way off or removing the two calls returns today's behaviour.
- **Probes:** *Confident (first-wins):* when you open two sessions in one project after a wrap, the one that started first is told to pick up the handoff, and the other is told which session took it and does not act on it. *Not confident (reach):* a session up to three sessions after the wrap is told to check the handoff against git before acting, and from the fourth on, or once the handoff is more than seven days old, it is only told where the file is. Are three sessions and seven days the right reach?
- **Inversion:** at one end, a successor finds a handoff only by looking for it, as `start` does today. At the other, every new session is handed the last handoff as its instructions. This decision hands it to one running session at a time, with a steer that weakens with distance from the writer and ends at naming the file.

## Context

A wrap writes a handoff for the next session. Two writers exist today. The `/wrap` skill saves its continuation prompt to `.claude/handoff.wrap.md` at the repository root, and the compaction-checkpoint way adds to that file or writes `.claude/handoff.local.md` (ADR-611). The skill's `allowed-tools` lists no Write tool, so the agent writes the file through Bash. The attend mod's keepwarm wrap (ADR-414) asks for the wrap skill when the keepalives run out and routes the reply to the transcript by default, to a file the mod writes itself, or to an attend `handoff:` message.

Nothing records that a wrap happened. A successor finds the handoff only if it runs `start`, looks for the file, and judges its age from the date and commit on its first line. A wrap left in the transcript leaves no file to find. When several sessions open in one directory, each one that looks reads the same handoff and may act on it.

Claude Code's session identity sets what a successor can be. Compaction keeps the session id, and `/clear` or a fresh start gives a new one. SessionStart's payload names its source: startup, resume, clear or compact. PostToolUse reaches ways for Edit, Write, Bash and Task. SessionStart runs `ways hook state` on all four sources and `ways hook session-start` on startup, compact and clear. A way's macro receives the session id and the project directory, and not the SessionStart source.

The operator first framed this as an attend command sensor, and believed compaction started a new session. Both were revised in session. The host became a way, because attend carries awareness and ways carry how a session operates, and the keepwarm wrap must go through the same way and macro. The transcript showed compaction keeps the session id, which gives the case where the reader is the writer. A ways sensor was not a candidate: ADR-199 §3 lets it see `session_start`, and §8 limits it to observing, so it cannot put a steer in front of the agent.

## Decision

### 1. One command records a wrap

`ways wrap record` saves a record in `{state}/wrap/records/<project key>/`, where `{state}` is ways' state root (`$XDG_STATE_HOME/agent-ways`) and the project key is the one Claude Code uses for the project's directory under `~/.claude/projects/`. A record holds:

- the writer's session id;
- the time it was written;
- git HEAD at that time, or none outside a repository;
- each handoff file's path and SHA-256.

A session has at most one unclaimed record. A later write in the same session updates it: it adds or refreshes that file's entry and moves the time and HEAD forward. The skill's handoff, the compaction checkpoint's addition and the keepwarm reply from one session therefore land in one record. Once any session has claimed the record (§4), the writer's next write starts a new one.

The command takes `--session`, `--project`, and either `--file <path>` or `--reply -`. With `--reply -` it writes the text it reads on stdin to `{state}/wrap/<session>.md` and records that path. It prints nothing and always exits 0. A failure goes to ways' event log, so a hook that calls it is never held up.

`ways wrap status [--project <dir>]` lists the project's records with their claims and each record's distance from the calling session.

### 2. Two callers, whoever invokes the wrap

- **The PostToolUse hook.** `ways hook post-tool` already runs on Edit, Write and Bash. On a Write or Edit whose path is `.claude/handoff.wrap.md` or `.claude/handoff.local.md` at the repository root, it calls the recorder. The wrap skill adds Write to its `allowed-tools` and writes the handoff with it. As a backstop, when a Bash command's parsed segments (ADR-188) name one of the two paths, the hook hashes the file after the call and records it when the hash differs from the session's record.
- **The attend mod.** After routing a keepwarm wrap, the mod calls `ways wrap record` for every outcome that delivered a reply. For `file` and `refused` it passes the file it wrote with `--file`. For `transcript` and `sent` it passes the reply with `--reply -`, so the next session has a file to read. `off` and `unfinished` record nothing. When `ways` is not on the mod's `PATH`, the mod skips the call and keepwarm works as before.

The operator typing `/wrap`, the agent choosing to wrap, and keepwarm asking for the wrap skill all reach the recorder through these two callers.

### 3. A builtin way at session start

The way is `meta/wrap/continuity`, a child of `meta/wrap`, shipped in the corpus with `trigger: session-start`, `macro: prepend` and `scope: agent`, so a teammate never claims a handoff. Its body is a few lines on what the macro's line means. A project switches it off with `ways settings set ways.project.meta/wrap/continuity false` (ADR-131). Recording continues while it is off.

`ways hook state` passes the SessionStart payload's `source` to every macro as `CLAUDE_SESSION_SOURCE`. On UserPromptSubmit the variable is empty. The macro runs `ways wrap claim` with the session id, the project directory and the source. It claims only on startup, clear and compact. On resume, or with no source, it prints nothing.

### 4. The first session to start claims, while it runs

The candidate is the project's newest record. Claims are numbered: `<record>.claim.1`, `<record>.claim.2`, and so on, each holding the claimer's session id, the time and the source. A session reads the highest claim. When there is none, or its claimer has ended, the session creates the next number with exclusive create. Only one session can create a given number, so of two sessions starting together the first takes the handoff, and the other reads the winner's claim.

A claimer is running while `~/.claude/sessions/` holds a record for its session id whose pid is alive with the recorded process start. A session that finds the newest record claimed by a running session gets one line: "The latest handoff, `<path>`, was taken by session `<id>` at `<time>`." Once the claimer has ended, the next session to start claims the record again, at its own distance (§5).

### 5. Distance from the writer

Distance is 0 when the claimer is the writer, which happens when the writing session compacts.

Otherwise distance is 1 plus the number of sessions in `~/.claude/projects/<project key>/` that started after the record was written and before the claimer. A session's start is its first transcript timestamp. Only sessions with at least one operator prompt count; a tool result or a harness notification is not a prompt. The claimer has no prompt yet at SessionStart and counts as the 1. The transcripts are read through the `claude-sessions` crate (ADR-504), which already backs `ways projects`.

A session that started before the wrap and kept working after it is not counted. Its commits still show in the HEAD comparison of §6.

### 6. The steer scales with distance

| Distance | Line |
|---|---|
| 0 | You wrote `<path>` before compacting. Continue from it. |
| 1 | Read `<path>`, the handoff from the previous session here, and continue. |
| 2 to k | N sessions started here since `<path>` was written. HEAD moved `<from>`→`<to>`, M commits. Check the handoff against git before acting on it. |
| beyond k | An earlier handoff is at `<path>`, written `<date>`. |

Age never withholds a steer. A handoff written more than T days ago adds to whichever line applies: "`<path>` is d days old and may be stale guidance."

At distance 1, a HEAD that moved adds the HEAD line to the steer. When the recorded HEAD is not an ancestor of the current one, the line says HEAD is no longer on the commit the handoff was written at. A file whose hash differs from the record adds "`<path>` was edited after it was written." A missing file replaces the steer with "`<path>` is gone." Outside a repository there is no HEAD line.

### 7. Initial values

`ways wrap claim` owns these and may change them without a new decision:

- k, the last distance that gets a steer: 3;
- T, the age past which the steer gives the handoff's age and says it may be stale: 7 days;
- retention: the recorder removes records, claims and its own reply files older than 30 days.

## Consequences

### Positive

- The next session hears about the handoff at its start, whether or not anyone runs `start`.
- A keepwarm wrap left in the transcript or sent as a message also leaves a file the next session can read.
- Sessions opened together in one directory no longer each act on the same handoff.
- A stale handoff is judged by the sessions and commits since it was written, as well as by its date.

### Negative

- The first session to start takes the handoff, even when the operator opened a later one for that work. The later session is told who took it, and the file stays readable.
- SessionStart carries no sign of a non-interactive run, so a `claude -p` run started in the project claims like any session while it runs, and counts toward distance afterwards. Teammate sessions do not claim, but each one with a prompt counts toward distance, which errs toward more checking.
- Liveness rests on Claude Code's `~/.claude/sessions/` records, which are not a documented interface. If their shape changes, every claim reads as ended and each new session claims at its own distance.
- Each session start lists the project's transcripts newer than the newest record and reads each to its first operator prompt.
- The attend mod depends on the `ways` command for the record.
- A Bash command that writes the handoff through a variable or a script goes unrecorded. The skill's Write is the supported path.

### Neutral

- Every macro gains `CLAUDE_SESSION_SOURCE`.
- `ways` gains a `wrap` group (`record`, `claim`, `status`), which the command regrouping of ADR-507 places with the others.
- The `start` skill keeps its own check of the handoff file. The record only points at the file and changes nothing in it.

## Alternatives Considered

- **An attend command sensor**, the operator's first framing. Rejected: attend carries awareness, and handing a session its predecessor's work is how a session operates. It would also miss every session where attend does not run.
- **A ways sensor (ADR-199).** Rejected: §8 limits a ways sensor to observing and dispatching outward, so it cannot steer the agent.
- **The agent records the wrap itself**, with the skill running `ways wrap record` as a step. Rejected: the operator asked for a status the agent does not write by hand, a skipped step leaves no record, and the keepwarm wrap runs with nobody watching.
- **Judge the handoff file alone at session start**, by its modification time and the date and commit on its first line. Rejected: with no writer session id there is no distance 0 and no claim, and a wrap left in the transcript has no file.
- **Distance by wall-clock time alone.** Rejected: the operator measured distance in sessions. A handoff one hour old with three sessions since is further from its reader than one a day old with none. Past T, age adds a staleness note to the steer.
- **Name the file only once the handoff is older than T.** Rejected by the operator: a handoff a week old can still be the right next step, so the steer stays and says how old the handoff is.
- **Counting every session, prompt-less ones included.** Rejected: a session opened and closed without a prompt changed nothing.
- **Claiming through attend's peer presence or a held lock.** Rejected: an exclusive file create is atomic on a local filesystem and needs no running process.
- **A claim that consumes the record for good.** Rejected: only the first session after a wrap would ever see it, and the operator asked for later sessions to be told how far they are from the writer. A claim holds only while its claimer runs.
- **Watching the handoff paths with a stat on every hook call.** Rejected: it adds a cost to every tool call to catch a write that the Write path and the parsed Bash segments already catch.
- **Steering on resume too.** Rejected: a resumed session already holds its context, and the operator chose it by resuming.
