---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-404#D9]
basis:
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
  - precedent: ADR-404
  - precedent: ADR-504
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
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
- **Probes:** *Confident:* a click should do both at once, never one per click. *Not confident:* a click on an agent row should insert `@Name ` into the compose box, the same as a click on its chip in the legend did.
- **Inversion:** the ends are a chat with no navigation chrome, where channels and agents are reached by typing, and a chat whose chrome lists everything and acts on every click. This sits near the second end, with `attend.chat.sidebar_key` to hide the column.

## Context

ADR-404 D9 specified a collapsible left sidebar holding the channel tree and toggled by `attend.chat.sidebar_key`. It opened a tab's menu on a second click or a right click. The sidebar was not built. Agents appeared only in the legend row under the compose box, filtered to live sessions. The tab bar listed channels that still had a directory on disk.

## Decision

D9 is amended as follows.

**(a) Click.** A left click on a channel tab or a Channels row shows the channel and opens its menu, the same menu as the tab's. A right click still opens the menu. A click on merged does the same with merged's menu. The action slots (`≡`, `+`) are unchanged. Keyboard paths are unchanged: Ctrl or Alt plus a digit shows a tab and opens its menu when repeated, and F2 or Ctrl+T focus the tab bar.

**(b) The left column.** `attend.chat.sidebar_key` toggles it. It is shown at start and hidden automatically when the terminal is too narrow to hold it beside the feed.

- **Agents:** every agent the chat has seen, from session records and message senders. Live agents come first. An inactive agent is drawn in the muted variant. A row is a lozenge with the nickname on the agent's identity colour, the project directory on a darker step of that colour, and a state marker on the darkest step. The steps are derived from the identity colour as the status line derives them. A click on a row inserts `@Name ` into the compose box. While the column is shown, the legend row under the compose box is dropped whenever it would list agents or channels, and the column marks what Tab would complete to. The row still shows slash commands and argument hints.
- **Channels:** `#open`, then every channel with a directory or a `_groups.yaml` entry, including empty ones and ones whose directory was swept. A row is a lozenge on the accent: the name, then the member count and the time since the last message. Unread counts, seals and scope nesting are drawn when D3, D4 and D5 provide the data. Until then the list is flat.
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
- agent-tui gains a pane hook for opening a tab's menu on the first click. Other panes keep the second-click rule.

## Alternatives Considered

- **Keep D9's second click.** Two clicks for one intent. Reported as unexpected.
- **One combined list of channels with members nested under them.** An agent in several channels repeats, and an agent in none has no place.
- **Keep the legend row and add the column.** It shows the same agents twice and costs a feed row.
