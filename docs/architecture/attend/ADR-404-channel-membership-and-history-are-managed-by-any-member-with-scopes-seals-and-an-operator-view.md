---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-172#Decision, 'ADR-172#Addendum, 2026-10-01: enrollment and one cold-start rule', ADR-503#Decision, 'ADR-504#Note (2026-10-02): a screen that is not a tree is a pane inside `App`', ADR-136#Decision, ADR-170#Decision]
basis:
  - operator: aaronsb
    level: directed
    said: "I would expect to be able to launch attend-chat myself, clean up the old rooms and delete the rooms and 'delete' their history (setting it to archived or removed, whatever state won't try to catch an agent up to date) and then establish a new channel for an intended topic with a name I choose, and then enroll agents to that channel."
    via: report relayed by the main session, 2026-10-10
  - operator: aaronsb
    level: directed
    said: agents have the same channel powers as the operator; agents can seal and unseal a channel and invite or remove members, and members include human operator sessions as well as agents
    via: relayed by the main session, 2026-10-10
    paraphrase: true
  - operator: aaronsb
    level: directed
    said: a channel's scope is a directory, this directory and everything under it; an agent joins only the deepest channel whose scope contains its project directory; members of a channel are effective members of every channel nested under it, not upward
    via: relayed by the main session, 2026-10-10, with the example of project-a-root and project-a-documentation
    paraphrase: true
  - operator: aaronsb
    level: directed
    said: a removed agent or human can simply rejoin; removal is not a ban
    via: relayed by the main session, 2026-10-10
    paraphrase: true
  - operator: aaronsb
    level: guided
    said: a membership cooldown keyed per member and channel stops remove and rejoin flapping; it applies to every channel and everyone; when it blocks an action in attend-chat the screen asks "cooldown <remaining> — override? y/N", y performs the action and the override is logged; the CLI has no override
    via: relayed by the main session, 2026-10-10, replacing an earlier exemption for attend-chat and a timing test
    paraphrase: true
  - operator: aaronsb
    level: directed
    said: "review of ADR-404: add enrolls directly and invite is a request, both for agents and humans; replay a message to a session only if it was a member when it was posted, marked by membership events in the channel's history, directed mail only to its addressee, with an attend history command for the rest; agents and operators both set scopes; no no_self_join switch; a channel's name and its path are synonyms; the top row is an active-channel switcher by recency; the override's friction is for an agent driving attend-chat, and it is recorded as an event"
    via: relayed by the main session, 2026-10-10
    paraphrase: true
  - operator: aaronsb
    level: directed
    said: lineage confirmed (clear, resume and compaction continue the agent; a fresh process in the same directory is a new agent); a new agent gets no replay, only a hint that N messages in a channel may be relevant because its predecessor was involved, carrying the session distance, labelled likely relevant or probably stale by thresholds in attend's settings
    via: relayed by the main session, 2026-10-10
    paraphrase: true
  - evidence: "/purge keeps any signal a live consumer has not seen (tools/attend-chat/src/app/keys.rs, purge_channel_in) and never reaches a project tray; the cold-start rule delivers addressed project-tray mail whatever its age, the newest 50 (tools/attend-state/src/cold_start.rs, plan); a reproduction in an isolated cache showed a new agent drained all 30 day-old addressed messages after #open and channel history were deleted"
  - evidence: "observed on attend 0.20.0 with the attend mod, 2026-10-12: a fresh Claude Code session (a new session id) started in ~/Projects/ai/harness/attend-collaborator with attend_mod.listening active, and before the operator typed anything its first turn was an idle delivery of 7 old messages addressed to [project], the previous session's conversation with Chaucer (test instructions, a channel invite, replies) that it had already read and answered. The files' mtimes were genuine (sent 22:02:35 to 22:03:31 on 10-11); the cold-start rule delivered project-tray mail whatever its age to a session with no seen-set (tools/attend-state/src/cold_start.rs, plan)"
  - operator: aaronsb
    level: directed
    said: "I think what's happening is that in 'attend-chat-test' channel, I can't see who's in the channel - I only see the historical scroll of myself and agents."
    via: relayed by the main session, 2026-10-12
  - evidence: "observed on attend 0.21.0, 2026-10-12: the operator posted in #attend-chat-test from attend-chat; the one Claude session in the channel ran attend reply to that message and got \"no live peers in channel 'attend-chat-test' (you are the only listener)\", and attend channels listed the channel with one member, the agent. attend-chat wrote the person as a member only on /join (tools/attend-chat/src/app/keys.rs, run_join); posting in a channel or showing it wrote nothing"
  - evidence: "/invite writes a directed signal and the invitee must run attend join itself (tools/attend-chat/src/app/keys.rs, run_invite: consent asymmetry, issue #393); /kick removes without consent"
  - precedent: ADR-136
  - precedent: ADR-170
  - precedent: ADR-172
  - precedent: ADR-173
  - precedent: ADR-503
  - precedent: ADR-504
  - precedent: ADR-124
  - precedent: ADR-401
  - operator: aaronsb
    level: guided
    said: 'Clear history is renamed Delete read messages for everyone…, with Delete older than: Current sessions / Today (local midnight) / All / Custom…; a message is deletable only when no live recipient has it unread; the rule lives in attend''s cleanup verb'
    via: Claude Code session 2026-10-13, recorded in the session handoff as the operator's direction
    paraphrase: true
  - operator: aaronsb
    level: guided
    said: '"we should add an option to clear dms" "on the side bar"'
    via: Claude Code session 2026-10-13
  - operator: aaronsb
    level: guided
    said: we probably need to be able to tombstone an agent's messages so they don't come back into chat again
    via: Claude Code session 2026-10-13
  - evidence: 'D3e as first written is not built: attend has no purge, history or unarchive verb and no _archive directory (tools/attend/src/cli.rs); the menu''s Clear history runs attend-chat''s /purge, which deletes a channel''s signals older than DEFAULT_GRACE and keeps any a live agent''s seen-set lacks (tools/attend-chat/src/app/keys.rs:800-829, tools/attend-chat/src/consumers.rs:1-12)'
  - evidence: attend message unread counts a session's unread messages with the drain's own scan, scan_pending(receive_dirs, seen, reading_ids, baselining) (tools/attend/src/verbs/unread.rs:56-69, tools/attend/src/cmd/inbox.rs:213-315); the scan marks the session's own messages and another session's tray mail without delivering them and applies the cold start while baselining
  - evidence: attend_presence::alive counts a member live by a running Claude process, a heartbeat within DEFAULT_GRACE (90 s) or a listening process still running (tools/attend-presence/src/lib.rs:36-40, tools/attend-presence/src/heartbeat.rs:37); reading_ids is the session id and the ids it had before each /clear (tools/attend-presence/src/enrollment.rs:417)
  - evidence: attend-chat depends on attend-config, attend-groups, attend-presence, attend-instances and attend-state, and has no crate dependency on attend (tools/attend-chat/Cargo.toml:16-32); attend message cleanup reaps by project liveness only (tools/attend/src/cmd/cleanup.rs:1-20)
  - precedent: ADR-413
  - operator: aaronsb
    level: guided
    said: maybe we use tombstones to mark, and then we have a seperate cleanup in days or agent generations
    via: Claude Code session 2026-10-13
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "lineage confirmed: clear, resume and compaction continue the agent; a fresh process in the same directory is a new agent; every decision reviewed with a disposition"
    via: relayed by the main session, 2026-10-10
    covers: [lineage]
  - operator: aaronsb
    said: "no shouting needed, that sounds right to me"
    via: Claude Code session 2026-10-13, reply to the delete-rule defaults
    covers: [tombstone]
