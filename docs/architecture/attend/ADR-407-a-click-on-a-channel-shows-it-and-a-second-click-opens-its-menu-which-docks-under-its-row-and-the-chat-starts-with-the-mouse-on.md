---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-405#Decision, 'ADR-504#Note (2026-10-02): a screen that is not a tree is a pane inside `App`']
basis:
  - operator: aaronsb
    level: directed
    said: "new fix for attend-chat: the merged channel needs to be called something different like ('combined' or full stream or something) - because it's all the chats together. Next, I don't want the merged channel to be actually a channel - it's an operator view for attend-chat, and should be justified right side, maybe with some characters that indicate 'merged chat' (no more than 3-4). That tab should be a different color (lighter? shaded? not sure). The chat box itself needs to be one of the tabbable elements. the text entry box swallows normal keyboard commands (including shift and tab) because it's the entry panel. when the chat box is active regular tab is autocomplete. whenever the entry box is active, we have a little legend update that indicates that shift+tab or arrow can be used to leave the box. nav arrows (up down left right) work inside the box, but if the up arrow is used when the cursor is at the beginning of the first line of the entry box, it is the same as leaving focus for the entry box and allows navigation of elements. same goes for the last position of the last line, down arrow exits the pattern. we need to define the chain of elements that tab (or shift tab, or left and right arrows) do for navigating the elemnts outside of chat. we need every agent entry and channel to be in that loop. we also need the hints on the bottom to reflect the context of where the active cursor is at, and we can use enter to raise the context menu for any element. (the same context menu that the mouse click affords). when not in the text entry box, we should be able to use all the characters for contorl that the help (F1) claims. If we're not deliberately in chat, the lighter highlight around the entry box for text is not brighter as it is now, but instead the lower border color.. we could say that this is a nav polish pass"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "currently, left clicking on a channel in 'attend-chat' does not bring up the context dialog. also, I think it would be nice for that dialog to dock on the next line under the clicked channel. the ideal path is first click activates that channel (selects it to foregrond) and the second click (essentially, clicking on the active/foreground channel) brings up the context dialog. I can use ctrl+4 (for the fourth marked channel) to bring to foreground and use ctrl+4 a second time to bring up the context menu now on the keyboar"
    via: chat, relayed by the main session, 2026-10-11
  - operator: aaronsb
    level: directed
    said: "Merge #953, then A→B→C (Recommended)"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "since we're changing things a bit, let's avoid work with low value by adhering to golden states"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "I want to point out that we should, before compaction, try to add an escape valve for golden repro - until we're sure of the flow/layout etc we should be more sparse in recording/checking golden layouts. we could call that a rapid iteration phase"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "otherwise we're making changes and then the repo recording path is essentially treating that as 'ok we've finally got it' when actually we haven't - as you can see by the number of polish suggestions I keep coming up wiht"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "active-channel tab, numbered column, Esc cycle"
    via: "the operator's work-in-progress commit c565165c on spec/esc-cycle-wip, relayed by the main session, 2026-10-12"
  - operator: aaronsb
    level: directed
    said: "The chat box is reserved for chatting, not a general entry interface."
    via: chat, relayed by the main session, 2026-10-12
  - evidence: "The operator's screenshot of the failing click showed `mouse off (M-m)` in the bottom bar, relayed by the main session, 2026-10-11. attend.chat.mouse defaulted to false, so a terminal never reported the click to the chat."
  - precedent: ADR-405
  - operator: aaronsb
    level: directed
    said: 1. digits only work on channel rows. 2. a. 3. agree with new behavior 4. keep 5. keep 6. keep the mark and add count on tab. 7. thats correct as built 8. agree- its a single char column and makes sense. 9. correct. 10. correct.
    via: 'the operator''s answers to the design questions in PR #972, given to the head session, 2026-10-13'
  - operator: aaronsb
    level: directed
    said: "left click on agent - open its dms. a second click (when the dms are open) opens its menu"
    via: "the operator's answer in the head session, 2026-10-13, to the #993 review's question on the agent-row left click"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "we can use enter to raise the context menu for any element. (the same context menu that the mouse click affords)"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    said: "List is the switch (Recommended)"
    via: "the review of PR #963 (option: Frames named in the committed PROVISIONAL file are never asserted or blessed, in CI too. The weakening stays visible as a reviewed file in the diff. Settled frames stay strict everywhere. Anchors for the chat: the default view at 80x25 and 120x35.), relayed by the main session, 2026-10-12"
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

Review of the change refined (a) and (c); the docking rule in (a) was the agent's call, made from the operator's report quoted above ("dock on the next line under the clicked channel"). This note records the result; the decision text above stands as accepted.

