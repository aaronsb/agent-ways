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
- The column's rows are reached by mouse and by keyboard. F2 or Ctrl+T focus the tab bar, and Down moves into the column; the bottom bar then reads `↑↓ move · Enter · Esc back`. Up and Down move along the rows, Enter acts as a click does, Up past the first row returns to the tab bar and Esc to the compose box. F2, a click outside the column, or the column hiding for width takes the keyboard back from it. A channel's menu offers Join to a channel the human is not in, and Leave to one it is in. Each menu item that drops something (Clear view, Clear history, Leave, Delete channel, Remove) asks for `y` first, since a click opens a menu with its first item under the cursor.
- What still takes a slash command or is not built: Add agent, which enrols a member without asking, is ADR-404 D1 and says so; sealing and scopes are D4 and D5; the `≡` menu's Settings item shows the `/config` listing and sets nothing, so `attend.chat.sidebar_key` and the other keys are set with `/config <key> <value>`. Tab completes a quiet agent or a swept channel, and a send to one is refused as before (ADR-129): the completion reaches it, the send does not.
- With the shape set to `plain`, or without colour, the lozenges lose their Nerd Font glyphs, as every other lozenge in agent-tui does.

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
- agent-tui gains a pane hook for opening a tab's menu on the first click, and a way for a pane to hand the keyboard back to the tab bar. Other panes keep the second-click rule.

## Alternatives Considered

- **Keep D9's second click.** Two clicks for one intent. Reported as unexpected.
- **One combined list of channels with members nested under them.** An agent in several channels repeats, and an agent in none has no place.
- **Keep the legend row and add the column.** It shows the same agents twice and costs a feed row.

## Amendment (2026-10-12): the operator view, reserved tabs, one focus owner, and Add agent to chat

The operator asked, in their words: "the merged channel needs to be called something different ... I don't want the merged channel to be actually a channel - it's an operator view for attend-chat, and should be justified right side, maybe with some characters that indicate 'merged chat' (no more than 3-4). That tab should be a different color", and "fo the open channel, I want that also to the right, as a reserved channels. we can consider that reserved channels are right side and dynamic channels or created channels are left side", and "the 'new channel' plus can be on the left side". The full words are in `basis`.

- **The operator view.** What was the merged tab is an operator view, every channel together, labelled `⋯all`: no `#`, no number of its own, never a channel to agents, and its menu offers Clear view alone. It is drawn on the shaded ground of the rule (`strip::view_seg`), bold when shown, so it reads apart from the accent-coloured channel tabs; without colour it reads by its label. The label `≡all` was the other candidate; `≡` already marks the common menu, so `⋯all` was chosen.
- **Reserved tabs sit right.** `tabs::reserved()` is the one list of reserved tabs: `#open` (attend reserves `open`, and `broadcast`, which is `#open`'s own room and has no tab), then the operator view. The tab bar (`tabs::bar`) draws `≡`, the created channels, `+`, then the reserved tabs pinned at its right end. When the bar is short of room the created channels' names shorten and then drop behind `›`; the reserved tabs, `≡` and `+` never do. Created channels are numbered 1-9 from the left; the reserved tabs carry no number, and 0 is the bar's last tab, the operator view.
- **One focus owner.** The keyboard is on one element at a time, and that element alone is drawn focused: a tab reversed behind `▸`, a column row reversed behind `▸`, the feed and the compose box by an accent border. A box without the focus has the rule's subdued border, the compose box included, and the compose caret is drawn only while the box has the focus. The ring, one ordered list the key handler walks and the renderer marks (`ChatPane::ring`): the tab bar in the order drawn, every agent row then every channel row of the column while it is drawn, the feed, the compose box, then round again.
- **Keys.** In the compose box every key edits: Tab completes, Enter sends, the arrows move the cursor. Shift+Tab leaves backward, as does ↑ at the start of the first line; ↓ at the end of the last line leaves forward. The footer says "Shift+Tab or ↑ at the top leaves the box". Outside it, Tab or → moves forward and Shift+Tab or ← back; ↑ and ↓ move along the column and through the feed's messages; Enter opens the focused element's menu, the one a click opens, through the one activation path; Esc goes back to the box. Every single key the key help names works there (`?`, `q`, `m`, the digits, PgUp and PgDn, F2, F3, Ctrl+N), since nothing is being typed; any other character goes to the box and is typed. The footer names the focused element's keys, and the mode lozenge says where the focus is (`chat`, `tabs`, `view`, `column`, `feed`). A click puts the keyboard back in the compose box, but for a click in the column or the feed while the focus is there.
- **The wheel scrolls what is under it.** Over a column list, that list; over the feed, the feed; over a menu, the key help or a report, that overlay; over the compose box, nothing.
- **Add agent to chat.** An agent row's menu offers Add agent to chat ▸, a picker titled `add to channel` of the channels the agent is not in (`#open` and the operator view left out). The pick invites the agent through the invitation `/invite` and Invite agent send, so the agent joins itself; attend has no enrolment without asking until ADR-404 D1, and the status line says "invited @X to #c — they join themselves". The item is set back with its reason for a human ("a human joins with /join"), a quiet agent ("quiet: no live session to invite"), and an agent already in every channel.
