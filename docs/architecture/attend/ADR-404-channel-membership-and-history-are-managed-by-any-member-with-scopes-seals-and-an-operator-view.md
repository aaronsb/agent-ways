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
  - evidence: "/purge keeps any signal a live consumer has not seen (tools/attend-chat/src/app/keys.rs, purge_channel_in) and never reaches a project tray; the cold-start rule delivers addressed project-tray mail whatever its age, the newest 50 (tools/attend-state/src/cold_start.rs, plan); a reproduction in an isolated cache showed a new agent drained all 30 day-old addressed messages after #open and channel history were deleted"
  - evidence: "/invite writes a directed signal and the invitee must run attend join itself (tools/attend-chat/src/app/keys.rs, run_invite: consent asymmetry, issue #393); /kick removes without consent"
  - precedent: ADR-136
  - precedent: ADR-170
  - precedent: ADR-172
  - precedent: ADR-173
  - precedent: ADR-503
  - precedent: ADR-504
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
  - ADR-136
  - ADR-170
  - ADR-172
  - ADR-173
  - ADR-503
  - ADR-504
---

# ADR-404: Channel membership and history are managed by any member, with scopes, seals and an operator view

## Summary

- **Decided:** ten decisions, each separable, so each can be accepted or rejected alone. D1 gives every member, agent or human, the same powers over a channel: invite or enroll a member, remove one, seal and unseal. Removal is not a ban. D2 adds a cooldown against flapping, on every channel and for everyone; attend-chat can override it for one action, after asking. D3 lets an explicit history clear archive a room's and a project tray's history past the unconsumed guard, so new agents are not caught up on it. D4 gives a channel a directory scope that enrolls agents by where they work, with membership inherited downward. D5 adds sealed channels, which cut inheritance. D6 lets `attend join` and `attend leave` take a path. D7 renders the same model as text for agents. D8 registers the settings these need, and lets attend-chat write the one theme choice. D9 is the operator screen's layout. D10 defines that override.
- **Trades away:** the consent asymmetry of issue #393 (only the invitee could add itself), and ADR-172 Decision 5's guarantee that no message a live session has not consumed is ever removed, where an explicit clear says otherwise. Membership stops being a flat list: it becomes manual entries plus scope rules plus inheritance, which every view must explain.
- **One-way?** No. D3 archives rather than deletes, so a clear can be undone. Scopes, seals and exclusions are new state in `_groups.yaml` that an older attend ignores; dropping them later is a migration of that file, not of messages.
- **Probes:** *Confident (powers):* you want an agent you set to work on a project to be able to bring a colleague into the channel without you, as you can. *Not confident (clear):* when you clear a channel's history, an idle agent that is still a member and has not read the last few messages should lose them too, rather than the clear waiting for it.
- **Inversion:** the ends are a flat chat room, where membership is whatever each member last typed, and an access-controlled directory, where only an administrator changes anything. This sits in between: anyone may change membership, every change is logged and announced, and the operator's screen can always see and undo.

## Context

An operator cleaned up attend's rooms from attend-chat and found that a new agent was still caught up on old messages. The causes, in this codebase as of this record:

- A session that joined a channel after its first scan was handed the channel's whole backlog: the cold-start rule ran only on the first scan. Fixed in the same change as this record (below).
- Addressed mail in a project's tray is delivered to a new session whatever its age, the newest 50 (ADR-172, addendum of 2026-10-01). Nothing the operator can run clears a tray: `/purge` reaches `#open` and channels only, and a tray lives as long as its project (ADR-136 Decision 3).
- `/purge` keeps every message a live consumer has not seen (ADR-172 Decision 5). An idle but live member holds the history open.
- `attend inbox` lists every message in the receive set, and the cold-start note points new agents at it.

The operator also wants to create a channel for a topic and put agents in it. Today `/invite` sends a request the agent must act on itself, a rule from issue #393 that no accepted record states. And the operator's model of a channel is a part of the file tree: one per project, one per subdirectory, agents placed by where they work.

Agents join and leave channels on their own through the CLI (`attend join`, `attend leave`), and that stays. The TUI is the operator's view; everything it can change, an agent can change through a CLI verb.

## Decision

### D1. Every member has the same channel powers