status: accepted
date: 2026-10-10
deciders:
  - aaronsb
related:
  - ADR-120
  - ADR-124
  - ADR-401
  - ADR-136
  - ADR-170
  - ADR-172
  - ADR-173
  - ADR-503
  - ADR-504
  - ADR-405
  - ADR-413
---

# ADR-404: Channel membership and history are managed by any member, with scopes, seals and an operator view

## Summary

- **Decided:** ten decisions, each separable, so each can be accepted or rejected alone. The operator's review of 2026-10-10 is marked on each: agreed, or revised and rewritten here. D1 gives every member, agent or human, the same powers over a channel, with two verbs: `add` enrolls a member directly and `invite` asks one to join. Removal is not a ban. D2 adds a cooldown against flapping, on every channel and for everyone. D3 decides what a session is replayed: only what was posted while it was a member, marked by membership events written into the channel's history. D3e, amended 2026-10-13, lets any member mark the messages that no live recipient still has to read, through `attend message delete`, and tombstone a dead session; every reader skips what is marked, and `attend message cleanup` removes a marked message seven days after the mark, or in a project tray once that project has started three sessions since. D4 gives a channel a directory scope, set by agents and operators alike, that enrolls agents by where they work, with membership inherited downward. D5 adds sealed channels, which cut inheritance. D6 lets `attend join` and `attend leave` take a path, a channel's name and its path being two names for one channel. D7 renders the same model as text for agents. D8 lets attend-chat edit its own keys. D9 makes the top row an active-channel switcher ordered by recent activity, beside a channel tree. D10 is the override of the cooldown in attend-chat.
- **Trades away:** the consent asymmetry of issue #393 (only the invitee could add itself); the cold-start rule's time window (ADR-172's addendum), which D3 replaces with membership; ADR-136's rule that no message is removed by age, wherever a member asks for a delete; any way back for a marked message once cleanup has removed it. A tombstone withholds a dead session's messages from a live session that has not read them. Membership stops being a flat list: it becomes manual entries plus scopes plus inheritance, which every view must explain.
- **One-way?** A delete under D3e writes marks that `--undo` removes; the removal cleanup makes later is one-way, and the rule confines it to messages no live recipient still had to read when they were marked. The rest is reversible. A tombstone is an empty file that `--undo` removes. D3's membership events are new lines in the channel's history that an older attend reads as messages it ignores. Scopes, seals and exclusions are new state in `_groups.yaml` that an older attend ignores; dropping them later is a migration of that file, not of messages.
- **Probes:** *Confident (powers):* you want an agent you set to work on a project to be able to bring a colleague into the channel without you, as you can. *Not confident (lineage):* a Claude Code session that continues another one (after `/clear`, `--resume` or a compaction) is the same agent and keeps its channels and its place in them, while a new Claude Code process started in the same directory is a new agent. For D3e (2026-10-13): *Not confident (delete-verb):* when you said the rule lives in attend's cleanup verb, you meant it lives in attend's own command line, where attend-chat and agents both call it; it is a new `attend message delete`, and `attend message cleanup` keeps reaping the mail of deleted projects as it does today. *Confident (tombstone):* you tombstone a dead agent by hand, from its row; an agent that carries on after `/clear` is never tombstoned for you, and its old row folds into the live one.
- **Inversion:** the ends are a flat chat room, where membership is whatever each member last typed, and an access-controlled directory, where only an administrator changes anything. This sits in between: anyone may change membership, every change is a line in the channel's history, and the operator's screen can always see and undo. D3e sits between a delete that removes whatever is selected, unread mail included, and a room nobody can clear while any member is live; it removes what every live recipient has read and keeps the rest.