- **Where a menu docks follows the input.** A click or right click on a tab docks the menu under the tab, even where the left column shows that channel's row. Enter on the focused tab bar does the same. A click on a column row docks under that row. Keyboard input from elsewhere (Ctrl+digit, Alt+digit, Enter on a column row) docks under the channel's column row when the column shows it, and under its tab otherwise. The placement rule in (c) is unchanged. A row that a resize has left outside the pane is clamped into it.
- **`attend.chat.tabs.menu_on_repeat: false` governs keys only.** With it off, a second Ctrl+digit or Alt+digit, or a click on the tab already shown, only shows the tab. A click on the shown channel's column row, or Enter on that row, still opens its menu, since the row has no other action on the channel already shown.
- **A click in the column moves its keyboard cursor.** A click on one of its rows puts the keyboard's cursor on that row, from wherever the keyboard was (ADR-405, Keys).

## Amendment (2026-10-12): Enter on a focused element opens its menu at once

The operator asked that "we can use enter to raise the context menu for any element. (the same context menu that the mouse click affords)". Enter on a focused tab, column row or the feed now opens that element's menu at once, as a right click does, through the same activation path; it no longer shows a channel first. A menu opened from the tab bar docks under its tab. Moving the tab bar's focus onto a channel tab still shows it, so a channel is shown from the keyboard by walking to it. The digits number the created channels from 1; the reserved tabs carry none, and 0 is the bar's last tab (ADR-405, amendment of 2026-10-12).

## Amendment (2026-10-12): one interaction state, moved by one pure step

The operator chose the order of the work that follows the review of the chat's focus handling: "Merge #953, then A→B→C (Recommended)". This amendment records A, a refactor with no change in behaviour. B replaces the shell's overlay hooks with a contract in agent-tui. C changes what Esc does. The operator set the bar for A: "since we're changing things a bit, let's avoid work with low value by adhering to golden states". No golden frame under `tools/attend-chat/tests/golden` changes. A golden that would need blessing again is a defect in the refactor.

- **One value.** The chat's interaction state is one value, `Ix` (`tools/attend-chat/src/app/ix.rs`). It holds where the keyboard is, the tab the walk along the tab bar is previewing over (`preview_from`), the column row under the focus, and the overlay the shell last reported. The flags `overlaid`, `bar_entry` and `side_focus_hit` are gone. `owns_text`, `overlaid` and the focused tab the shell reads are derived from `Ix`, and so are the renderer's focus marks and the footer.
- **One step.** `step(ix, ev, view) -> (Ix, effects)` is a pure function. Every input that can move the focus reaches it: keys in and outside the compose box, a paste, F2 or Ctrl+T, a click in the pane or one the shell takes, a tab's digit, a tab activated, a world read again, the column hidden, and the overlay up or down. It returns the effects in order: type, move the caret, send, complete, clear, page, show a tab, put the previewed tab back, activate a target, the feed's menu, reveal or deselect a message, follow a row, redraw. The tab is put back before anything is typed or sent. The chat's dispatcher is the only writer of `Ix`, and the direct writes of the focus that bypassed the old `focus_on` are gone.
- **The world is read when a step needs it.** The view carries the tab bar and the column only when the world has been read. A step that needs them and does not have them asks for the world. The dispatcher then reads it, which refocuses, and steps again. The chat reads the world at the same points as before, so a key that only types does not read it.
- **The overlay has one kind for now.** `Pane::overlaid` reports only that an overlay is up, so `Ix` records one kind of overlay. B's contract reports which overlay is up and adds the kinds: a menu, an entry, a question, the help, the guard. B also decides whether `Ix.overlay` mirrors or owns the pane's own menus. Those menus are a second record of what is open, outside `Ix`: `ChatPane::open` is written in about 13 places in `menu.rs`, and the dispatcher patches a menu's anchor after `Activate`.
- **Evidence.** Unit tests cover each cell of the interaction table and need no terminal. Property tests over seeded random sequences check three things: the preview is never held off the bar, the overlay is what the shell last reported, and a printable key lands only in the compose box, after the previewed tab is put back. A differential run drove the chat on main and on the refactor through 600 seeded sequences of 200 keys, clicks and pastes, through the shell's real handlers. Both recorded identical state and frames at every step. The run leaves the world unchanged after setup: it pushes no signals, makes no `stale()` call, and sends no wheel or resize. So it does not exercise refocus after a refresh. Two planted defects showed this: refocus landing on the old row index, and the follow effect dropped from a row move. The differential run missed both, and the unit tests caught both.


## Amendment (2026-10-12): the chat's goldens are in a rapid iteration phase

