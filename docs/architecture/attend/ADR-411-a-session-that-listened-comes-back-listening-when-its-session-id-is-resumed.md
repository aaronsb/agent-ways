---
contract: adr/v1
kind: decision
verb: change
capability: attend
basis:
  - operator: aaronsb
    level: directed
    said: "Resume restores (Recommended)"
    via: "AskUserQuestion through the coordinator session, 2026-10-12; question: \"An attend_listen activation currently ends when Claude Code exits, so `claude --resume` comes back dormant (ADR-406 item 2). That's what happened to McKay. Should a resumed session come back listening?\"; option label and text written by the agent, chosen by the operator: \"The mod keeps the mode in $.store, keyed by session id, which --resume preserves. A resumed conversation already believes it's on attend, so coming back silently dormant leaves it waiting for messages that never arrive. This amends ADR-406 item 2 via adr consider; a new session id still starts dormant.\""
  - operator: aaronsb
    level: directed
    said: "resume should definitely resume attend"
    via: "session chat through the coordinator, 2026-10-12, added to the Resume restores answer"
  - evidence: "McKay's session in attend-collaborator, 2026-10-12: activated with attend_listen, then the process exited and `claude --resume` started a new one (pid 1944850, `claude --resume`, started 13:57:11Z; the transcript's cost-state record closes the earlier process). The resumed session listed only attend_listen; the transcript's deferred_tools_delta removed attend_send, attend_reply, attend_join and attend_peers."
  - evidence: "probe mod under a throwaway CLAUDE_CONFIG_DIR, Claude Code 2.1.296, 2026-10-12: $.store written in one process reads the same after the process exits and `claude --resume <id>` starts a new one, where $.session.id() at session.start is the resumed id and the settings SessionStart (source resume) fires before session.start. An in-session /resume ends the old session with reason resume, empties $.state, and raises the settings SessionStart with source resume and session_id the resumed id, while $.session.id() reads the old id until that event settles. `claude --resume <id> --fork-session` raises SessionStart with source fork and the fork's new session_id and nothing naming the parent; the fork's transcript is not yet written at session start."
  - upstream: "Claude Code 2.1.296 plugin API: $.store is a plugin's key-value store kept between sessions and reloads, one JSON file per plugin identity, with get, set, delete and keys and no versioned write"
  - precedent: ADR-406
agent:
  name: Claude
  model: claude-opus-5-5
amends:
  - ADR-406#Decision
status: accepted
date: 2026-10-12
deciders:
  - aaronsb
related:
  - ADR-406
observable:
  - "A session in a dormant project that called attend_listen, then exited, comes back listening under `claude --resume <its id>`: the row shows, all five attend tools are listed, and attend lists it as listening"
  - "A new `claude` in that dormant project, and a `--fork-session` child of the listening session, start dormant with attend_listen alone listed"
  - "`/resume` inside a running dormant session, to a session that listened, turns the session on"
---

# ADR-411: A session that listened comes back listening when its session id is resumed

## Summary

- **Decided:** An `attend_listen` activation now lasts as long as its session id, not just its process. The mod records the id in its own `$.store`, which Claude Code keeps on disk. A resume of that id comes back listening: `claude --resume` in a new process, or `/resume` inside a running one. A `/clear` records its new id the same way. Any other new id, a `--fork-session` child included, starts as the project's config says. This amends ADR-406 items 1 and 2, which ended activation at process exit and persisted nothing.
- **Trades away:** "Nothing carries over between processes" no longer holds without exception. The mod keeps one small record on disk across processes: at most 50 session ids. A fork of a listening conversation starts dormant, although it believes it is on attend.
- **One-way?** No. Removing the record restores ADR-406 as written, and the record holds nothing but session ids.
- **Probes:** *Confident (resume):* You resume a session that was on attend and it is on attend again, with no attend_listen call. A brand-new session in a dormant project is still plain Claude Code. *Not confident (fork):* A `--fork-session` copy of a listening session starts dormant, because the fork does not reveal which session it came from.
- **Inversion:** The two ends are activation that ends at process exit (ADR-406 as written) and activation stored per project, which would make every later session in that project listen. Per session id sits between them. It follows the conversation the user resumes, not the project.

## Context

ADR-406 item 2 ends an activation when Claude Code exits and persists nothing, so the next `claude` in a dormant project is dormant. `claude --resume` is a new process too, so a resumed conversation came back dormant. That conversation had already called `attend_listen`, been told it was listening, and talked to peers. Coming back dormant left it waiting for messages that never arrived, and the change in its tool list looked like a fault. The operator chose that a resume restores listening.

