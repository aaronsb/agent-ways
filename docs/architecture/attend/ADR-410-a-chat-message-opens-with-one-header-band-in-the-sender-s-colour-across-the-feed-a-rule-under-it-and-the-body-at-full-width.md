---
contract: adr/v1
kind: decision
verb: change
capability: attend
amends: [ADR-405#Decision]
basis:
  - operator: aaronsb
    level: directed
    said: "I'd like to 1) use our colored band that we use for agents as a single line spanning the width of the message."
    via: chat, relayed by the main session, 2026-10-11
  - operator: aaronsb
    level: directed
    said: "It's the activity aparkline from status line. It does not really make sense to have it in a message history and I should have left it out."
    via: chat, relayed by the main session, 2026-10-11
  - operator: aaronsb
    level: guided
    said: "the lozenge curved edges come from the 'statusline' for claude code"
    via: chat, relayed by the main session, 2026-10-11
  - precedent: ADR-405
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
  - ADR-504
---

# ADR-410: A chat message opens with one header band in the sender's colour across the feed, a rule under it, and the body at full width

## Summary

- **Decided:** each message in attend-chat's feed starts with one header line across the feed. The line is a lozenge band in the sender's colour, the same steps and shape the left column's agent row wears. It holds the sender's channel glyphs and the time, then the name, then the project. A rule runs under it, and the body runs below at the feed's full width. The boxed card beside the body is gone, and no activity sparkline is drawn.
- **Trades away:** the card's fixed left column, so the eye no longer scans names down one edge. Every message costs one row more than its body, plus the rule.
- **One-way?** No. Drawing only; nothing on disk changes.
- **Probes:** *Confident (band):* one band per message, coloured as the column colours the agent. *Not confident (order):* the time first on the darkest step, then the name and the project, as the operator's mockup has it.
- **Inversion:** one end is a dense log line per message (time, name, text on one row); the other is a card per message. This sits between: a header line and a body, so a long body reads at full width and the sender is still one glance.

## Context

ADR-405 gave the left column an agent row drawn as a lozenge: the name on the identity colour, the project on a darker step, a mark on the darkest, in the status line's shapes. The feed still drew each sender as a bordered card of three lines beside the body, which narrowed every body by the card's width and drew the sender a second, different way.

The operator asked for the agents' coloured band as one line across the message, with the rounded caps the status line draws, and without the activity glyph their mockup had carried over from the status line.

## Decision

- **The header.** One line per message, as wide as the feed. It is a lozenge in the shape the chat draws every lozenge in (`theme.shape`: round, flame, arrow, slant, pixel or plain). Its segments wear the column's steps for that sender (`identity_styles`): the channel glyphs of the sender's memberships and the time on the darkest step, the name on the identity colour, the project on the step between. A sender the column sets back is set back here. A human's name keeps its identity style (bold, italic). At 16 colours the name keeps its ANSI colour and the rest is set back; without colour the segments are reverse video and no glyph is drawn.
- **Width.** The time segment is never cut. When the row is short, the name and project are cut, the project first, and then left out. When the row is long, the project segment fills the rest of the row, so the band spans the message.
- **The rule and the body.** A rule runs under the header, and the body is wrapped to the feed's full width below it. A selected message's rule is drawn heavy (`━`) in the accent.
- **Targets.** A click anywhere on a message selects it. A right click on its header opens the sender's menu, as ADR-405's agent row does.

## Consequences

### Positive

- A body gets the feed's whole width; long messages wrap far less at 80 columns.
- The sender is drawn one way across the screen: the column's row and the message header are the same lozenge.

### Negative

- Short messages take more rows: header, rule, body and a blank row.
- Per-channel glyph colours inside the time segment are lost; the glyphs take the segment's colour.

### Neutral

- agent-tui's `Feed` takes header segments and a shape in place of a label and a label width, and `FeedState::entry_row` names which row of an entry a click hit.
- The sender chip no longer cuts names and projects to a card's width; the band cuts them.

## Alternatives Considered

- **Keep the card, colour its border.** It keeps the narrowed body and the second drawing of the sender.
- **Header and body on one line.** A long name pushes the body; the operator's mockup puts the body under the header.
- **Keep the sparkline glyph.** The operator said it does not belong in a message history.