The operator asked for "an escape valve for golden repro - until we're sure of the flow/layout etc we should be more sparse in recording/checking golden layouts. we could call that a rapid iteration phase". The reason, in the operator's words: "otherwise we're making changes and then the repo recording path is essentially treating that as 'ok we've finally got it' when actually we haven't". Recording a golden asserts that the layout is settled. The chat's layout is still taking polish feedback each round.

- **The list is the switch.** The review of #963 offered two designs. The operator chose "List is the switch (Recommended)", which read: "Frames named in the committed PROVISIONAL file are never asserted or blessed, in CI too. The weakening stays visible as a reviewed file in the diff. Settled frames stay strict everywhere. Anchors for the chat: the default view at 80x25 and 120x35."
- **Provisional frames.** A frame named in `tools/attend-chat/tests/golden/PROVISIONAL` is never asserted and never blessed by agent-tui's golden check, in any run, CI included. A drift or a missing golden prints one note. A listed name with no `.frame` file that no check reaches prints a stale-entry warning. Every other frame is settled and always asserted.
- **The anchors.** The two settled frames are the default view: the chat with its left column shown, as it starts, at 80x25 (`sidebar-80x25`) and at the suite's reference size (`sidebar-120x40`). The suite has no 120x35 size. Its reference size is 120x40, so that frame stands for the operator's "120x35". Every other chat frame is listed.
- **What carries behaviour.** The table and property tests of `step` (amendment above) carry the chat's interaction behaviour.
- **Leaving the phase.** The phase ends when the operator accepts the layout. Each provisional frame is then removed from the list, blessed and reviewed.
- **The refactor bar stands.** A change with no change in behaviour still leaves every settled golden byte-identical.

## Amendment (2026-10-12): the shell asks one question and reports each overlay

This records B of the order the operator chose ("Merge #953, then A→B→C (Recommended)"): the agent-tui contract under the interaction state. It changes no behaviour. The bar for A stands: no golden frame changes, settled or provisional.

- **One routing question.** The shell asked a pane four things to route a key: `owns_text`, `owns_focus` with `focus_tab`, and `esc_quits`. It now asks one, `Pane::keymap_ctx() -> Ctx { text, digits, owns_bar, esc_quits }`. `text`: the pane types every plain key. `digits`: beside no text, a plain digit shows a tab. `owns_bar`: the pane holds the keyboard focus, the tab bar's included. `esc_quits`: Esc the pane passes on quits. The default is the shell's old default: no text, digits, no bar, Esc quits. The chat answers from `Ix::ctx`, given the bar as the world laid it out: text in the compose box, digits on a tab slot the bar draws, the bar its own, Esc never quits (ADR-409). `focus_tab` stays, as where the bar draws its cursor.
- **Overlay events.** `Pane::overlaid(bool)` is gone. Before each frame the shell compares what is open with what it last told the pane and calls `Pane::overlay_closed` for each overlay that closed, top down, then `Pane::overlay_opened` for each that opened. An overlay has a kind: `Menu { id }`, `Entry { id }` and `Ask { id }` carry the id the pane opened it under, and `Help`, `Guard` and `Report` (the response modal) carry none. An entry dialog the exit guard holds stays under the guard in the stack. The pane hears of a change at the frame where it heard `overlaid` before.
- **Ix owns the stack.** In the chat the two calls reach `step` as `OverlayOpened` and `OverlayClosed`, and `Ix` keeps the stack. It is the one record of what is open, the chat's own menus included, by their ids. `ChatPane::open` is renamed `to_open`: it holds a request until the shell takes it, and a request the shell refuses never enters the stack. The pane redraws when the stack goes from empty to not empty or back, at each event. One overlay replacing another between two frames passes through an empty stack, so the pane draws once more than before; what it draws is the same.
- **One writer, by construction.** The fields of `Ix` are private to `ix.rs`. The chat holds its `Ix` in a `Machine`, whose only writing method runs `step`, and `ChatPane::dispatch` is its only caller. Code outside `ix.rs` can read an `Ix` and clone one, and the compiler rejects any write to its fields. `Machine` has no `Default`, so `std::mem::take` cannot reset it. The compiler cannot stop the owner from replacing the whole value with `Machine::new()`; only `ChatPane::new` calls it. The one test that wrote `Ix` directly now clicks the row.
- **The layout is kept.** The tab bar and column a step reads (`Laid`) are built with the world and kept with it. Showing a tab updates the tab shown in place. A dispatch no longer rebuilds the bar and the column order.
- **The column hidden.** `toggle_sidebar` sends `ColumnHidden` only when it hides the column. On show the event did nothing.
- **Evidence.** The shell's reports have a test of their own in agent-tui: a recording pane checks the exact sequence of openings and closings for a menu closed, a menu replaced by its question within one frame, an entry under the guard and back, and the key help closed by Esc. A shell that never reports a close fails it. The differential trace is `tools/attend-chat/tests/ix_trace.rs`. It adds what A's run lacked: messages arriving through the watcher, a third of them from senders the chat has not seen, which reads the world again and refocuses; the wheel over the column and the feed; twenty more agents in the column, so its lists overflow at both sizes; and a digest of each frame's cell styles, so the caret, a focused border and the focus marks count as well as the text. CI asserts a pinned digest of twenty seeds of 200 steps. A change in behaviour changes it, and re-pinning it is a reviewed change; the full trace, recorded on two revisions and compared, shows what moved. Over 300 seeded sequences of 200 steps, the base and this change recorded identical state, text and styles at every step. Both defects that A's run missed (refocus onto the old row index, the follow effect dropped from a row move) change the full trace. A shell that never reports a close, and Esc that leaves the key help open, change the pinned digest. Every golden frame the suite renders, the 38 provisional ones included, is byte-identical to the committed file.