Any member of a channel, agent or human, may invite or enroll a member, remove one, and seal or unseal the channel (D5). Members are agent sessions and human operator sessions alike (ADR-170). The verbs follow attend's existing names: `attend invite <member> [<channel>]` enrolls the member at once (the invite becomes membership, not a request), `attend kick <member> [<channel>]` removes, `attend seal <channel>` and `attend unseal <channel>`. attend-chat's `/invite`, `/kick` and the tab menu's Add agent and Remove agent run the same paths.

- **An invitation brings a briefing.** A join that answers a pending invitation for that member and channel skips the late-join baseline: the room's history stays deliverable, the newest 50 messages, the cap the cold-start rule puts on addressed mail. A self-join with no invitation keeps the baseline. Under D1 the invitation is itself the enrollment, so the briefing is what the enrolled member is handed first.

- **Removal is not a ban.** Removing a member stops delivery to it and records an exclusion for that member and channel. The exclusion blocks only automatic re-enrollment (D4 scopes, inheritance). An explicit `attend join` by the member always succeeds, clears the exclusion, and is logged like any other change. There is no ban state. The one thing that can refuse a join is D5's no-self-join switch.
- **A self-leave records an exclusion too**, so a leave from a scope- or inherited-membership channel is not undone by the next evaluation.
- **Every change is logged and announced.** The membership log in attend's cache records actor, verb, member, channel and time. The channel receives a system line naming the actor, such as `@kg-agent sealed #project-a-docs` or `@aaron added @wells`.
- **Operator access cannot be lost.** An agent may remove an operator from a channel, which stops delivery to that operator. The operator screen still lists every channel, and the operator can rejoin or reverse any change from it.

### D2. A membership cooldown per member and channel

After any membership change for a (member, channel) pair (an invite, a removal, a self-join, a self-leave), a further change for that pair is refused until the cooldown has passed. The refusal says how long remains and is logged. Sealing and unsealing take the same cooldown, keyed per channel. The duration is `attend.channels.membership_cooldown`, in seconds, default 60, registered in attend's schema (ADR-503 §13).

The cooldown is universal: every channel, `#open` included, with no per-channel switch and no exempt actor. The one way past it is D10's override in attend-chat, for one action at a time. The CLI has no override.

### D3. An explicit clear archives history past the unconsumed guard

A history clear is an explicit act by a member, and it means what the operator said: new agents are not caught up on what came before it.

- `/purge`, the tab menu's Clear history, and a new `attend purge [<channel>]` move every signal in the room older than the 90-second grace window into an archive outside every receive directory (`_archive/<room>/`), whether or not a live session has consumed it. This amends ADR-172 Decision 5 for an explicit clear only; the drain and the sensors still never remove a message, and the age-only reaper of ADR-136 Decision 3 is unchanged.
- An archived message is never delivered by a conduit, is not counted in the cold-start note, and is listed by `attend inbox --archived` only. `attend unarchive <channel>` moves it back.
- A project tray takes the same clear: `/purge @name` or `attend purge --tray <path>`. The cold-start rule's "addressed mail whatever its age" (ADR-172, addendum of 2026-10-01) then holds only for mail newer than the tray's last clear.
- The clear's status line says how many were archived, and from where.

### D4. A channel's scope is a directory, and enrollment follows it

A channel may carry one scope: a directory, meaning that directory and everything under it. The scope is stored canonical (tilde expanded, symlinks resolved, no trailing slash) in the channel's `_groups.yaml` entry. Scopes are not globs, so whether one channel nests under another is path-prefix containment, always decidable.