## Context

An operator cleaned up attend's rooms from attend-chat and found that a new agent was still caught up on old messages. The causes, in this codebase as of this record:

- A session that joined a channel after its first scan was handed the channel's whole backlog: the cold-start rule ran only on the first scan. Fixed in the same change as this record (below).
- Addressed mail in a project's tray is delivered to a new session whatever its age, the newest 50 (ADR-172, addendum of 2026-10-01). A tray is per project, so mail written to one session reaches every later session in that directory. Fixed for the tray since (below). Nothing the operator can run clears a tray: `/purge` reaches `#open` and channels only, and a tray lives as long as its project (ADR-136 Decision 3).
- `/purge` keeps every message a live consumer has not seen (ADR-172 Decision 5). An idle but live member holds the history open.
- `attend inbox` lists every message in the receive set, and the cold-start note points new agents at it.

The operator also wants to create a channel for a topic and put agents in it. Today `/invite` sends a request the agent must act on itself, a rule from issue #393 that no accepted record states. And the operator's model of a channel is a part of the file tree: one per project, one per subdirectory, agents placed by where they work.

Agents join and leave channels on their own through the CLI (`attend join`, `attend leave`), and that stays. The TUI is the operator's view; everything it can change, an agent can change through a CLI verb.

## Decision

### D1. Every member has the same channel powers, with two ways in

*Operator: revised 2026-10-10 (two verbs).*

Any member of a channel, agent or human, may add, invite or remove a member, and seal or unseal the channel (D5). Members are agent sessions and human operator sessions alike (ADR-170).

A person in attend-chat becomes a member of a channel by posting in it or by joining it, so a person who speaks in a channel is its member. Showing a channel joins nothing: a tab bar is walked, cycled and advanced past channels nobody chose, and a Leave holds until the person posts in the channel or joins it again. A member is a listener while it is live: an agent session by its process or its heartbeat, a person while their attend-chat is open and keeps their heartbeat fresh (ADR-170). A send or reply to a channel whose only other live member is a person goes out; one to a channel with no other live member is refused. A post does not make a channel that does not exist or bring back one whose directory was swept; the send is refused, as before.

- **`attend add <member> [<channel>]`** enrolls the member directly, with no answer from it. The added member's history position is the add event (D3), so it is replayed what follows; the add's briefing is the same as an invitation's, below.
- **`attend invite <member> [<channel>]`** is a request: the invitee joins itself, or not. A join that answers a pending invitation brings the invitation's briefing (D3).
- **`attend kick <member> [<channel>]`** removes; `attend seal <channel>` and `attend unseal <channel>` seal and unseal. attend-chat's `/invite`, `/kick` and the channel menu's Add agent, Invite agent and Remove agent run the same paths.
- **Removal is not a ban.** Removing a member stops delivery to it and records an exclusion for that member and channel. The exclusion blocks only automatic re-enrollment (D4 scopes, inheritance). An explicit `attend join` by the member always succeeds, clears the exclusion, and is recorded like any other change. There is no ban state, and nothing refuses a join.
- **A self-leave records an exclusion too**, so a leave from a scope- or inherited-membership channel is not undone by the next evaluation.
- **Every change is recorded and announced.** Each add, invite, accepted invite, join, leave, kick, seal and unseal is a membership event in the channel's history (D3), naming the actor; the chat shows it as a system line, such as `@kg-agent sealed #project-a-docs` or `@aaron added @wells`.
- **Operator access cannot be lost.** An agent may remove an operator from a channel, which stops delivery to that operator. The operator screen still lists every channel, and the operator can rejoin or reverse any change from it.

### D2. A membership cooldown per member and channel

*Operator: agreed 2026-10-10.*

After any membership change for a (member, channel) pair (an add, an invite, a removal, a self-join, a self-leave), a further change for that pair is refused until the cooldown has passed. The refusal says how long remains and is recorded. Sealing and unsealing take the same cooldown, keyed per channel. The duration is `attend.channels.membership_cooldown`, in seconds, default 60, registered in attend's schema (ADR-503 Decision 13).

