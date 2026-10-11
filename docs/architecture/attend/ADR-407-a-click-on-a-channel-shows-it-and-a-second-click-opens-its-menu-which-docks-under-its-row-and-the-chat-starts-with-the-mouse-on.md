---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-405#Decision, 'ADR-504#Note (2026-10-02): a screen that is not a tree is a pane inside `App`']
basis:
  - operator: aaronsb
    level: directed
    said: "currently, left clicking on a channel in 'attend-chat' does not bring up the context dialog. also, I think it would be nice for that dialog to dock on the next line under the clicked channel. the ideal path is first click activates that channel (selects it to foregrond) and the second click (essentially, clicking on the active/foreground channel) brings up the context dialog. I can use ctrl+4 (for the fourth marked channel) to bring to foreground and use ctrl+4 a second time to bring up the context menu now on the keyboar"
    via: chat, relayed by the main session, 2026-10-11
  - evidence: "The operator's screenshot of the failing click showed `mouse off (M-m)` in the bottom bar, relayed by the main session, 2026-10-11. attend.chat.mouse defaulted to false, so a terminal never reported the click to the chat."
  - precedent: ADR-405
agent:
  name: Claude
  model: claude-opus-5-5
status: accepted
date: 2026-10-11
deciders:
  - aaronsb
related:
  - ADR-404
  - ADR-405
  - ADR-504
---

# ADR-407: A click on a channel shows it and a second click opens its menu, which docks under its row, and the chat starts with the mouse on

## Summary

- **Decided:** in attend-chat a click on a channel, on the tab bar or in the left column, does what Ctrl+digit does. The first click shows the channel. A click on the channel already shown opens its menu. Mouse and keyboard reach this through one rule. A menu docks on the line under the row it belongs to. The chat starts with the mouse on.
- **Trades away:** ADR-405's single click that both showed a channel and opened its menu. Plain drag-to-select and middle-click paste now need Shift, or Alt+m.
- **One-way?** No. This is input handling and one default, and `attend.chat.mouse: false` restores the old start.
- **Probes:** *Confident:* a click on the channel you are already reading is how you ask for its menu. *Not confident:* Shift-drag for selecting text is an acceptable cost of having the mouse on at start.
- **Inversion:** one end is a click that only navigates, with menus behind a right click; the other is a click that always opens a menu. This sits between them, as the keyboard already did.

## Context

ADR-405 made one click on a channel show it and open its menu. The keyboard kept a two-step rule: the first Ctrl+digit shows a tab, and the second opens its menu. The operator asked for the click to follow the keyboard's rule. Menus opened centred on the screen, away from the row clicked.

The mouse started off (`attend.chat.mouse` defaulted to false), so the terminal kept its clicks for text selection and the chat never saw one. The tests and the screenshot script feed mouse bytes to the chat whether or not the terminal would send them, so none of them could catch this.

## Decision

**(a) One rule for click and key.** A left click on a channel tab, or on a channel's row in the left column, takes the path Ctrl+digit takes. If the channel is not shown, it is shown and no menu opens. If it is shown already, its menu opens. Alt+digit, where Ctrl+digits do not arrive, follows the same rule. Enter on a channel row in the column does the same, and the cursor stays on the row so a second Enter opens the menu. A channel with no tab (one whose directory was swept) cannot be shown, so a click on its row opens its menu at once. A right click still opens a tab's menu without showing it.

**(b) Agent rows keep ADR-405's rule.** An agent has no foreground to show, so a click on an agent's row opens its menu at once.

**(c) Docking.** A menu docks under the row it belongs to: a channel's or agent's row in the left column, else its tab on the tab bar. The common `≡` menu and merged's menu dock under their tabs. A menu opened from another menu docks where that one did. The placement rule:

1. On the line under the row, starting at the row's left edge, as wide as its title and items need.
2. Where the lines below cannot hold it, it ends on the line above the row, so the row stays in sight.
3. Where neither side can hold it, it sits at the foot of the pane.
4. It moves left as far as it must to end inside the screen, and is never wider or taller than the pane.

**(d) The mouse starts on.** `attend.chat.mouse` defaults to true. Shift-drag selects text in most terminals while the mouse is on, and Alt+m gives the terminal its mouse back for plain selection and middle-click paste. A test runs the binary on a pseudo-terminal and checks that it asks the terminal for clicks at start.

**(e) The question guard.** A click on a row of the column drops a question waiting for `y` before anything opens. A menu closed unchanged puts "unchanged" over the bottom bar, and a later `y` must not answer a question that is out of sight.

## Consequences

### Positive

- The operator's mouse habit and keyboard habit give the same result.
- A menu opens next to what was clicked, and the clicked row stays visible.
- A fresh install answers clicks without a setting.

### Negative

- Two clicks reach a channel's menu where ADR-405 took one.
- With the mouse on, selecting text takes Shift-drag. A terminal without that passthrough needs Alt+m first.

### Neutral

- agent-tui drops `Pane::menu_on_click`, which only the chat used. It gains `Open::Tab`, for a pane row that stands for a tab, and `Pane::take_anchor`, for the row a menu docks under.

## Alternatives Considered

- **Keep ADR-405's single click.** The operator reported it as the defect.
- **Dock under the tab on the tab bar for every channel menu.** The column is where the operator clicks, so the menu would open away from the click. The tab is used only when the column does not show the channel.
- **Keep the mouse off at start and say so on the bar.** The bar already said `mouse off (M-m)`, and the click still went unnoticed.

## Amendment (2026-10-11): a menu docks where its input came from

Review of the change refined (a) and (c). This note records the result; the decision text above stands as accepted.

- **Where a menu docks follows the input.** A click or right click on a tab docks the menu under the tab, even where the left column shows that channel's row. Enter on the focused tab bar does the same. A click on a column row docks under that row. Keyboard input from elsewhere (Ctrl+digit, Alt+digit, Enter on a column row) docks under the channel's column row when the column shows it, and under its tab otherwise. The placement rule in (c) is unchanged. A row that a resize has left outside the pane is clamped into it.
- **`attend.chat.tabs.menu_on_repeat: false` governs keys only.** With it off, a second Ctrl+digit or Alt+digit, or a click on the tab already shown, only shows the tab. A click on the shown channel's column row, or Enter on that row, still opens its menu, since the row has no other action on the channel already shown.
- **A click in the column moves its keyboard cursor.** While the column has the keyboard, a click on one of its rows puts the cursor on that row.