- **Auto-enrollment.** When an agent session starts or joins the bus, and when a scope is added or changed for sessions already live, the agent joins the deepest channel whose scope contains its project directory (ADR-171's origin path), and no other by scope.
- **Inheritance downward.** The members of a channel, whatever their origin, are effective members of every channel whose scope nests under it. Not upward: an agent in a docs channel does not receive the root channel's traffic. A channel may switch off delivery of its descendants' traffic to its inherited members (`inherit_descendants: false`), since the volume grows with depth; the default is on.
- **Two channels with the same scope are refused.** Setting a scope that another channel already holds fails and names that channel. Treating them as one depth would enroll every agent there in both, two names for one room.
- **Exclusions apply to effective membership.** A removal or self-leave (D1) excludes the member from the channel whether its membership came from a scope, from inheritance or from a join.
- **Origin is shown.** Every view of membership names its origin: `manual`, `scope`, or `inherited-from-<channel>`.
- **Setting a scope is operator-only for now.** That is the one power D1 does not share. Whether agents should set scopes is an open question below.

### D5. A sealed channel cuts inheritance

Any member may seal or unseal a channel (D1). A sealed channel does not inherit members from channels whose scopes contain it. The cut is at the seal: channels nested under a sealed channel inherit from the sealed channel's effective members, never from above it, so an ancestor cannot reach into a sealed subtree through a nested channel.

A seal blocks inheritance, not a join. An agent may `attend join` a sealed channel, and it shows as `manual`. A per-channel `no_self_join` switch, default off, refuses a self-join to a sealed channel, a rejoin after removal included; an invite by a member still admits. "Sealed" is the term, since `scene private` already means leaving every channel.

### D6. `attend join` and `attend leave` take a path

An argument starting with `/`, `~` or `.` is a path; channel names cannot contain `/` (`attend_groups::validate_group_name` admits letters, digits and `-`). A path is canonicalized and resolves to the channel whose scope equals it, else to the deepest channel whose scope contains it, the D4 rule. No covering channel is an error that lists the nearest channels. A path never creates a channel, since scopes are operator-set; a join by name creates one as today. A leave by path resolves the same way.

### D7. The agents' text views carry the same model

`attend channels` renders the channel tree by scope nesting as indented text: each channel with its scope, member count, sealed marker, and for each member its origin. `attend peers` shows each agent's effective channels. Both are compact, one line per entry, and greppable: an agent reads the same model the operator sees.

### D8. attend-chat edits its own keys, and the theme

The chat's keys are attend's, under `attend.chat.*`, in attend's user file: `tabs.jump`, `tabs.menu_on_repeat`, `tabs.focus_key`, `mouse`, and `sidebar_key` for D9. `attend.channels.membership_cooldown` serves D2. attend registers them in its own schema, as ADR-503 Decision 13 provides.

That decision also says attend's settings "are edited through `ways settings`" and that "`attend` has no settings UI of its own". attend-chat is a separate application (ADR-504 Decision 1), but the keys are attend's, and its `/config` and the common menu's Keybinding set and Mouse items are a second way to edit them. This amends ADR-503 Decision 13: attend-chat may list and set its own `attend.chat.*` keys, through attend-config's writer and the schema `ways settings` reads, so the two stay one definition and one file. `ways settings` still edits every key; `attend` itself still has no settings UI.

On the same terms attend-chat may write the one theme choice, ways' `theme.active`, through the settings writer and the declaration `agent_theme::settings` holds, as ADR-504's note of 2026-10-01 says the theme tab does. Until then the common menu's Theme item changes the look for the session only.

### D9. The operator screen's layout

- The top row holds the common menu (`≡`) and the channels in use: joined, or active recently. Action slots carry no number, so merged stays tab 1.
- A collapsible left sidebar shows the full channel tree by scope nesting, expanding and collapsing as a directory browser does, with compact per-row figures: members, unread, time since the last message, and the sealed marker, for example `▸ project-a-root  3👤 12✉ 4m`. Enter or a click on a row opens the same context menu as the tab. `attend.chat.sidebar_key` toggles it.
- The channel menu gains Seal or Unseal and Auto-join rule… (setting the scope, D4).
- The tabs of a pane with menus are reached by Ctrl+1-9 where the terminal reports Ctrl+digits (again on the shown tab: its menu), by F2 or Ctrl+T on the tab bar, and by a second click or a right click; Alt+1-9 stays where it arrives. This amends ADR-504's note of 2026-10-02 on panes, which names Alt+1-9 as the tab key beside text: Konsole and GNOME Terminal keep Alt plus a digit for their own tabs.

### D10. attend-chat may override the cooldown, after asking

When the cooldown blocks an action in attend-chat, the bottom bar asks `cooldown <remaining> — override? y/N`. `y` performs the action; any other key cancels it. An override is logged with its actor like every other membership event, and the channel's system line says it was an override. The CLI has no override, and a headless `attend-chat --snap` is a dry run that changes nothing.

The question is a pause, not a lock: anything that drives attend-chat can answer it. It stops a flap that nobody meant, and the log names every override.

## Consequences

### Positive

- An operator can clear a room and be sure a new agent is not caught up on it, including the addressed mail that no command reached before.
- Agents organize themselves: one can bring a colleague into a channel or seal a subtree without asking the operator, and every such change is announced in the channel.
- Channels map onto projects and their parts, so an agent lands in the right room by starting work in the right directory.
- One model, shown three ways: the operator's tree, the tab menus, and the agents' `attend channels`.

### Negative

- ADR-172 Decision 5's guarantee becomes conditional: an explicit clear archives a message an idle live member has not read. The archive makes it recoverable, not delivered.
- Membership is computed, not listed. A member's channels depend on scopes, seals, inheritance and exclusions, and a bug there misroutes messages. Every view must show origin, and the evaluation needs its own test table.
- Symmetric powers let a confused agent remove members or seal a channel; the cooldown, the log, the system line and the operator's view bound the damage but do not prevent it.
- The cooldown refuses a legitimate quick correction made through the CLI; attend-chat's override is the way past it.

### Neutral

- `_groups.yaml` gains per-channel `scope`, `sealed`, `inherit_descendants`, `no_self_join` and per-member `exclusions`; the parser in attend-groups owns them, and an older attend ignores them.
- The consent asymmetry of issue #393 ends: `/invite` enrolls.

## Implemented with this record

### Under the accepted decisions

These fit ADR-136, ADR-170 and ADR-172 as accepted and need none of the above:

- A self-join (CLI or scene) marks the channel's messages older than the cold-start window seen before writing the membership, and says how many it held back: ADR-136 Decision 2's "keeps a fresh join from dumping history".
- `/dissolve` from attend-chat no longer counts the operator's own heartbeat as a live member (ADR-170's guard protects peers working in the channel).

