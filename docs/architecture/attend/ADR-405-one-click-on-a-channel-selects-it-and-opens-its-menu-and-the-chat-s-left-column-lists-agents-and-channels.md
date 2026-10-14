---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-404#D9]
basis:
  - operator: aaronsb
    level: directed
    said: "new fix for attend-chat: the merged channel needs to be called something different like ('combined' or full stream or something) - because it's all the chats together. Next, I don't want the merged channel to be actually a channel - it's an operator view for attend-chat, and should be justified right side, maybe with some characters that indicate 'merged chat' (no more than 3-4). That tab should be a different color (lighter? shaded? not sure). The chat box itself needs to be one of the tabbable elements. the text entry box swallows normal keyboard commands (including shift and tab) because it's the entry panel. when the chat box is active regular tab is autocomplete. whenever the entry box is active, we have a little legend update that indicates that shift+tab or arrow can be used to leave the box. nav arrows (up down left right) work inside the box, but if the up arrow is used when the cursor is at the beginning of the first line of the entry box, it is the same as leaving focus for the entry box and allows navigation of elements. same goes for the last position of the last line, down arrow exits the pattern. we need to define the chain of elements that tab (or shift tab, or left and right arrows) do for navigating the elemnts outside of chat. we need every agent entry and channel to be in that loop. we also need the hints on the bottom to reflect the context of where the active cursor is at, and we can use enter to raise the context menu for any element. (the same context menu that the mouse click affords). when not in the text entry box, we should be able to use all the characters for contorl that the help (F1) claims. If we're not deliberately in chat, the lighter highlight around the entry box for text is not brighter as it is now, but instead the lower border color.. we could say that this is a nav polish pass"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "up and down nav and enter opens the context menu (the same as a left mouse click on an item)"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "fo the open channel, I want that also to the right, as a reserved channels. we can consider that reserved channels are right side and dynamic channels or created channels are left side"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "the 'new channel' plus can be on the left side"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "in the agent roster, let's add the context menu 'add to chat' for any agents'. it works just like the flow for removing an agent from a chat (right now on a chat, pick \"remove agent *\" -> \"pick one: remove from #channenlname\"). So now, picking an agent a new flow \"add agent to chat *\" -> \"pick one: add to channel\" (and the list of channels)"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "some behavior I was hoping to see in the attend-chat was using the mouse to click on a channel should select it and also open the context menu. the left vertical isn't showing all channels or sessions (including inactive ones) and I think we likely need to split it into two vertical sections as well - a list of agents who have attended chat and a list of channels"
    via: chat, relayed by the main session, 2026-10-10
  - operator: aaronsb
    level: directed
    said: "currently I can see the agents at the bottom, and they draw across, (that's been the existing behavior) but I'd like to arrange them on a left side column for space saving."
    via: chat, relayed by the main session, 2026-10-10
  - operator: aaronsb
    level: directed
    said: "additionally the formatting style of our statusline is a style approach I'd like to pick up"
    via: chat, relayed by the main session, 2026-10-10
  - operator: aaronsb
    level: directed
    said: "as the human, I need to be able to use ui/ux conventions to get around - submitting rapit command line calls isn't really my thing"
    via: attend-chat, relayed by the main session, 2026-10-10
  - operator: aaronsb
    level: directed
    said: "*rapid"
    via: attend-chat, a separate message correcting "rapit" in the one above, relayed by the main session, 2026-10-10
  - precedent: ADR-404
  - precedent: ADR-504
  - operator: aaronsb
    level: directed
    said: 1. digits only work on channel rows. 2. a. 3. agree with new behavior 4. keep 5. keep 6. keep the mark and add count on tab. 7. thats correct as built 8. agree- its a single char column and makes sense. 9. correct. 10. correct.
    via: 'the operator''s answers to the design questions in PR #972, given to the head session, 2026-10-13'
  - operator: aaronsb
    level: directed
    said: "I'm not so sure, it would get complicated - my intuition tells me we could pick any agent out of the list, and have a special conversation view under that agent on the left nav bar that can reveal the conversation flow from that agent's perspective. so if I pick 'agent froboz' from the left nav bar and pick a new option called 'review DMs' then it's DM perspective appears under my DM flow link, labeled agent froboz DMs (with a different color maybe to be distinctive). This 'agent view' is dismissable I think, in order to declutter (it's just show/hide) (and also, it makes me think we can show/hide our dms to that agent too). So we have two DM views for all agents on the left nav bar, that are hide-able: the obvious one, the dms between the attend-chat operator and the agent, and then a second view which is that agent's DM view. (which would include it's conversations to the attend-chat operator). it's just a different query of the same messages."
    via: "the operator's chat message to the head session, 2026-10-13, describing the per-agent DM views"
  - operator: aaronsb
    level: directed
    said: "the ui for attend-chat does not seem to open a dm channel under your agent on the left side nav bar"
    via: "the operator's follow-up to the head session, 2026-10-13, reporting that no DM view opened under an agent"
  - operator: aaronsb
    level: directed
    said: "I think the open behavior is great as it is - if I @ message you on open, you get notified which is fine."
    via: "the operator's follow-up to the head session, 2026-10-13, confirming that @Name in #open stays an addressed #open post"
  - operator: aaronsb
    level: directed
    said: "the agent's dms are read only from attend-chat's perspective"
    via: "the operator's chat message to the head session, 2026-10-13"
  - operator: aaronsb
    level: directed
    said: "by 'read only' it's only basically we don't have a function in attend-chat to write to that"
    via: "the operator's chat message to the head session, 2026-10-13, saying what the line above means"
  - operator: aaronsb
    level: guided
    said: '"we should add an option to clear dms" "on the side bar"'
    via: Claude Code session 2026-10-13
  - operator: aaronsb
    level: guided
    said: we probably need to be able to tombstone an agent's messages so they don't come back into chat again
    via: Claude Code session 2026-10-13
  - operator: aaronsb
    level: guided
    said: "build the capabilities that are missing and make sure the ones implemented don't have inconsistent implementation"
    via: "issue #931, quoting the operator's goal for the attend-chat UX audit; its build item B7 is editable settings"
  - operator: aaronsb
    level: directed
    said: "Named channels get \"Hide messages here\" (and \"Show hidden messages\"), not only DMs and #open. Same per-tab watermark semantics; it hides on this screen only."
    via: "the operator's decision of 2026-10-14, relayed by the main session"
  - operator: aaronsb
    level: directed
    said: "Delete and tombstone get their own thread. attend message delete (all its forms, incl. Undo) and attend message tombstone run on a dedicated thread with NO kill timeout; the row/status shows a \"deleting…\" (or \"tombstoning…\") state until it finishes, then the result. Only one delete/tombstone runs at a time."
    via: "the operator's decision of 2026-10-14, relayed by the main session"
  - operator: aaronsb
    level: directed
    said: "Never pop up; confirm on re-choose"
    via: "the operator's pick in the head session, 2026-10-14, after three review rounds found stray-keystroke paths to a delete for everyone"
  - operator: aaronsb
    level: directed
    said: "let's set it up so it's just one flow: after I choose the selection (custom, day, etc) we immediately provide a dialog to confirm delete. if the user declines, we clear the delete markers. if they accept, then we actually do the delete."
    via: "the operator in the head session, 2026-10-14, after trying the report-then-re-choose build"
  - operator: aaronsb
    level: directed
    said: "add a little countdown for delete so it's obvious"
    via: "the operator in the head session, 2026-10-14"
  - precedent: ADR-503
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "we could say that this is a nav polish pass"
    via: chat, relayed by the main session, 2026-10-12; the full spec is quoted in basis