## Decision

1. **The record.** `$.store` holds one list of the session ids that listened, newest last. The mod keeps at most 50 entries and drops the oldest. The record holds no message state and nothing else about a session.
2. **What is recorded.** An id is recorded only when the project's config would not start that session listening on its own: an `attend_listen` call, or a `/clear` of such a session. Two cases skip the write:
   - a session in an `attend_mod.listening: active` project, whose resume starts active anyway;
   - an id the record already lists, as on a reload or a resume.

   So the record is written rarely, which keeps the cap from evicting ids that are still wanted.
3. **What restores.** Two paths turn a listed id back on:
   - **Process start.** `session.start` treats the session as active when `$.state` says so (a reload) or when the record lists the session id (`claude --resume`, which keeps the id).
   - **In-session `/resume`.** It raises the settings SessionStart with source `resume` and the resumed id, after Claude Code has emptied `$.state`. A resume to a listed id turns the session on. A session that is already active stays active, as ADR-406 keeps it for the process.
4. **New ids start from config.** A fresh `claude`, and any new id the record does not list, start as the project's start state says. That includes a `--fork-session` child. The fork's SessionStart carries source `fork` and the fork's own id, and nothing at session start names the parent. The rule "a resumed conversation listens" cannot be applied to a fork without guessing.
5. **The record never gates the session.** The mod tells attend the session is listening and starts the poll before it writes the record. A store write that fails is logged as a transcript line, and the session goes on listening.
6. **No deactivation.** The mod has no verb that turns listening off, so nothing removes an id; the cap bounds the record.
7. **Per plugin identity.** The record lives in the plugin's own store. A session loaded with `--plugin-dir` (`attend@inline`) keeps a record separate from `attend@agent-ways`, so a resume under the other identity starts dormant.

## Consequences

### Positive

- A resumed conversation that was on attend is on attend again, with its tools listed from turn one.
- A dormant project's new sessions stay plain Claude Code, as ADR-406 intends.

### Negative

- `$.store` offers no versioned write, so the write is a read, then a set. Two processes recording at the same moment can lose one id. The rare writes in item 2 keep that window small, but it remains.
- A forked conversation that believes it is on attend starts dormant, and the user must ask for attend again.
- A session id from a record of 50 or more activations ago starts dormant on resume.

### Neutral

- The record is one JSON list under the plugin's store file in the Claude Code config directory, shared by every project in that directory. Session ids are UUIDs, so one session's entry can only match a process that resumes that same id.

## Alternatives Considered

- **Keep ADR-406 as written: a resume starts dormant.** Rejected by the operator: the resumed conversation already believes it is on attend.
- **Persist per project.** Rejected: it would make every later session in the project listen, which ADR-406's dormant default exists to prevent, and which `attend_mod.listening: active` already offers when wanted.
- **Let a fork inherit by reading the parent's transcript.** Rejected: at session start the fork names no parent and its transcript is not yet written. Finding the parent would mean reading Claude Code's transcript files, which the mod does not own.
- **Record every activation, config-driven ones included.** Rejected: active projects and scripted `-p` runs would write the record often, churn the cap and widen the lost-write window, for sessions their config restores anyway.

## Addendum, 2026-10-21: the mod reads a /clear and a /resume from session.end

Appended after acceptance; nothing above is changed. Item 3 named the settings SessionStart as the event an in-session `/resume` raises, and the mod hooked it as `classic.SessionStart` for `/clear` too. Where an organization manages Claude Code (Team and Enterprise, or a machine with managed settings), Claude Code 2.1.306 seats its built-in policy plugin outermost, and that plugin sends every `classic.*` event, and `session.compact`, past the plugins a person installs. The mod's hook never ran there, so a `/clear` or a `/resume` left attend unaware of the id that went on.

- **The trigger.** `session.end` with reason `clear` or `resume` reaches the mod in every host. The mod notes whether the session was listening, then reads `$.session.id()` until it no longer names the ending id, as the 2.1.296 probe found it reads the old id for a while. It then restores the session as item 3 says. A main-loop turn that starts first does the same at once. The mod hooks no `classic.*` event.
- **Compaction.** The preamble's after-compaction flag (ADR-406) is set by the person's `/compact` (`command.run`), by the mod's own compaction after a wrap (ADR-414), and by a `session.measure` whose context fill fell below half of the previous one or reads as absent, which a compaction at the engine's threshold always does. The mod hooks no `session.compact`.
- **The gate.** `scripts/check-plugins.sh` fails when a mod under `plugins/` hooks an event the policy plugin passes over.
