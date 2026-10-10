---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-172#Decision, 'ADR-172#Addendum, 2026-10-01: enrollment and one cold-start rule', ADR-503#Decision, 'ADR-504#Note (2026-10-02): a screen that is not a tree is a pane inside `App`']
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
  - evidence: "/purge keeps any signal a live consumer has not seen (tools/attend-chat/src/app/keys.rs, purge_channel_in) and never reaches a project tray; the cold-start rule delivers addressed project-tray mail whatever its age, the newest 50 (tools/attend-state/src/cold_start.rs, plan); a reproduction in an isolated cache showed a new agent drained all 30 day-old addressed messages after #open and channel history were deleted"
  - evidence: "/invite writes a directed signal and the invitee must run attend join itself (tools/attend-chat/src/app/keys.rs, run_invite: consent asymmetry, issue #393); /kick removes without consent"
  - precedent: ADR-136
  - precedent: ADR-170
  - precedent: ADR-172
  - precedent: ADR-173
  - precedent: ADR-503
  - precedent: ADR-504
  - precedent: ADR-124
  - precedent: ADR-401
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
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
---

# ADR-404: Channel membership and history are managed by any member, with scopes, seals and an operator view

## Summary

- **Decided:** ten decisions, each separable, so each can be accepted or rejected alone. The operator's review of 2026-10-10 is marked on each: agreed, or revised and rewritten here. D1 gives every member, agent or human, the same powers over a channel, with two verbs: `add` enrolls a member directly and `invite` asks one to join. Removal is not a ban. D2 adds a cooldown against flapping, on every channel and for everyone. D3 decides what a session is replayed: only what was posted while it was a member, marked by membership events written into the channel's history; an explicit clear that archives history stays beside it. D4 gives a channel a directory scope, set by agents and operators alike, that enrolls agents by where they work, with membership inherited downward. D5 adds sealed channels, which cut inheritance. D6 lets `attend join` and `attend leave` take a path, a channel's name and its path being two names for one channel. D7 renders the same model as text for agents. D8 lets attend-chat edit its own keys. D9 makes the top row an active-channel switcher ordered by recent activity, beside a channel tree. D10 is the override of the cooldown in attend-chat.
- **Trades away:** the consent asymmetry of issue #393 (only the invitee could add itself); the cold-start rule's time window (ADR-172's addendum), which D3 replaces with membership; ADR-172 Decision 5's guarantee that no message a live session has not consumed is ever removed, where an explicit clear says otherwise. Membership stops being a flat list: it becomes manual entries plus scopes plus inheritance, which every view must explain.
- **One-way?** No. D3's archive can be undone, and its membership events are new lines in the channel's history that an older attend reads as messages it ignores. Scopes, seals and exclusions are new state in `_groups.yaml` that an older attend ignores; dropping them later is a migration of that file, not of messages.
- **Probes:** *Confident (powers):* you want an agent you set to work on a project to be able to bring a colleague into the channel without you, as you can. *Not confident (lineage):* a Claude Code session that continues another one (after `/clear`, `--resume` or a compaction) is the same agent and keeps its channels and its place in them, while a new Claude Code process started in the same directory is a new agent.
- **Inversion:** the ends are a flat chat room, where membership is whatever each member last typed, and an access-controlled directory, where only an administrator changes anything. This sits in between: anyone may change membership, every change is a line in the channel's history, and the operator's screen can always see and undo.

## Context

An operator cleaned up attend's rooms from attend-chat and found that a new agent was still caught up on old messages. The causes, in this codebase as of this record:

- A session that joined a channel after its first scan was handed the channel's whole backlog: the cold-start rule ran only on the first scan. Fixed in the same change as this record (below).
- Addressed mail in a project's tray is delivered to a new session whatever its age, the newest 50 (ADR-172, addendum of 2026-10-01). A tray is per project, so mail written to one session reaches every later session in that directory. Nothing the operator can run clears a tray: `/purge` reaches `#open` and channels only, and a tray lives as long as its project (ADR-136 Decision 3).
- `/purge` keeps every message a live consumer has not seen (ADR-172 Decision 5). An idle but live member holds the history open.
- `attend inbox` lists every message in the receive set, and the cold-start note points new agents at it.

The operator also wants to create a channel for a topic and put agents in it. Today `/invite` sends a request the agent must act on itself, a rule from issue #393 that no accepted record states. And the operator's model of a channel is a part of the file tree: one per project, one per subdirectory, agents placed by where they work.