status: accepted
date: 2026-10-10
deciders:
  - aaronsb
related:
  - ADR-404
  - ADR-504
  - ADR-409
---

# ADR-405: One click on a channel selects it and opens its menu, and the chat's left column lists agents and channels

## Summary

- **Decided:** in attend-chat a single left click on a channel, on the tab bar or in the left column, shows that channel and opens its menu. The left column has two stacked sections: Agents, every agent the chat has seen, inactive ones set back; and Channels, every channel including inactive and empty ones. Rows are drawn as segmented lozenges in the style of the operator's Claude Code status line. While the column is shown, the agent legend below the compose box gives its row back to the feed.
- **Trades away:** a click that only switches channels. Reading a channel without its menu opening takes a keyboard jump, Tab, or Esc after the click.
- **One-way?** No. Both are drawing and input handling in attend-chat; nothing on disk changes.
- **Probes:** *Confident:* a click should do both at once, never one per click. *Not confident:* a click on an agent row should open a menu (Address @Name, Who is this, Remove from the shown channel) rather than put `@Name ` in the compose box at once, as a click on its chip in the legend did; addressing then takes a click and an Enter.
- **Inversion:** the ends are a chat with no navigation chrome, where channels and agents are reached by typing, and a chat whose chrome lists everything and acts on every click. This sits near the second end, with `attend.chat.sidebar_key` to hide the column.