The cooldown is universal: every channel, `#open` included, with no per-channel switch and no exempt actor. The one way past it is D10's override in attend-chat, for one action at a time. The CLI has no override.

### D3. A session is replayed what it was a member for

*Operator: revised 2026-10-10 (rewritten around membership events). D3d: Operator: agreed 2026-10-10, with session distance. D3e: revised 2026-10-13 (mark what is read, then a separate cleanup; tombstones).*

**D3a. Membership events in the history.** Every add, invite, accepted invite, join, leave and kick writes a membership event into the channel's history, beside its messages: the member, its session id, the time, and the actor. `#open` takes an implicit join when a session enrolls (ADR-172's addendum).

**D3b. Replay follows membership.** A message is replayed to session S only if S was a member when the message was posted: after S's join event, with no leave or kick of S between. Directed mail is replayed only to the session it was addressed to, by ADR-401's addressee field, not to every later session in the project. This replaces the cold-start rule's time window (ADR-172, addendum of 2026-10-01) for both conduits; the seen-set still keeps delivery to once.

A session that a message fails the rule for is not replayed it. A new agent (D3d) is replayed nothing from before its first join; what it may need is offered as D3f's hint, one line per channel, and otherwise as `N messages in history — attend history <channel> to read`. `attend history <channel>` is a new read-only command that lists the channel's history, membership events included, and marks nothing seen.

**D3c. The briefing of an add or an invitation.** A member brought in by someone else needs the conversation it is brought into, and its own join event postdates that conversation. So an add or an invitation carries a replay-from point, recorded in its membership event: by default 60 minutes before the add or the invitation, capped at the newest 50 messages, the cap ADR-172 gives addressed mail. The adder or inviter may set it: `attend add|invite <member> [<channel>] --since <duration|message-id>`. When the invitee's join answers the invitation, or when the add lands, the member's replay position starts at that point rather than at its join. A self-join with no invitation starts at the join.

*Why this point, and not another:* the alternative anchors were the inviter's own last membership change, which is arbitrary (a long-time member would brief everything, one who just joined nothing), and the invitee's join, which briefs nothing. The person who brings a member in decides what it needs; a default window before that moment covers the conversation that prompted it, and the cap bounds a busy channel.

**D3d. Session lineage.** A session that continues another (after `/clear`, `--resume` or a compaction) is the same agent: it inherits the earlier session's memberships and replay positions. The link is made at `SessionStart` from the hook's source, as the task-list carry-forward links them (`gh-tasks attach`): `compact` keeps the session id; `clear` gives the same Claude Code process a new id, which attend already follows by the process key (`attend_presence::enrollment::previous_id`); `resume` is recorded under the session it resumes. A new Claude Code process in the same directory is a new agent, with no membership and no replay position.

**D3f. A new agent gets a hint, with the session distance.** A new agent is replayed nothing. For each channel its predecessor took part in, it gets one line saying how many messages there may be relevant and how far back that predecessor was:

- The **predecessor** is the most recent earlier session in the same project directory that was a member of the channel, or sent or received in it.
- The **distance** is the number of Claude Code sessions started in that directory since the predecessor started, a lineage (D3d) counting as one session.
- The line reads, for example, `14 messages in #kg-sync from a session 1 back — likely relevant · attend history #kg-sync`; past the stale threshold, `… from a session 10 back — probably stale`; in between, neutral wording with no label. The thresholds are attend settings: `attend.channels.hint_relevant_within` (default 2: a distance of 2 or less reads "likely relevant") and `attend.channels.hint_stale_from` (default 6: a distance of 6 or more reads "probably stale").
- The session list comes from Claude Code's per-project transcripts under `~/.claude/projects/<slug>/`, read through the claude-sessions crate. Today that crate lists a project's transcripts (`claude_sessions::transcripts_in`, `newest_transcript`) and live session records (`read_session_records`: pid, session id, cwd), but it exposes no start time per session. The follow-up adds one there, from each transcript's first timestamped line, so the start order is read in one place.

**D3e. A delete marks what no live recipient still has to read, a tombstone keeps a dead session's messages out, and cleanup removes what is marked.** With D3b a new session is no longer replayed old history, and a member may still want a room's messages gone. The rule and its verbs live in attend's CLI, under `attend message` (ADR-413), and every surface calls them.

