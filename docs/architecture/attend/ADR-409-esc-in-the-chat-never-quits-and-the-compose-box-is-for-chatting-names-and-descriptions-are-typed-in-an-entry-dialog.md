---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-408#Decision, 'ADR-504#Note (2026-10-02): a screen that is not a tree is a pane inside `App`']
basis:
  - operator: aaronsb
    level: directed
    said: "we should have the new channel name entry be consistent with the other context entry dialogs, not appearing in the chat box"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: directed
    said: "the chat box is reserved for chatting, not a general entry interface"
    via: chat, relayed by the main session, 2026-10-12
  - operator: aaronsb
    level: guided
    said: "we should go through the feature list of attend-chat and make sure it's consistent in application of ui/ux conventions (I see the nav bar for agents and channels). its kind of expected that the mouse click should work on elements"
    via: chat, relayed by the main session, 2026-10-11
  - operator: aaronsb
    level: guided
    said: "(and by consistent I mean - build the capabilities that are missing and make sure the ones implemented don't have inconsistent implementation)"
    via: chat, relayed by the main session, 2026-10-11
  - precedent: ADR-408
  - precedent: ADR-504
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Never quits (Recommended)"
    via: AskUserQuestion, session chat, 2026-10-11; option label written by the agent, chosen by the operator
    covers: [esc-never-quits]
  - operator: aaronsb
    said: "the chat box is reserved for chatting, not a general entry interface"
    via: chat, relayed by the main session, 2026-10-12
status: accepted
date: 2026-10-11
deciders:
  - aaronsb
related:
  - ADR-404
  - ADR-405
  - ADR-407
  - ADR-408
  - ADR-504
---

# ADR-409: Esc in the chat never quits, and the compose box lent to a name says what it is lent to

## Summary

- **Decided:** in attend-chat, Esc closes what is open and clears, and never quits, with or without the kitty keyboard protocol. Quitting is Ctrl-C, which asks first over a draft, and a new Quit item on the `≡` menu, which does the same. The compose box lent to a new channel's name or a description says so where the text is typed (`new channel name >`, `describe #deploy >`) in place of `>`, and draws no destination flag. Showing a tab, opening a menu, or the column taking the keyboard gives the box back, by key as by click.
- **Trades away:** Esc as a one-key quit on an empty line. Leaving the chat takes Ctrl-C or two clicks.
- **One-way?** No. This is input handling and drawing in agent-tui and attend-chat; nothing on disk changes.
- **Probes:** *Confident (esc-never-quits):* Esc never quits; Ctrl-C and `≡` → Quit do, and a double Esc is always safe. *Not confident (esc-clears):* with nothing open, Esc clears the selected message and the status line and leaves the draft as it is. The agent chose to keep the draft, so a stray Esc never loses typing.
- **Inversion:** one end is Esc as the universal way out, which quits once nothing is left to close; the other is Esc as a pure close key with quitting elsewhere. This takes the second end, because the chat's tab key Ctrl+3 arrives as Esc in terminals without the kitty protocol, and a habitual double Esc to make sure a menu closed should never cost the session.

## Context

ADR-504's note on panes says Esc and Ctrl-C quit beside a pane that owns text, asking first over unsaved work. In attend-chat that met two problems.