Agents join and leave channels on their own through the CLI (`attend join`, `attend leave`), and that stays. The TUI is the operator's view; everything it can change, an agent can change through a CLI verb.

## Decision

### D1. Every member has the same channel powers, with two ways in

*Operator: revised 2026-10-10 (two verbs).*

Any member of a channel, agent or human, may add, invite or remove a member, and seal or unseal the channel (D5). Members are agent sessions and human operator sessions alike (ADR-170).

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

*Operator: revised 2026-10-10 (rewritten around membership events). The lineage rule is proposed by the main session and awaits the operator's confirmation.*

**D3a. Membership events in the history.** Every add, invite, accepted invite, join, leave and kick writes a membership event into the channel's history, beside its messages: the member, its session id, the time, and the actor. `#open` takes an implicit join when a session enrolls (ADR-172's addendum).

**D3b. Replay follows membership.** A message is replayed to session S only if S was a member when the message was posted: after S's join event, with no leave or kick of S between. Directed mail is replayed only to the session it was addressed to, by ADR-401's addressee field, not to every later session in the project. This replaces the cold-start rule's time window (ADR-172, addendum of 2026-10-01) for both conduits; the seen-set still keeps delivery to once.

A session that a message fails the rule for is not replayed it. It gets one line per channel instead: `N messages in history — attend history <channel> to read`. `attend history <channel>` is a new read-only command that lists the channel's history, membership events included, and marks nothing seen.

**D3c. The briefing of an add or an invitation.** A member brought in by someone else needs the conversation it is brought into, and its own join event postdates that conversation. So an add or an invitation carries a replay-from point, recorded in its membership event: by default 60 minutes before the add or the invitation, capped at the newest 50 messages, the cap ADR-172 gives addressed mail. The adder or inviter may set it: `attend add|invite <member> [<channel>] --since <duration|message-id>`. When the invitee's join answers the invitation, or when the add lands, the member's replay position starts at that point rather than at its join. A self-join with no invitation starts at the join.

*Why this point, and not another:* the alternative anchors were the inviter's own last membership change, which is arbitrary (a long-time member would brief everything, one who just joined nothing), and the invitee's join, which briefs nothing. The person who brings a member in decides what it needs; a default window before that moment covers the conversation that prompted it, and the cap bounds a busy channel.

**D3d. Session lineage.** A session that continues another (after `/clear`, `--resume` or a compaction) is the same agent: it inherits the earlier session's memberships and replay positions. The link is made at `SessionStart` from the hook's source, as the task-list carry-forward links them (`gh-tasks attach`): `compact` keeps the session id; `clear` gives the same Claude Code process a new id, which attend already follows by the process key (`attend_presence::enrollment::previous_id`); `resume` is recorded under the session it resumes. A new Claude Code process in the same directory is a new agent, with no membership and no replay position.

**D3e. An explicit clear archives history.** Kept from the first draft as its own sub-decision: with D3b a new session is no longer replayed old history, but an operator may still want a room's history gone. `/purge`, the menu's Clear history and a new `attend purge [<channel>]` move every message older than the 90-second grace window into an archive outside every receive directory (`_archive/<room>/`), whether or not a live session has consumed it, which amends ADR-172 Decision 5 for an explicit clear only. A project tray takes the same clear (`/purge @name`, `attend purge --tray <path>`). `attend history --archived` lists the archive; `attend unarchive <channel>` moves it back. Membership events are not archived.

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

`attend channels` renders the channel tree by scope nesting as indented text: each channel with its scope, member count, sealed marker, and for each member its origin. `attend peers` shows each agent's effective channels. Both are compact, one line per entry, and greppable: an agent reads the same model the operator sees.

### D8. attend-chat edits its own keys, and the theme

*Operator: agreed 2026-10-10.*

The chat's keys are attend's, under `attend.chat.*`, in attend's user file: `tabs.jump`, `tabs.menu_on_repeat`, `tabs.focus_key`, `mouse`, and `sidebar_key` for D9. `attend.channels.membership_cooldown` serves D2. attend registers them in its own schema, as ADR-503 Decision 13 provides.

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

### Negative