- **The rule.** A message is deletable when all four hold: it is in the requested scope; it is older than the requested cutoff; it is older than `DEFAULT_GRACE` (90 seconds, `attend_presence::heartbeat`), the floor that spares a file a peer may be scanning; and no live recipient has it pending. No flag turns the rule off.
- **Live recipients and what they hold.** A live recipient is an agent session that may still read the bus: an enrolled session until `attend_presence::has_exited` says its process has gone, a Claude Code session whose recorded process runs on this host whether or not it listens, and an agent whose heartbeat is fresh. A session this host cannot judge, one on another host or under a host name this one no longer has, holds its mail while its last heartbeat is under `MARK_REAP_DAYS` (7) days old, judged by heartbeat as ADR-406 item 12 judges another host's session; with no session record here and no heartbeat in that window it counts as gone, for a delete and for the tombstone's refusal alike. A running session that does not listen holds its mail too, whose cold start would deliver its mail once it listens. What it holds is what `attend message unread` counts for it: the drain's own scan, `scan_pending(receive_dirs, seen, reading_ids, baselining)`. Addressing (ADR-401's `to`), its own messages, the ids it had before `/clear` and its cold start all apply through that one function, and a delete keeps every message the session would be delivered. A person in attend-chat keeps no seen-set and holds nothing.
- **A delete marks.** A delete writes one mark per deletable message and removes no file. Marks and tombstones share one store, `signals/_tombstones/`: a message mark is a file at `messages/<key>`, the key being the message's seen-set key (its filename), with the mark time as its mtime and the batch id as its content, written whole and linked into place, and each delete records its batch at `batches/<batch-id>`, one key per line. A message already marked keeps its first mark. Once cleanup has removed every message a batch marked, its record stays for 30 days as a stub that says when. Membership events (D3a) are never marked. There is no `_archive/`, no `attend history --archived` and no `attend unarchive`.
- **Cleanup removes what is marked.** `attend message cleanup`, and the sweep `attend run` makes, removes a marked message's file and its mark once the mark is older than 7 days, or, for a message in a project tray, once that project has started 3 Claude Code sessions since the mark, each `/clear` counting as one. A session start is a transcript Claude Code wrote under `~/.claude/projects/<slug>/` after the mark, one per session id. `#open` and channel messages go by the days alone. Both limits are named constants in attend. A message no delete marked is never removed this way, whoever sent it and whether or not that session has exited.
- **`attend message delete [--open | --channel <c> | --dm <session-id> | --everywhere] [--from <session-id>] [--as-person] [--older-than current|today|all|<dur>|<YYYY-MM-DD>] [--dry-run] [--json]` and `attend message delete --undo [<batch-id>]`.** One scope is required: `#open`, a channel, the direct-message thread between the caller and a session, or every room. `--dm` reaches only the messages that session sent the caller and those the caller sent it, in both directions, and never that session's direct messages with anyone else; the caller is the person in attend-chat, or an agent by its `reading_ids`. `--as-person` makes the caller the person at the terminal with no walk up the process tree to a session, and attend-chat always passes it with `--dm`, since it may run under a session's shell. It is attend-chat's convenience, not an authorization check: anyone at the terminal may pass it, and it chooses whose thread `--dm` names, never what the caller may delete. The flag is hidden from the help and refused without `--dm` (amendment of 2026-10-14). `--from` narrows the scope to messages that session sent. `current` is older than the earliest start of any live session; `today` is before local midnight; a duration or a local date is the cutoff it names; `all`, the default, leaves the cutoff to the rule. The report counts what was marked, names the batch and, for each live recipient that held a message, counts what it kept. `--dry-run` reports the same and marks nothing. `--undo` removes the marks of the newest batch, or of the batch named, and the messages cleanup has not yet removed are delivered again: a reader passes over a marked message without recording it read. The undo reports the delete's age and how many of its messages cleanup had already removed. On a stub it says cleanup removed that delete's messages and on what date, and a bare `--undo` whose newest record is a stub refuses rather than undo an older delete. `--dm` takes a message only when its `to` names the caller's ids alone or the other session alone; one sent to a third party as well is outside the thread.
- **`attend message cleanup` stays ADR-136's project-liveness reaper**, on its own sweep, and removes marked messages under the two limits above.
- **What this amends.** An explicit delete overrides ADR-136 Decision 3's rule that nothing is removed by age, for the messages it marks. ADR-170's `/purge` moves into attend under this rule. ADR-172 Decision 5's property holds for a delete: it never marks a message a live session would be delivered.

**Tombstones.**

- **`attend message tombstone <session-id> [--undo] | --list`.** A tombstone is an empty file at `signals/_tombstones/<session-id>`, and it hides what the session sent up to the tombstone's mtime; what an id sends later, back under `--resume`, shows. The verb tombstones only an agent's session id, never a person's member id, and only one that has gone: `attend_presence::has_exited` confirms it, or it is a session this host cannot judge whose heartbeat is past the window above. Tombstoning a session again moves the tombstone, and its cut, to now. It refuses an id among the live recipients above or that one of them reads as (its ids before `/clear`), the caller's own included. It writes the mark and runs no delete: the session's messages stay on disk, skipped by every reader, until a delete marks them or ADR-136's project sweep reaps them. `--undo` removes the mark, and what a drain marked read while it stood stays read. `--list` lists the marks.
- **Every reader honours the marks.** The drain delivers none of them, which covers the attend mod and `attend message unread`, since both go through `scan_pending`. It passes over a marked message without recording it read, so an undo restores its delivery, and records a tombstoned session's messages read, so they stay read after the tombstone's `--undo`. `sensor-peers` does the same, `attend message inbox` leaves them out of its listing, and reading one by its id says it was deleted or tombstoned and names the `--undo` that restores it. attend-chat's watcher drops them at backfill and as they arrive, watches the mark store, evicts from its buffer, and so from every view, what a new mark hides, and reads again what an `--undo` restores, in its place by time. The left column seeds no agent row from them, and a tombstoned session that has exited makes no row.
- **No automatic tombstone on `/clear`.** The session that continues reads its earlier ids' mail through `reading_ids`, and a tombstone on an earlier id would hide that mail from it. attend-chat folds a dead sighting whose id is among a live session's earlier ids into that session's row, by the ids the enrollment and the instance registry carry and not by name.