## Context

ADR-404 D9 specified a collapsible left sidebar holding the channel tree and toggled by `attend.chat.sidebar_key`. It opened a tab's menu on a second click or a right click. The sidebar was not built. Agents appeared only in the legend row under the compose box, filtered to live sessions. The tab bar listed channels that still had a directory on disk.

## Decision

D9 is amended as follows.

**(a) Click.** A left click on a channel tab or a Channels row shows the channel and opens its menu, the same menu as the tab's. A right click still opens the menu. A click on merged does the same with merged's menu. The action slots (`≡`, `+`) are unchanged. Keyboard paths are unchanged: Ctrl or Alt plus a digit shows a tab and opens its menu when repeated, and F2 or Ctrl+T focus the tab bar.

**(b) The left column.** `attend.chat.sidebar_key` toggles it, F3 by default (Ctrl+B is tmux's prefix); the `≡` menu's Hide column and Show column do the same with the mouse. It is shown at start and hidden automatically when the terminal is too narrow to hold it beside the feed. With the key set to `none` it is shown, and only the menu hides it.

- **Agents:** every agent the chat has seen, from session records and message senders. Live agents come first. An inactive agent is drawn in the muted variant. A row is a lozenge with the nickname on the agent's identity colour, the project directory on a darker step of that colour, and a state marker on the darkest step. The steps are derived from the identity colour as the status line derives them. A click on a row, or Enter on it, opens the agent's menu: Address @Name puts `@Name ` in the compose box; Who is this shows what `/whois` says, or for an agent that is not live what the chat knows of it; Remove from #channel, offered when the shown channel lists the agent, removes it by its membership id, live or quiet, after a `y`. While the column is shown, the legend row under the compose box is dropped whenever it would list agents or channels, and the column underlines what Tab would complete to. Tab completes over the column's own lists: every claude, live or quiet, and every channel, swept ones too. A human is listed but neither underlined nor completed, since `@name` routes to a session's inbox. The legend row still shows slash commands and argument hints, and keeps its place while a command is typed.
- **Channels:** `#open`, then every channel with a directory or a `_groups.yaml` entry, including empty ones and ones whose directory was swept. A row is a lozenge on the accent in three segments: the glyph and name; the member count and the time since the last message; and `●` when the channel is active (on disk, and either `#open` or with a live member) or `○` when it is not, the place D5's seal marker will take. An inactive channel's row is set back. Unread counts, seals and scope nesting are drawn when D3, D4 and D5 provide the data. Until then the list is flat.
- Each list scrolls when it is longer than its rows: the keyboard's cursor keeps itself in view, the wheel over a list scrolls it, and its header marks rows out of view with `↑` and `↓`.
- The column's rows are reached by mouse and by keyboard. F2 or Ctrl+T focus the tab bar, and Down moves into the column; the bottom bar then reads `↑↓ move · Enter · Esc back`. Up and Down move along the rows, Enter acts as a click does, Up past the first row returns to the tab bar and Esc to the compose box. F2, a click outside the column, or the column hiding for width takes the keyboard back from it. A channel's menu offers Join to a channel the human is not in, and Leave to one it is in. Each menu item that drops something (Clear view, Clear history, Leave, Delete channel, Remove) asks for `y` first, since a click opens a menu with its first item under the cursor. (Clear view and Clear history became Hide messages here and Delete read messages for everyone… in the amendment of 2026-10-13; the amendment of 2026-10-14 offers Hide messages here on every channel.)
- What still takes a slash command or is not built: Add agent, which enrols a member without asking, is ADR-404 D1 and says so; sealing and scopes are D4 and D5; the `≡` menu's settings are item (c). Tab completes a quiet agent or a swept channel, and a send to one is refused as before (ADR-129): the completion reaches it, the send does not.
- With the shape set to `plain`, or without colour, the lozenges lose their Nerd Font glyphs, as every other lozenge in agent-tui does.

**(c) Settings.** The `≡` menu's Settings ▸ lists and sets the settings the chat and the attend mod read. Issue #931 lists this as build item B7.

- **The list.** It shows `attend_mod.listening` and `attend_mod.keepwarm`, then the chat's `attend.chat.*` keys, one row each as `name = value (layer)`. The value is the one in effect for the project enclosing the chat's working directory, or for the user when no project encloses it. The layer is `default`, `user`, `target` or `project`.
- **The place.** An attend mod key is a ways key that a project can set (ADR-406), so picking one asks where to write it. The choices are the user file, the project enclosing the chat's directory, the project of each live agent, and Another project…, a directory typed in an entry dialog, where `~` is the home directory. Each choice names what its file holds now. A directory that no project encloses is refused inside the dialog. A chat key is held in the user file alone, so picking one goes straight to its values.
- **The value.** The picker lists the key's values and marks the one in effect at that place. Unset here is offered where the place's file holds the key, and it removes the key from that file.
- **The write.** A pick is written as `ways settings set <key> <value> [--project <root>]` writes it. The file, the header of a new file and the edit are the same, and the write goes through the agent-settings writer under its lock. The writer waits at most two seconds for another holder of the lock, as the `ways settings` screens do, and then writes nothing and says so on the bottom bar. A project is found by the rule `ways settings` uses: the nearest enclosing project root, else the nearest git checkout, and never the home directory.
- **What the bar says.** It names what was written and where. It reads the key back where the chat runs, or at the project written, through the check `ways settings set` makes before it exits 4. When the written file's section fails the schema and falls back to its defaults there, or a higher file still sets the key, the bar says so as a warning. After Unset here it says what is in effect and which higher file still sets the key. The chat's own keys apply at once. The picker closes after the write, as Theme ▸ and Keybinding set ▸ do.
- `/config` still lists and sets the chat's own keys.

## Consequences

### Positive

- One gesture reaches a channel's actions.
- Agents and channels that have gone quiet stay visible, so they can be addressed, cleaned up or rejoined.
- The legend's row goes back to the feed, which suits a wide terminal.

### Negative

- The column takes about 26 columns from the feed. Below a threshold width it hides itself.
- Agents seen only in past sessions are listed while their session records exist. A record Claude Code has removed leaves no trace until the agent sends again.

### Neutral

- `attend.chat.sidebar_key` joins attend's schema beside the other `attend.chat.*` keys (ADR-404 D8).
- attend-chat depends on `ways-core` for the ways schema, layers and project rule that item (c) writes through. This reverses an alternative ADR-504 rejected, putting the Claude locator in `ways-core`, which it turned down because attend's crates would take on `ways-core`'s dependencies. Measured on 2026-10-13, the attend-chat binary grows by only 75,888 bytes (3.9%, including the feature's own code), since the crates ADR-504 named were already in its dependency tree.
- agent-tui gains a pane hook for opening a tab's menu on the first click, and a way for a pane to hand the keyboard back to the tab bar. Other panes keep the second-click rule.

## Alternatives Considered

- **Keep D9's second click.** Two clicks for one intent. Reported as unexpected.
- **One combined list of channels with members nested under them.** An agent in several channels repeats, and an agent in none has no place.
- **Keep the legend row and add the column.** It shows the same agents twice and costs a feed row.

## Amendment (2026-10-12): the operator view, reserved tabs, one focus owner, and Add agent to chat

The operator asked, in their words: "the merged channel needs to be called something different ... I don't want the merged channel to be actually a channel - it's an operator view for attend-chat, and should be justified right side, maybe with some characters that indicate 'merged chat' (no more than 3-4). That tab should be a different color", and "fo the open channel, I want that also to the right, as a reserved channels. we can consider that reserved channels are right side and dynamic channels or created channels are left side", and "the 'new channel' plus can be on the left side". The full words are in `basis`.

- **The operator view.** What was the merged tab is an operator view, every channel together, labelled `⋯all`: no `#`, no number of its own, never a channel to agents, and its menu offers Clear view alone (Hide messages here since the amendment of 2026-10-13; see also the amendment of 2026-10-14). It is drawn on the shaded ground of the rule (`strip::view_seg`), bold when shown, so it reads apart from the accent-coloured channel tabs; without colour it reads by its label. The label `≡all` was the other candidate; `≡` already marks the common menu, so `⋯all` was chosen.
- **Reserved tabs sit right.** `tabs::reserved()` is the one list of reserved tabs: `#open` (attend reserves `open`, and `broadcast`, which is `#open`'s own room and has no tab), then the operator view. The tab bar (`tabs::bar`) draws `≡`, the created channels, `+`, then the reserved tabs pinned at its right end. When the bar is short of room the created channels' names shorten and then drop behind `›`; the reserved tabs, `≡` and `+` never do. Created channels are numbered 1-9 from the left; the reserved tabs carry no number, and 0 is the bar's last tab, the operator view.
- **One focus owner.** The keyboard is on one element at a time, and that element alone is drawn focused: a tab reversed behind `▸`, a column row reversed behind `▸`, the feed and the compose box by an accent border. A box without the focus has the rule's subdued border, the compose box included, and the compose caret is drawn only while the box has the focus. The ring, one ordered list the key handler walks and the renderer marks (`ChatPane::ring`): the tab bar in the order drawn, every agent row then every channel row of the column while it is drawn, the feed, the compose box, then round again.
- **Keys.** In the compose box every key edits: Tab completes, Enter sends, the arrows move the cursor. Shift+Tab leaves backward. ↑ at the start of the first line leaves backward and ↓ at the end of the last line leaves forward, but only a box that holds text; an empty box is left with Shift+Tab or Esc (ADR-409, the operator's "Guarded" answer). The footer puts Enter and Tab first, then "S-Tab or ↑ at the top leaves the box". Outside it, Tab or → moves forward and Shift+Tab or ← back; ↑ and ↓ move along the column and through the feed's messages; Enter opens the focused element's menu, the one a click opens, through the one activation path; Esc goes back to the box. The help keys work there (`?`, `m`, PgUp and PgDn, F2, F3, Ctrl+N); `q` asks before it quits, whatever is unsaved; the digits switch tabs only while the tab bar has the focus, and elsewhere are typed like any other character, which goes to the box. Backspace and Delete only go back to the box. The footer names the focused element's keys, and the mode lozenge says where the focus is (`chat`, `tabs`, `view`, `column`, `feed`). A click on a column row puts the keyboard on that row, from wherever it was, so ↑, ↓ and Enter go on from the row clicked ("up and down nav and enter opens the context menu (the same as a left mouse click on an item)"). Any other click puts the keyboard back in the compose box, but for a click in the column or the feed while the focus is there.
- **The focus on a tab is kept by what the tab is**, not its place: a channel created under it leaves it on the same tab, and one dissolved under it moves it to the tab shown. Walking the bar previews each tab it lands on; walking off it into the column, the feed or the box without choosing puts back the tab shown before, so the draft's destination does not change on the way past. Enter or a digit on the bar keeps the tab, and a digit moves the focus to it; Esc from the bar cancels the preview, as F2 back to the box does.
- **The wheel scrolls what is under it.** Over a column list, that list; over the feed, the feed; over a menu, the key help or a report, that overlay; over the compose box, nothing.
- **Add agent to chat.** An agent row's menu offers Add agent to chat ▸, a picker titled `add to channel` of the channels the agent is not in (`#open` and the operator view left out), docked under the agent's row. The channel menu's Add agent ▸ does the same from the other side, a picker of live agents not in the channel (it replaces Invite agent, and the set-back "needs ADR-404 D1" item): both menus add by invitation. The pick invites the agent through the invitation `/invite` and Invite agent send, so the agent joins itself; attend has no enrolment without asking until ADR-404 D1, and the status line says "invited @X to #c — they join themselves". The item is set back with its reason for a human ("a human joins with /join"), a quiet agent ("quiet: no live session to invite"), and an agent already in every channel.

## Amendment (2026-10-12): one channel tab, a numbered column and the Esc cycle

ADR-407's amendment of the same date, "C, the operator's navigation redesign", changes the tab bar and the keys of the amendment above.

- **The tab bar.** It holds `≡`, the created channel shown, `+`, then the reserved tabs. The created channels are numbered in the left column, `#open` first, and no longer on the bar; while the column is hidden each channel's tab carries its number. Ctrl+n or Alt+n picks the column's channel n, and 0 is the operator view.
- **Keys.** In the compose box Esc goes to the feed, and Tab leaves the box when the caret ends no word to complete. The feed is the chat: a printable key there is typed into the box, and `q`, `m` and `?` are text. On the tab bar and the column the single keys work as before, and a plain digit picks a channel by its number only on a channel's row; agents carry no number. Outside the box Esc opens the `≡` menu, and Esc on that menu goes back to the tab or column row the focus was last on; Tab out of the box onto `≡` is not remembered. Walking the bar shows nothing; Enter on a tab opens its menu.
- **The column.** It marks the agents in the created channel shown, and that channel's tab carries its member count.

## Amendment (2026-10-13): two DM views per agent

The operator described the column's DM views (`basis`, the entry of 2026-10-13): "we could pick any agent out of the list, and have a special conversation view under that agent on the left nav bar", and "So we have two DM views for all agents on the left nav bar, that are hide-able: the obvious one, the dms between the attend-chat operator and the agent, and then a second view which is that agent's DM view". They noted that "it's just a different query of the same messages", and later reported that "the ui for attend-chat does not seem to open a dm channel under your agent on the left side nav bar".

- **Two views per agent, each a row under the agent's.** The first holds the direct messages between the operator and the agent, both ways. The second, the agent's own DM perspective, holds every direct message the agent sent or received, its DMs with the operator among them; the agent's menu opens it with Review DMs, and its row is labelled with the agent's name and drawn in a colour of its own. Each view is shown or hidden, and hiding one deletes nothing. Both are views over the messages the chat already holds, filtered by sender and recipient.
- **The second view offers nothing to send.** The operator called the agent's DMs "read only from attend-chat's perspective", and said what that means: "we don't have a function in attend-chat to write to that". attend-chat has no send as another agent or into its conversations, so the view shows no compose box. A view that sends nothing is a kind the nav model allows; the first view's compose box comes with its view, not with every view.
- **Opening the second view.** The agent's menu offers Review DMs under Show DMs (or Hide DMs), and Hide review while the view is listed. Review DMs lists the view on a row under the agent's, after the first view's row when both are listed, labelled with the agent's name and `DMs`, and shows it. The row is drawn on the info ground the first view's row wears, led by `◈`, and the view's tab, `Name DMs`, is led by the same mark, so the two views read apart in any palette. The feed lists every direct message the agent sent or received, with the operator or anyone else, each header naming whom it went to, plus the chat's own notices. The feed takes the compose box's rows, and its border reads `read only`. The focus ring has no compose box while the view is shown: Tab goes from the feed round to the tab bar, a focus that would go to the box goes to the feed, and a character typed or pasted there goes nowhere; a paste says `read only: nothing pasted`. A menu item that writes into the compose box, Address @Name or a message's Reply, first shows the operator's DMs with that agent, or the operator view for a human, and its message menu sets Quote back. Hide review, on the agent's menu or the view's own row and tab, takes the row off the column; when the hidden view was shown, the operator view takes its place.
- **Opening the first view.** A click on a claude's row lists its DM view on the row under the agent's and shows it, as a click on a channel's row shows the channel (ADR-407 (a)). A click on the agent whose view is shown opens the agent's menu. Enter on an agent's row opens its menu, as Enter on any row does, and the menu's first item is Show DMs, or Hide DMs while the view is listed. A right click opens the menu at once and keeps the channel shown, which is how the channel shown's Remove item is reached by mouse. A human's row has no DM view, since `@name` routes to a session's tray and a human has none: a click on it opens its menu at once.
- **What the view shows.** The feed shows the direct messages the agent sent the operator's person tray and those the operator sent with the agent among the recipients, plus the chat's own notices. The chat reads every tray, each project's and each person's, not only its own (issue #995): attend-chat now reads every agent's direct messages, and only reads them, since it writes into no agent's conversation but the operator's. A direct message the operator sent reads back from the tray it went to as sent by the operator to its recipients, whichever session sent it and whatever else the same message addressed: `@Name #channel` puts it in the agent's view and the channel's, and the operator view shows the channel's post alone. A message to agents in several projects leaves a copy in each project's tray and is listed once. What a send writes is read back from where it went at once, so it shows even when the watcher misses its event, and the watcher's copy of it, when it comes within a few seconds of the send, is dropped, however many messages arrive between. A direct message between others is read, lists no agent in the column by itself, and is the first to go when the chat's buffer is full. A direct message between others is read too, for the second view, and no view of the operator's shows it.
- **Sending from the view.** The compose box sends an unaddressed message to the agent as a direct message, `to` set to its session, never to `#open`. The destination flag reads `@Name`, marked `(quiet)` when the agent has no heartbeat, and a send to a quiet agent is refused with the draft kept. A message that starts with `@Name` or `#channel` goes as it would from the operator view (ADR-406 item 7). `@Name` in `#open` stays an addressed `#open` post, which wakes the agent; the operator confirmed it: "I think the open behavior is great as it is - if I @ message you on open, you get notified which is fine." Slash commands scope as they do in the operator view.
- **In the nav.** The DM row is indented under its agent's row and drawn on the info ground the destination flag wears, with the count of messages it holds; the shown view is marked as the channel shown is. The shown view's tab, `@Name DMs`, holds the left of the tab bar where the created channel shown would, on the shaded ground of a view. Hide DMs, on the agent's menu or on the DM row's own menu, takes the row off the column; when the hidden view was shown, the operator view takes its place, and the same DMs show there among everything else. A click on the agent's row lists the view again with its messages.


## Amendment (2026-10-13): delete read messages for everyone, hide messages here, and tombstone

The operator asked for a way to clear direct messages from the column: "we should add an option to clear dms" "on the side bar", and "we probably need to be able to tombstone an agent's messages so they don't come back into chat again". ADR-404 D3e, amended the same day, holds the rule and the verbs. This amendment places them in the menus.

- **Two items where Clear view and Clear history were.** Hide messages here hides, in the tab it is chosen on, every message the tab holds at that moment: a watermark per tab, kept in attend-chat's own state across restarts. The mark is the newest bus message the tab holds; the chat's own notices do not move it. Nothing on the bus changes, and other tabs and other people still see those messages. `/clear` does the same on the tab shown, and asks first as the item does. While a tab has a watermark its menu offers Show hidden messages, which takes the mark away. A message restored later by an undo, whose send order sorts below the mark, stays hidden in that tab until then. Delete read messages for everyone… opens a submenu, Delete older than: Current sessions / Today (local midnight) / All / Custom…; Custom… takes a duration or a local date in the entry dialog (ADR-409). The pick runs `attend message delete` with the tab's scope (`--open` for `#open`, `--channel <c>` for a channel) and the matching `--older-than`. The `y` question before it carries what `--dry-run` reports: how many messages go, and how many stay because a live agent has them unread. After a delete, the menu of the same scope offers Undo delete, which runs `attend message delete --undo <batch>` for the batch the status line named. The operator view offers Hide messages here alone.
- **The DM row, the DM tab and the agent row.** A DM view's row and its tab, and a claude's row in Agents, carry the same pair. Hide messages here acts on the agent's DM view. Delete read messages for everyone… runs the verb with `--dm <session-id> --as-person`, which reaches the thread between the operator and that agent. The view and the delete differ at one edge: the view also shows a message the operator sent that agent together with others, and the delete leaves such a message alone, since it is shared with a third party. The agent's direct messages with other agents are out of its reach: attend-chat has no function that writes into another agent's conversations, and its delete follows the same line.
- **Tombstone… on a dead agent's row.** A claude whose session is not live has Tombstone… on its menu, and on the menu of its own DM view (Review DMs), asked with a `y`. It runs `attend message tombstone <session-id>`; the agent's messages leave every tab and its row leaves Agents. A live agent's row never offers it. A dead sighting whose id is among a live session's ids before `/clear` is folded into that session's row, so it has no row of its own.
- **Through the attend CLI.** attend-chat runs both verbs as child processes with `--json` and shows the report on the status line. When the installed attend lacks a verb, the status line names the verb and says attend needs updating. `/purge` stays for one release as an alias of Delete read messages for everyone… → All on the shown channel.
- Item (b)'s rule holds: every item that drops something asks for `y` first, and the list now reads Hide messages here, Delete read messages for everyone…, Leave, Delete channel, Remove and Tombstone…. Undo delete restores and asks nothing.

## Amendment (2026-10-14): hide in a named channel, a delete that runs to its end, and one flow from the pick to the delete

- **Hide messages here on every channel.** A named channel's menu offers Hide messages here after Describe…, and Show hidden messages while the channel has a mark, as `#open` and the DM views do. The watermark is the same per-tab mark, kept in attend-chat's state file; it hides on this screen only, and the channel's members and the bus keep every message. The order is Add agent, Remove agent, Describe…, Hide messages here, Delete read messages for everyone…, Leave, Seal, Delete channel.
- **A delete runs to its end.** Every form of `attend message delete` (the dry run that the question carries, the delete, the undo) and `attend message tombstone` runs on a thread of its own with no time limit. Before this, attend-chat stopped an attend that gave no answer in ten seconds, which could cut a long delete short. The status line says what is running (`counting read messages…`, `deleting…`, `undoing the delete…`, `tombstoning…`) until it ends, then the result; a fresh result or note takes the line for its few seconds and the running state comes back after it. The screen stays live meanwhile. One delete or tombstone runs at a time: another asked for meanwhile, by its menu item, `/purge` or a `y`, is refused with a note, and nothing is queued. A named verb attend refused and a report without its count are told as before. attend-chat runs no other attend call, so nothing it runs has a time limit now. Since nothing stops it on its own, the operator can: the status line counts the seconds and says where (`deleting… 12 s · stop it from ≡`), and `≡` ▸ Stop deleting (or Stop tombstoning) kills attend, takes no answer from it, and says what that leaves (marks a stopped delete made come back with `attend message delete --undo`). Quitting while one runs goes through the exit guard, which names it. `/purge` refused while one runs keeps the command in the compose box.
- **One flow: the pick asks, the count fills the question.** Picking an age (a preset, Custom…, or `/purge`) opens the delete's question at once, in answer to that pick, reading `counting read messages in #x…` while the dry run runs; its yes is drawn set back and nothing presses it. When the count lands it fills the same question in place, `delete 412 read messages in #slow for everyone?`, and yes is live. A count of nothing, or attend's refusal, says so in the question and offers only no. No or Esc, at any time, closes it and stops a count still running; yes runs the delete for that scope and age. Once the count shows, the question counts down its seconds and at none closes as a no, the status line saying `delete question expired`, so a question left open while the operator was away cannot be answered by the next key typed on coming back. A count only ever fills the question its own pick opened, so it can never answer or replace another; each question carries an id of its own, and an answer acts only on the question with that id. The countdown and the waiting yes are the shell's question, not the chat's (ADR-504, note of 2026-10-14). (Earlier the same day the count opened its question itself, then reported and asked on a second pick; review found stray-keystroke paths in the first, and the operator found the second odd in use.)
- **Tombstone… only where attend takes it.** A dead row is offered Tombstone… only when no live session reads as its id: itself, or a live session that had it before a `/clear`. attend refuses either (`ReadAs`, `NotExited`). The column folds such an id into its session's row when that session has a row, but a live session with no row of its own (an enrolled or heartbeating agent that has not posted) left the earlier id's row standing and offered. The offer and attend's tombstone now ask the same reader list (`attend_groups::readers`).