## Amendment (2026-10-12): C, the operator's navigation redesign

This records C of the order the operator chose ("Merge #953, then A→B→C (Recommended)"). A and B changed no behaviour; C changes it, to the design in the operator's work-in-progress commit, "active-channel tab, numbered column, Esc cycle". It is built on `Ix`, `step` and `Ctx`.

- **One channel tab.** The tab bar holds `≡`, the created channel shown with its description, `+`, then `#open` and `⋯all` at the right. No tab is drawn per channel. While `#open` or the operator view is shown, the left of the bar holds no channel tab. The description moves from the trailer after the tabs into the tab, and the bar cuts it first when it is short of room. The created channel shown carries its member count after its name, the operator included, ahead of the description so the cut leaves it; `#open` holds everyone and carries none. While the column is hidden, by its key or for want of width, each channel's tab carries the column's number before its name, so Ctrl and Alt with a digit stay on screen; the operator view carries none. The tabs follow the width of the frame being drawn: agent-tui's `Pane::layout` tells the pane its area before the tab bar asks for the tabs. Walking the bar shows nothing. With one channel tab, a walk that showed `#open` would take the channel's tab off the bar under the cursor, and Tab and Shift+Tab would no longer retrace each other. Enter opens a tab's menu, and a click on a tab shows it, as before.
- **A numbered column.** The column numbers its channels from 1, `#open` first, up to 9. Ctrl+n, or Alt+n where Ctrl+digits do not arrive, picks channel n as a click on its row picks it: the channel is shown, and on the channel shown its menu opens, as `attend.chat.tabs.menu_on_repeat` allows. 0 is the operator view. Agents carry no number. A plain digit is the column's number only while the focus is on a channel's row; on an agent's row, as everywhere else, it is typed in the box, as any other character outside the chat is, and the footer there names no digits. agent-tui gains `Pane::own_numbers`, which names what a pane numbers in place of its tabs ("channel"), and `Pane::jump`, which the shell calls with the number. The tab bar then draws no numbers, and the footer names the pane's noun.
- **The chat is the compose box and the feed.** `Ctx.text` holds in both, so `q`, `m`, `?` and the digits are typed there and are the shell's only on the tab bar and the column. A printable key in the feed goes to the compose box and is typed there. In the operator's words: "The chat box is reserved for chatting, not a general entry interface." Slash commands stay in it.
- **The Esc cycle.** Esc moves along one cycle: the compose box, the feed, the `≡` menu, the tab or column row the focus was last on (the feed when there is none), then the `≡` menu again. The draft is kept throughout. An overlay that is open takes the first Esc. In the box Esc goes to the feed whether or not it holds a draft. `Ix` remembers the last tab or row the focus was on. Leaving the box onto `≡` by Tab, or by ↓ at the end of its last line, is not a visit: the cycle comes back to the tab or row focused before, or to the feed when there was none. Outside the box Esc opens the `≡` menu, docked under `≡`, with the focus on `≡`. The shell reports a menu the pane opened in answer to Esc as `Overlay::Menu { by_esc: true }`. When that menu closes with the focus still on `≡`, by Esc, a click outside or an item picked, `step` moves the focus back to the remembered tab or row, found by what it is, or to the feed when it is no longer drawn: its channel or agent gone, its tab off the bar, or the column hidden, Hide column picked from that menu included. A click that moved the focus first keeps it where the click put it. The shell tells the pane what opened and closed before it draws the tab bar, so the bar's cursor moves in the same frame. Esc after any menu has closed is the cycle's next stop, so a double Esc outside the box ends on the `≡` menu; nothing quits. Where the terminal lacks the kitty keyboard protocol Ctrl+3 arrives as Esc and is that stop, and an Alt+digit split across two reads is an Esc stop then a typed digit; neither loses anything.
- **Enter opens a menu everywhere outside the box.** On the feed it opens the selected message's menu, docked under its header: Reply (the message's channel shown and the keyboard in the box; a direct message is answered by addressing its sender), Quote (its lines put in the box as `> ` lines), Address @Name, and Copy. Copy puts the message on the terminal's clipboard by OSC 52, through agent-tui's `Open::Copy`, cut to its first 74 kB, which base64 makes about 100 kB, past which many terminals drop the copy whole; the status line says when it was cut. The menu names its message by the message's id, so a message the feed has dropped since the menu opened is acted on no more. ↑ and ↓ select the message.
- **Tab and Shift+Tab are mirrors.** In the box Tab completes only when completing would change the box's text: a command, a subcommand, an `@agent` or a `#channel` that something matches, or a completion cycle with another candidate to move to. A word nothing matches, or a cycle of one, completes nothing, and Tab leaves the box forward, as Shift+Tab leaves it backward. While Tab is completing, the walk does not pass through the box. Tab on an empty line no longer cycles the channels; the column's numbers do that.
- **Who is in the channel shown.** The column marks the agents in the created channel shown, the operator included, at the left of their rows, as it marks the channel shown. `#open` and the operator view hold everyone, so nothing is marked while they are shown. The channel's tab carries the count (above).
- **Evidence.** The interaction table gains cells for the Esc cycle, the column's numbers and Tab's two meanings in the box, and a table test: from every element of the ring, N × Tab then N × Shift+Tab returns to it, for every N up to the ring's length. The same test runs through the shell's real keys in `tools/attend-chat/tests/focus.rs`, beside integration tests of the Esc cycle, the message menu and the digits. agent-tui's overlay test covers `by_esc`, and new tests cover a pane's own numbers and the OSC 52 copy. The pinned trace digest changed, as a change in behaviour changes it, and is re-pinned in the same change. The operator's answers to the questions of #972 add cells for a digit on an agent's row and for Tab out of the box, focus tests for both and for the tab's member count and its numbers with the column hidden, and a re-pinned trace. The two settled anchors, `sidebar-80x25` and `sidebar-120x40`, change on the tab bar's row, the column's channel rows and the footer, and are blessed in the same change with each changed row accounted for in the review.