**In attend-chat** (the menus are ADR-405's): Clear history becomes Delete read messages for everyone…, with Delete older than: Current sessions / Today (local midnight) / All / Custom…, and calls `attend message delete`. Clear view becomes Hide messages here, a per-tab watermark local to attend-chat that persists across restarts; nothing on the bus changes. Tombstone… is offered on a dead agent's row. attend-chat has no crate dependency on attend: it runs the attend CLI as a child process and reports when the installed attend lacks the verb. `/purge` stays for one release as an alias of the shown channel's delete with `--older-than all`.

### D4. A channel's scope is a directory, and enrollment follows it

*Operator: revised 2026-10-10 (agents set scopes too).*

A channel may carry one scope: a directory, meaning that directory and everything under it. The scope is stored canonical (tilde expanded, symlinks resolved, no trailing slash) in the channel's `_groups.yaml` entry. Scopes are not globs, so whether one channel nests under another is path-prefix containment, always decidable.

- **Who sets it.** Any member, agent or operator, may set or change a channel's scope (`attend scope <channel> <path>`), under D2's cooldown per channel, recorded as a membership event.
- **Auto-enrollment.** When an agent session starts or joins the bus, and when a scope is added or changed for sessions already live, the agent joins the deepest channel whose scope contains its project directory (ADR-171's origin path), and no other by scope.
- **Inheritance downward.** The members of a channel, whatever their origin, are effective members of every channel whose scope nests under it. Not upward: an agent in a docs channel does not receive the root channel's traffic. A channel may switch off delivery of its descendants' traffic to its inherited members (`inherit_descendants: false`), since the volume grows with depth; the default is on.
- **Two channels with the same scope are refused.** Setting a scope that another channel already holds fails and names that channel. Treating them as one depth would enroll every agent there in both, two names for one room.
- **Exclusions apply to effective membership.** A removal or self-leave (D1) excludes the member from the channel whether its membership came from a scope, from inheritance or from a join.
- **Origin is shown.** Every view of membership names its origin: `manual`, `scope`, or `inherited-from-<channel>`.

### D5. A sealed channel cuts inheritance

*Operator: revised 2026-10-10 (no `no_self_join` switch).*

Any member may seal or unseal a channel (D1). A sealed channel does not inherit members from channels whose scopes contain it. The cut is at the seal: channels nested under a sealed channel inherit from the sealed channel's effective members, never from above it, so an ancestor cannot reach into a sealed subtree through a nested channel.

A seal blocks inheritance, not a join. Anyone may `attend join` a sealed channel, and it shows as `manual`. "Sealed" is the term, since `scene private` already means leaving every channel.

### D6. A channel's name and its path are two names for it

*Operator: agreed 2026-10-10, with the synonym rule added.*

`attend join` and `attend leave` take a channel's name or a directory. An argument starting with `/`, `~` or `.` is a path; channel names cannot contain `/` (`attend_groups::validate_group_name` admits letters, digits and `-`). A path is canonicalized and resolves to the channel whose scope equals it, else to the deepest channel whose scope contains it, the D4 rule. No covering channel is an error that lists the nearest channels. A path never creates a channel; a join by name creates one as today. A leave by path resolves the same way.

A channel's name and its scope path are synonyms for one channel. A channel always displays its name, however it was joined.

### D7. The agents' text views carry the same model

*Operator: agreed 2026-10-10.*

`attend channels` renders the channel tree by scope nesting as indented text: each channel with its scope, member count, sealed marker, and for each member its origin. `attend peers` shows each agent's effective channels. Both are compact, one line per entry, and greppable: an agent reads the same model the operator sees. Both name a person among the members as `<username> (attend-chat)`, beside an agent's `<name> (<project>)`, so an agent can see that a person is in the channel.

### D8. attend-chat edits its own keys, and the theme

*Operator: agreed 2026-10-10.*

The chat's keys are attend's, under `attend.chat.*`, in attend's user file: `tabs.jump`, `tabs.menu_on_repeat`, `tabs.focus_key`, `mouse`, and `sidebar_key` for D9. `attend.channels.membership_cooldown` serves D2; `attend.channels.hint_relevant_within` and `attend.channels.hint_stale_from` serve D3f. attend registers them in its own schema, as ADR-503 Decision 13 provides.

That decision also says attend's settings "are edited through `ways settings`" and that "`attend` has no settings UI of its own". attend-chat is a separate application (ADR-504 Decision 1), but the keys are attend's, and its `/config` and the common menu's Keybinding set and Mouse items are a second way to edit them. This amends ADR-503 Decision 13: attend-chat may list and set its own `attend.chat.*` keys, through attend-config's writer and the schema `ways settings` reads, so the two stay one definition and one file. `ways settings` still edits every key; `attend` itself still has no settings UI.

On the same terms attend-chat may write the one theme choice, ways' `theme.active`, through the settings writer and the declaration `agent_theme::settings` holds, as ADR-504's note of 2026-10-01 says the theme tab does. Until then the common menu's Theme item changes the look for the session only.

### D9. The top row switches between active channels

*Operator: revised 2026-10-10 (an active-channel switcher by recency).*

- The top row is the active-channel switcher: the common menu (`≡`) and merged fixed at the left, `#open` pinned next (ADR-124 Decision 3), then the channels in use ordered by the time of their newest message, newest first, and the `+` slot at the right. Ctrl+N and Alt+N jump to its numbered slots; action slots carry no number, so merged is 1 and `#open` 2.
- A collapsible left sidebar shows every channel as a tree by scope nesting, expanding and collapsing as a directory browser does, with compact per-row figures: members, unread, time since the last message, and the sealed marker, for example `▸ project-a-root  3👤 12✉ 4m`. It navigates every channel, including those the top row has no slot for. Enter or a click on a row opens the same context menu as the tab. `attend.chat.sidebar_key` toggles it.
- The channel menu gains Seal or Unseal and Scope… (D4).
- The tabs of a pane with menus are reached by Ctrl+1-9 where the terminal reports Ctrl+digits (again on the shown tab: its menu), by F2 or Ctrl+T on the tab bar, and by a second click or a right click; Alt+1-9 stays where it arrives. This amends ADR-504's note of 2026-10-02 on panes, which names Alt+1-9 as the tab key beside text: Konsole and GNOME Terminal keep Alt plus a digit for their own tabs.

### D10. attend-chat may override the cooldown, after asking

*Operator: revised 2026-10-10 (what the friction is for; the override is an event).*

When the cooldown blocks an action in attend-chat, the bottom bar asks `cooldown <remaining> — override? y/N`. `y` performs the action; any other key cancels it. The CLI has no override, and a headless `attend-chat --snap` is a dry run that changes nothing.

The question exists to slow an agent that drives attend-chat's screen directly rather than through the CLI: it costs a person one key and makes a scripted flap stop at every step. It is a pause, not a lock. An override is recorded in the channel's history as a membership event naming its actor (D3a); the chat view need not show it.

## Consequences

### Positive

- A new agent is replayed only what it was a member for, so cleaning up stops being the way to keep it from being caught up; addressed mail stops leaking to later sessions in the same directory.
- An invited or added member is briefed on the conversation it was brought into, by a point the person who brought it chose.
- Agents organize themselves: one can bring a colleague into a channel, scope or seal a subtree without asking the operator, and every such change is in the channel's history.
- Channels map onto projects and their parts, so an agent lands in the right room by starting work in the right directory.
- One model, shown three ways: the operator's tree, the tab menus, and the agents' `attend channels` and `attend history`.
- A room can be cleared while its members are live: a delete takes what every live recipient has read and leaves an idle agent's mail in place, by the scan `attend message unread` already reports.
- A dead session's messages stop reappearing in the chat and in deliveries once it is tombstoned.

### Negative

- Replay becomes a computation over membership events, scopes, inheritance and lineage; a bug there misroutes or withholds messages. It needs its own test table, and every view must show origin.
- Lineage at `SessionStart` depends on the hook's source and on Claude Code's session records; a continuation the hook cannot link is treated as a new agent, which is replayed nothing from before and gets only D3f's hint.
- The session distance counts sessions, not time or relatedness: a short throwaway session in the directory pushes a real predecessor one step further back.
- A marked message cannot be brought back once cleanup removes it, 7 days or 3 project sessions after the mark. A tombstone hides from a live session the messages a dead session sent it, read or unread. The pending rule and the refusal of live ids bound both.
- A delete computes every live recipient's pending set, one drain scan per session; a busy host with many live sessions makes a delete slower than a file sweep.
- Symmetric powers let a confused agent remove members, rescope or seal a channel; the cooldown, the history and the operator's view bound the damage but do not prevent it.

### Neutral

- `_groups.yaml` gains per-channel `scope`, `sealed`, `inherit_descendants` and per-member `exclusions`; channel histories gain membership events. The parser in attend-groups owns both, and an older attend ignores them.
- The consent asymmetry of issue #393 ends for `add`; `invite` keeps it.
- `signals/_tombstones/` is a new directory. An older attend or attend-chat ignores it and still shows marked messages and a tombstoned session's messages until they leave the disk.

## Implemented with this record

### Under the accepted decisions

These fit ADR-124, ADR-136, ADR-170 and ADR-172 as accepted and need none of the above:

- A self-join (CLI or scene) marks the channel's messages older than the cold-start window seen before writing the membership, and says how many it held back: ADR-136 Decision 2's "keeps a fresh join from dumping history". D3b replaces it with the membership rule.
- `/dissolve` from attend-chat no longer counts the operator's own heartbeat as a live member (ADR-170's guard protects peers working in the channel).
- The top row orders channels by their newest message, `#open` pinned first: the "recent-activity band" ADR-124 left for later, with its pinned `#open` kept.
- D1, a person's membership: attend-chat writes the person as a member of a channel when they send to it, before the message is written, as `/join` does, touching the heartbeat first so the membership is live at once. Showing a channel writes nothing. A chat with no heartbeat, `--snap` or a dry run, joins nothing, and a chat that closes cleanly clears the person's heartbeat rather than leaving it fresh for its grace window. `attend channels` and `attend peers` mark the person `(attend-chat)` (D7). Liveness is ADR-170's heartbeat, unchanged.

### Ahead of acceptance

These shipped with the record and stand on the decisions named; rejecting a decision takes its item out:

- D1 and D3c, interim: attend-chat's `/invite` and Invite agent record a pending invitation beside the room's signals, and the invitee's `attend join` spends it and keeps the room's newest 50 messages deliverable, whatever their age, running a cold session's cold start at once so its first scan does not baseline them away. D3c's replay-from point (60 minutes before the invitation, or `--since`) replaces the age-blind newest 50 when D3 is built. Add agent is a stub that names D1: enrolling a session that has not started attend needs its cold start run for it, and that scan lives in the attend binary, out of attend-chat's reach. Clear history still runs today's `/purge`, which keeps messages younger than 90 seconds and any a live agent has not read.
- D3b, interim, until membership events and ADR-401's `to` field exist: the cold start reads a project-tray message as addressed to the session when it was sent after the session's current Claude Code process started (its session record's `startedAt`), since `--to` addresses the sessions live at the path. It reads an `#open` or channel message as said to the session when it was posted after the session last started listening, a time the enrollment record keeps (the implicit `#open` join of D3a). Older messages of either kind are backlog, held back with the count note, whether or not the session they were for read them; a new session is replayed none of its predecessor's. Each kind is capped at the newest 50, and anything younger than 120 seconds is delivered. A record with no time leaves that kind to the 120-second window. In the attend mod, a session its config starts listening submits no turn before the operator's first prompt: what is pending rides that prompt as context with the held-back note, a toast says so, a reload keeps the hold, and a prompt that does not enter leaves the batch for the poll.
- D9, the tab keys and menus: Ctrl+1-9 under the kitty keyboard protocol (Esc on an empty line asks before quitting where Ctrl+3 arrives as Esc), F2 and Ctrl+T, menus on a second click or a right click, the `+` slot, and the `≡` common menu.
- D8, the `attend.chat.*` keys with `/config`, the Keybinding set presets and the mouse item. The theme item holds for the session only.

## Open questions

- Whether `attend inbox` should list only what D3b would replay, leaving the rest to `attend history`.

## Alternatives Considered

- **Keep invitations as requests only (issue #393).** A request an idle agent never acts on leaves the channel empty. D1 keeps `invite` and adds `add` beside it.
- **Replay by age (the cold-start window).** Old messages are held back from a new session, but a member that was away for an hour is treated like a stranger, and addressed mail to a project reaches every later session there. Membership answers "was this said to you" directly.
- **Brief an invitee from the inviter's last membership change, or from its own join.** The first is arbitrary, the second briefs nothing (D3c).
- **A clear that archives, whether or not a live session has read the message (D3e as first written).** It takes from an idle live agent mail addressed to it, and the archive is a second store every reader must skip and something must empty. With D3b and the cold start, a new agent is no longer caught up on old history, the problem the unconditional clear was for. Not built, and replaced on 2026-10-13.
- **Remove the files at delete time (D3e as amended first on 2026-10-13).** A delete could not be undone, and every surface that deletes would remove files itself. A mark lets a delete be taken back until cleanup runs, and one sweep owns removal, by age or by sessions in the project.
- **Consult each live consumer's seen-set (today's `/purge`, ADR-172 Decision 5).** A seen-set records what a session has read. What it would be delivered also turns on addressing, its own posts, its earlier ids and its cold start, which the drain's scan already applies. Asking the scan answers the question once, in the place `attend message unread` reads.
- **Put the rule in `attend message cleanup`.** That verb is the project-liveness sweep `attend run` also calls. A delete a member asks for, with a scope and a cutoff, is a separate act, and one verb for both would put the sweep and the request on one set of defaults.
- **Delete from attend-chat directly.** attend-chat cannot compute an agent's pending set without the drain's scan, which lives in the attend binary, and agents would need a second path for the same act.
- **Tombstone automatically on `/clear`.** It would hide from the continuing session the mail `reading_ids` gives it.
- **Glob rules for auto-membership.** Globs can overlap without nesting, so "the deepest channel" is undefined for them. Directory scopes make nesting a prefix test.
- **Same-scope channels as one depth.** Enrolls every agent there in two rooms with one meaning; refusing keeps one name per place.
- **Operator-only scope, seal and membership.** Simpler to reason about, but agents would route every reorganization through the operator, the bottleneck this record removes.
- **A `no_self_join` switch on sealed channels.** Dropped: a seal is about inheritance, and refusing joins brings back a ban by another name.
- **No override at all.** Simplest, and a flap could not be forced; but an operator correcting their own mistake would wait out the cooldown with nothing to do.
- **Exempting the operator, by a CLI flag or by attend-chat's identity.** A flag any agent could pass, and an exemption that asks nothing makes the cooldown invisible to whoever holds it. Asking each time keeps it visible and recorded.
- **A timing or hold-to-confirm test before the override.** Considered and dropped as more ceremony than the risk needs: anything that drives the screen could pass it too.
