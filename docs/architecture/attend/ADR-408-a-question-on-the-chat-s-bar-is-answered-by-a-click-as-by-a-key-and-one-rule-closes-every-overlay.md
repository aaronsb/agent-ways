---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-405#Decision, ADR-407#Decision]
basis:
  - operator: aaronsb
    level: guided
    said: "we should go through the feature list of attend-chat and make sure it's consistent in application of ui/ux conventions (I see the nav bar for agents and channels). its kind of expected that the mouse click should work on elements"
    via: chat, relayed by the main session, 2026-10-11
  - operator: aaronsb
    level: guided
    said: "(and by consistent I mean - build the capabilities that are missing and make sure the ones implemented don't have inconsistent implementation)"
    via: chat, relayed by the main session, 2026-10-11
  - precedent: ADR-405
  - precedent: ADR-407
  - precedent: ADR-504
agent:
  name: Claude
  model: claude-opus-5-5
status: accepted
date: 2026-10-11
deciders:
  - aaronsb
related:
  - ADR-405
  - ADR-407
  - ADR-504
---

# ADR-408: A question on the chat's bar is answered by a click as by a key, and one rule closes every overlay

## Summary

- **Decided:** a pane's question that waits for `y` is the shell's own confirm. The bottom bar shows the question and its two answers as lozenges, and a click on an answer gives that answer, as its key does. Every overlay over a pane closes on Esc or on a click outside it, and closing says nothing. A click that closes an overlay and lands on the tab bar or on the chat's left column then does what it would have done with nothing open.
- **Trades away:** `n` no longer types into the draft while a question waits; it answers. A click in the feed while a question waits answers no rather than leaving the question armed.
- **One-way?** No. This is input handling and drawing in agent-tui and attend-chat; nothing on disk changes.
- **Probes:** *Confident:* a question drawn with its answers should take a click on them. *Not confident:* a click on the tab bar or the column that closes a menu should also act there, rather than only close it. The agent chose this default; it is how desktop menus behave.
- **Inversion:** one end is overlays that swallow every click outside them, so each one needs its own dismissal before anything else works; the other is overlays that never block, so a stray click acts behind them. This sits between them: a click outside always closes, and acts only on the two navigation surfaces.

## Context

ADR-405 made every menu item that drops something ask for `y` first. The chat drew that question as text on the bottom bar and answered it only from the keyboard. A click on the bar dropped the question, and a click in the feed left it armed out of sight. The shell already drew its own confirms with clickable answers. Two implementations of one behaviour had drifted apart, and only one of them answered the mouse.

Closing an overlay also varied. A pane's menu closed on Esc saying "unchanged", a word from the settings tree, where a picker edits a value. A click outside a menu closed it, and a second click was needed to reach the tab or row the operator had aimed at. The compose box lent to a new channel's name stayed lent after a click elsewhere.

## Decision

**(a) One question.** A pane asks through `Open::Ask { id, question, yes, no }`. The shell owns the question while it waits: the bottom bar draws the question, cut short where it must be so the answers fit, then ` y <yes> ` and ` n <no> ` as lozenges recorded as click targets. The answer reaches the pane through `Pane::picked` under `id`.

- `y`, or a click on the yes lozenge, answers yes.
- `n`, Esc, a click on the no lozenge, a click of any button anywhere else, or any other key answers no.
- After a no, a character the pane types and a key the shell takes (F1, F2, Ctrl+digit, Alt+m) go on as if nothing were asked. Enter and the editing keys only answer, so an Enter meant for the question never sends a draft.
- A paste over the question answers no, then lands in the compose box.
- The chat's own question state and its key handling for `y` are removed. Its Quit question, asked where Ctrl+3 arrives as Esc, is the same `Open::Ask`, and its yes asks the shell to quit through `Open::Quit`. A quit from a click ends the session through `App::ended`, since a click has no return value to end it with.

**(b) One closing rule.** Over a pane, the menu, the picker, the question, the key help, the response modal and the compose box lent to a name or description each close on Esc or on a click outside them. Closing says nothing on the bar. The settings tree's own picker still says "unchanged", since there it reports that a value was left as it was.

**(c) Clicks that go on.** A left or right click that closes an overlay goes on to act when it lands on the tab bar, or on a part of the pane the pane names through `Pane::click_through`. The chat names its left column. Elsewhere such a click only closes. A click on the tab bar or the bottom bar also gives a lent compose box back (`Pane::clicked_outside`).

ADR-405's rule that a destructive item asks first stands. ADR-407 (e), the question guard, now holds by construction: the question lives in the shell, so a click on a column row answers it no before the row acts, and no later `y` can answer a question out of sight.

## Consequences

### Positive

- Every destructive menu item can be finished with the mouse.
- A key and a click on an answer run the same code and land in the same state.
- Reaching another channel or tab with a menu open takes one click.

### Negative

- A click intended only to dismiss a menu that lands on a tab or a column row also acts there.
- `n` typed while a question waits is not kept in the draft.

### Neutral

- agent-tui gains `Open::Ask`, `Open::Quit`, `Pane::click_through`, `Pane::clicked_outside` and `App::ended`. The other panes, `ways introspect` and `ways projects`, keep their defaults: nothing passes clicks through.
- Esc with nothing open is unchanged here. Whether it quits is a separate decision.

## Alternatives Considered

- **Make the chat's bar text clickable in place.** It would keep two implementations of one confirm, which the operator asked to avoid.
- **Swallow every click outside an overlay.** Each overlay would cost a click before the next action, which is what the operator reported as unexpected.
- **Pass every outside click through, including the feed.** A click to dismiss a menu would select a message behind it.