- Replay becomes a computation over membership events, scopes, inheritance and lineage; a bug there misroutes or withholds messages. It needs its own test table, and every view must show origin.
- Lineage at `SessionStart` depends on the hook's source and on Claude Code's session records; a continuation the hook cannot link is treated as a new agent, which is replayed nothing from before.
- ADR-172 Decision 5's guarantee becomes conditional: an explicit clear archives a message an idle live member has not read. The archive makes it recoverable, not delivered.
- Symmetric powers let a confused agent remove members, rescope or seal a channel; the cooldown, the history and the operator's view bound the damage but do not prevent it.

### Neutral

- `_groups.yaml` gains per-channel `scope`, `sealed`, `inherit_descendants` and per-member `exclusions`; channel histories gain membership events. The parser in attend-groups owns both, and an older attend ignores them.
- The consent asymmetry of issue #393 ends for `add`; `invite` keeps it.

## Implemented with this record

### Under the accepted decisions

These fit ADR-124, ADR-136, ADR-170 and ADR-172 as accepted and need none of the above:

- A self-join (CLI or scene) marks the channel's messages older than the cold-start window seen before writing the membership, and says how many it held back: ADR-136 Decision 2's "keeps a fresh join from dumping history". D3b replaces it with the membership rule.
- `/dissolve` from attend-chat no longer counts the operator's own heartbeat as a live member (ADR-170's guard protects peers working in the channel).
- The top row orders channels by their newest message, `#open` pinned first: the "recent-activity band" ADR-124 left for later, with its pinned `#open` kept.

### Ahead of acceptance

These shipped with the record and stand on the decisions named; rejecting a decision takes its item out:

- D1 and D3c, interim: attend-chat's `/invite` and Invite agent record a pending invitation beside the room's signals, and the invitee's `attend join` spends it and keeps the room's newest 50 messages deliverable, whatever their age, running a cold session's cold start at once so its first scan does not baseline them away. D3c's replay-from point (60 minutes before the invitation, or `--since`) replaces the age-blind newest 50 when D3 is built. Add agent is a stub that names D1: enrolling a session that has not started attend needs its cold start run for it, and that scan lives in the attend binary, out of attend-chat's reach. Clear history still runs today's `/purge`, which keeps messages younger than 90 seconds and any a live agent has not read.
- D9, the tab keys and menus: Ctrl+1-9 under the kitty keyboard protocol (Esc on an empty line asks before quitting where Ctrl+3 arrives as Esc), F2 and Ctrl+T, menus on a second click or a right click, the `+` slot, and the `≡` common menu.
- D8, the `attend.chat.*` keys with `/config`, the Keybinding set presets and the mouse item. The theme item holds for the session only.

## Open questions

- D3d's lineage rule awaits the operator's confirmation, in particular whether `--resume` continues an agent or starts one.
- Whether `attend inbox` should list only what D3b would replay, leaving the rest to `attend history`.

## Alternatives Considered

- **Keep invitations as requests only (issue #393).** A request an idle agent never acts on leaves the channel empty. D1 keeps `invite` and adds `add` beside it.
- **Replay by age (the cold-start window).** Old messages are held back from a new session, but a member that was away for an hour is treated like a stranger, and addressed mail to a project reaches every later session there. Membership answers "was this said to you" directly.
- **Brief an invitee from the inviter's last membership change, or from its own join.** The first is arbitrary, the second briefs nothing (D3c).
- **A clear that deletes.** Simpler, but a clear made by mistake, or by a confused agent under D1, would lose messages for good. Archiving costs one directory.
- **Clear waits for live consumers (today's Decision 5).** It cannot meet the operator's intent while any idle member is live, which is the usual case.
- **Glob rules for auto-membership.** Globs can overlap without nesting, so "the deepest channel" is undefined for them. Directory scopes make nesting a prefix test.
- **Same-scope channels as one depth.** Enrolls every agent there in two rooms with one meaning; refusing keeps one name per place.
- **Operator-only scope, seal and membership.** Simpler to reason about, but agents would route every reorganization through the operator, the bottleneck this record removes.
- **A `no_self_join` switch on sealed channels.** Dropped: a seal is about inheritance, and refusing joins brings back a ban by another name.
- **No override at all.** Simplest, and a flap could not be forced; but an operator correcting their own mistake would wait out the cooldown with nothing to do.
- **Exempting the operator, by a CLI flag or by attend-chat's identity.** A flag any agent could pass, and an exemption that asks nothing makes the cooldown invisible to whoever holds it. Asking each time keeps it visible and recorded.
- **A timing or hold-to-confirm test before the override.** Considered and dropped as more ceremony than the risk needs: anything that drives the screen could pass it too.