- Where the terminal does not speak the kitty keyboard protocol, Ctrl+3 arrives as the same byte as Esc. The chat answered by asking `quit the chat?` on Esc with an empty line (ADR-404's implementation notes, then ADR-408 (a) as a shell question). With the protocol, an Esc on an empty line quit at once with no question. A double Esc, a common habit to make sure a menu has closed, closed the menu and then quit or asked to quit.
- The compose box could be lent to a new channel's name or a description, and only the bottom bar said so. The box kept its `>` and its destination flag. The audit's finding 3: with the box lent, a menu opened and closed, `abc` and Enter created a channel `#abc` while the flag still read `#infra`. ADR-408 (b) made Esc and a click outside give the box back, but a tab key or the column's keyboard focus left it lent and out of sight.

The audit put the Esc question to the operator: Esc on an empty compose box with nothing open could always ask, or never quit. The operator chose the option labelled "Never quits (Recommended)", whose description read: "Esc only closes overlays and clears things. Quit stays on Ctrl-C (asks) and the ≡ menu. A double Esc is always safe." What "clears things" covers, the selection and the status line but not the draft, is the agent's reading.

## Decision

**(a) Esc never quits the chat.** Esc closes the topmost overlay as ADR-408 (b) says: a menu, a picker, a question, the key help, the response modal, or the compose box lent to a name. With nothing open it clears the selected message and what the status line says. The draft stays. Where Ctrl+3 arrives as Esc it is the same key, so a tab key can never close the chat.

- The pane says so to the shell through `Pane::esc_quits`, false for the chat. An Esc the pane passes on then quits nothing, and the key help lists `^C` as the quit key in place of `Esc ^C`. Panes that keep the default, `ways introspect` and `ways projects`, still quit on Esc.
- ADR-408 (a)'s Quit question and its `stayed` answer are removed. No key asks to quit.

**(b) Quitting.** Ctrl-C quits at once with nothing unsaved and opens the exit guard over a draft, as before. The `≡` menu ends with Quit, which asks the shell to quit through `Open::Quit`: the same path as Ctrl-C, guard included.

**(c) The lent box says what it is lent to.** While the compose box is lent, its first row starts with the label in bold accent, `new channel name > ` or `describe #channel > `, in place of `> `. The label is cut to half the box so the text keeps room. The destination flag is not drawn, since Enter sends no message. Continuation rows indent by the label's width.

**(d) What gives the box back.** Besides Esc and a click outside it (ADR-408 (b)), showing a tab (Ctrl+digit, Alt+digit, Left or Right on the focused tab bar), opening any tab's or `≡`'s menu, and the column taking the keyboard (F2, then Down) give the box back with the draft it held. The `+` slot and Ctrl+N lend it. A click on a tab already gave it back, so a key and a click on a tab now land in the same state.

## Consequences

### Positive

- A double Esc is always safe, with or without the kitty protocol.
- Quitting has one path for keys and clicks, and the mouse can quit.
- The box shows what Enter will do: a message has a destination flag, and a name or description has its label.
- Finding 3 cannot recur: a tab key or the column ends the lent box before Enter reaches it.

### Negative

- Esc no longer quits an empty chat; muscle memory from other TUIs reaches for it.
- Showing a tab while typing a new channel's name drops the name typed so far. The draft from before the box was lent comes back.

### Neutral

- agent-tui gains `Pane::esc_quits`, default true.
- The `--help` text and `docs/attend-and-monitor/tui.md` describe Esc and quitting as above.
- ADR-405 and ADR-407 say nothing about Esc quitting and are unchanged. ADR-404's implementation notes describe the Esc question this record removes.

## Alternatives Considered

- **Esc on an empty line always asks.** It keeps a one-key route to quitting, but a double Esc still lands on a question and the question needs an answer before anything else works.
- **Esc clears the draft.** It would make Esc a full reset, but a stray or doubled Esc would lose typing, which is the cost this record removes.
- **Keep the lent box until Enter or Esc, and only draw the label.** It would make the state visible but let a tab change carry a half-typed name onto another channel's view, and a key would still differ from a click on the tab.

## Correction (2026-10-11): the `≡` menu, the action slots, and the shown tab

Review of the change refined (d) so that (b) and (d) hold together. The decision text above stands as accepted.

- **The `≡` menu opens over a lent box.** Its items give the box back when picked, except Quit, which leaves it lent. The shell's quit then counts a name typed there as unsaved and opens the exit guard, as Ctrl-C does, and Back returns to the lent box. Opening `≡` no longer gives the box back on its own.
- **A click on an action slot does not dismiss.** A click on `≡` or `+` acts as the slot's key does: `≡` opens its menu over the lent box, and `+` lends the box or keeps it lent, as Ctrl+N does.
- **Ctrl+digit on the shown tab** with `attend.chat.tabs.menu_on_repeat` off shows the tab again, so it gives the box back as a click on that tab does.

## Amendment (2026-10-12): the chat box is for chatting; names and descriptions are typed in an entry dialog

The operator said: "we should have the new channel name entry be consistent with the other context entry dialogs, not appearing in the chat box", and stated the principle: "the chat box is reserved for chatting, not a general entry interface".

- The compose box is never lent. The lend mechanism (decision (b)'s `new channel name >` and `describe #deploy >` leads) is removed.
- A line that is not a chat message is typed in one shared entry dialog, `agent_tui::Open::Entry`, docked where a menu docks: the new channel's name (from `+` or Ctrl+N) under the `+` slot, a channel's description (Describe…) beside its menu item, opened on the line the channel has now. Enter gives the text to the chat; a name it refuses opens the dialog again with the name and the reason inside it. Esc or a click outside closes it with nothing, and Esc never quits. While it is open it holds the keyboard; the compose box and its draft are untouched throughout.
- A name typed in the dialog is unsaved work: Ctrl-C's exit guard names it ("new channel name being typed"), and going back opens the dialog again with the name.
- Entry points moved: the new channel's name (`+`, Ctrl+N), and a channel's description (Describe…). Slash commands typed as chat stay in the compose box, since the operator typed them there. Confirmations were already the shell's question on the bottom bar (ADR-408).