## Amendment (2026-10-13): an agent's DM view on the one rule, the ring and the Esc cycle

ADR-405's amendment of the same date adds a DM view per agent, listed on a row under the agent's. It changes (b) above and adds nav elements to the redesign's ring.

- **(b) is replaced for claudes.** A click on a claude's row follows the rule in (a): the first click shows the agent's DM view, and a click on the agent whose view is shown opens its menu. The operator confirmed it: "left click on agent - open its dms. a second click (when the dms are open) opens its menu". A click on the DM row, or on the view's tab, shows the view, and on the view shown opens the view's menu. A human's row has no view and keeps (b): a click opens its menu at once. A right click opens the menu at once on every row, and Enter on a row opens its menu, as the amendment of 2026-10-12 says.
- **The ring.** The DM row is a stop of the ring, directly after its agent's row; the view's tab is a stop on the bar, in the place the created channel shown takes. Tab and Shift+Tab stay exact mirrors with a view open. ↓ from the view's tab lands on its row. A digit on the DM row is typed in the compose box, since the row carries no number, and the column's numbers count channels only, past every agent and DM row.
- **The Esc cycle.** The DM row and the view's tab are remembered like any row or tab: Esc from the DM row opens the `≡` menu, and the next Esc comes back to the row. When the view is hidden with the keyboard on its row, the keyboard goes to its agent's row.
- **Evidence.** The interaction table gains cells for the DM row (the arrows, ↓ from its tab, the numbers passing over it, Enter, a click, a digit) and for a hidden DM row handing the keyboard to its agent; the mirror test runs with a view shown, and the random views include DM rows and tabs. `tools/attend-chat/tests/dm_view.rs` drives the same through the shell's real keys and mouse. The pinned trace digest moves for three reasons only: a left click on an agent's row now shows a view where it opened a menu, the agent's menu has Show DMs as its first item, one row taller, and the key help's click line reads `shows a channel or agent's DMs, again its menu; flag: menu`. The settled anchors `sidebar-80x25` and `sidebar-120x40` are unchanged.