### Ahead of acceptance

These shipped with the record and stand on the decisions named; rejecting a decision takes its item out:

- D1, the invitation briefing: attend-chat's `/invite` and Add agent record a pending invitation beside the room's signals, and the invitee's `attend join` spends it, keeps the newest 50 messages deliverable, and runs a cold session's cold start at once so its first scan does not baseline the briefing away. Add agent still invites rather than enrolls; Clear history still runs today's `/purge`, which keeps messages younger than 90 seconds and any a live agent has not read.
- D9, the tab keys and menus: Ctrl+1-9 under the kitty keyboard protocol (Esc on an empty line asks before quitting where Ctrl+3 arrives as Esc), F2 and Ctrl+T, menus on a second click or a right click, the `+` slot, and the `≡` common menu.
- D8, the `attend.chat.*` keys with `/config`, the Keybinding set presets and the mouse item. The theme item holds for the session only.

## Open questions

- Should agents set a channel's scope? D4 keeps it operator-only until misuse is understood.
- Whether `attend inbox` should list only messages newer than the reader's join, not the room's whole remaining history.

## Alternatives Considered

- **Keep invitations as requests (issue #393).** The operator asked to enroll agents, and agents asked the same of each other through the operator. A request an idle agent never acts on leaves the channel empty. Rejected; the system line and the log keep the change visible to the enrolled member.
- **A clear that deletes.** Simpler, but a clear made by mistake, or by a confused agent under D1, would lose messages for good. Archiving costs one directory.
- **Clear waits for live consumers (today's Decision 5).** It cannot meet the operator's intent while any idle member is live, which is the usual case.
- **Glob rules for auto-membership.** Globs can overlap without nesting, so "the deepest channel" is undefined for them. Directory scopes make nesting a prefix test.
- **Same-scope channels as one depth.** Enrolls every agent there in two rooms with one meaning; refusing keeps one name per place.
- **Operator-only seal and membership.** Simpler to reason about, but agents would route every reorganization through the operator, the bottleneck this record removes.
- **No override at all.** Simplest, and a flap could not be forced; but an operator correcting their own mistake would wait out the cooldown with nothing to do.
- **Exempting the operator, by a CLI flag or by attend-chat's identity.** A flag any agent could pass, and an exemption that asks nothing makes the cooldown invisible to whoever holds it. Asking each time keeps it visible and logged.
- **A timing or hold-to-confirm test before the override.** Considered and dropped as more ceremony than the risk needs: anything that drives the screen could pass it too.
