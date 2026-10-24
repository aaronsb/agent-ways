---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-311#decision, ADR-304#1, ADR-304#4, ADR-304#6, ADR-304#12, ADR-309#1]
basis:
  - operator: aaronsb
    level: directed
    said: "I don't like append only. I thought it made sense but we need to be able to edit where it makes sense too"
    via: "the kmscon session, 2026-10-24, relayed verbatim to this session by that session (kmscon-1f) at the operator's request"
  - evidence: "aaronsb/kmscon commit cfb43b3 (PR #155, docs/fork/development.md) adopted the same rule on the same words: an accepted record is edited in place where that keeps it correct and readable; an edit that changes a decision carries a dated note with the operator's words and the earlier text; appended dated sections remain for additions; git keeps every version"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "accepted should mean what it reads as in the decisions. if something was written as superceded in the adr, then that's considered history. imagine that reading the adr from top to bottom and considering its sections, the total read of a concept in the adr is what is 'accepted'"
    via: this session, 2026-10-24, answering the agent's question whether an accepted record should read as what is decided now
    covers: [edit-to-current]
  - operator: aaronsb
    said: "when things are reversed or replaced (or rejected) if it is substantially the same topic then I'd rather alter the existing adr. if it is a different direction or concept or less than half overlapping (thats a subjective call but acceptable) then it's a new record. The reason for this is a blanket 'new append' policy would create a bueacratic mess."
    via: this session, 2026-10-24, answering the agent's question whether a reversed or replaced decision should get its own record; the answer changed Decision 4
    covers: [new-record-line]
status: accepted
date: 2026-10-24
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 309
  - 311
---

# ADR-313: An accepted record is edited in place where that keeps it correct, with a dated note where an edit changes a decision

## Summary

- **Decided:** an accepted record is edited in place where that keeps it correct and readable, so its text says what is decided now. An edit that changes what a record decided carries a dated note at the edited passage, with the operator's words and what the text said before. Appended dated sections remain for evidence, corrections and amendments that add to a record. A decision reversed, replaced or rejected on substantially the same topic is altered in its own record. A different direction or concept, or a change that overlaps the record by less than about half, is a new record that names what it replaces through `supersedes` or `amends`. Git keeps every earlier version.
- **Trades away:** a record's body is no longer its original text. A reader who wants the record as first accepted reads `git log -p` or the dated notes.
- **One-way?** No. The rule lives in the ADR way and its neighbours, and an edit made under it is a commit.
- **Probes:** *Confident (edit-to-current):* you want an accepted record to read as what is decided now, with the earlier wording one dated note or one `git log -p` away. *Not confident (new-record-line):* whether a reversed or replaced decision should still get its own record, or whether you would rather edit the old record in place for those too.
- **Inversion:** at one end every accepted record is frozen and every change is appended or superseded, so a record's current meaning is assembled from its notes and successors. At the other every record is rewritten freely and only git holds what it said. This edits in place and marks the edits that changed a decision.

## Context

ADR-311 §4 made "correct an accepted record by appending" a convention, and ADR-304 §1 and ADR-309 §1 called decision and evidence bodies append-only and frozen. Under that rule, records that were corrected several times read as an original text followed by notes that revise it; ADR-304 carries four such notes, and a reader learns what §12 means now only by reading all of them. The operator overruled the rule in another project's session and asked for the same change here.

## Decision

1. **Edit to keep a record correct.** An accepted record of any kind is edited in place where that keeps it correct and readable: a wrong fact, a stale path or name, an unclear passage, or a rule the operator revised within the record's direction. Its text then says what is decided now. The accepted decision is the record read top to bottom, section by section: a passage the record marks as superseded or replaced is history, and the rest is what is accepted.
2. **Mark an edit that changes a decision.** Where an edit changes what a record decided, the edited passage carries a dated note: the date, the operator's words (or the reason, when no operator spoke), and what the text said before. An edit that changes no decision, such as a typo, a path or a clearer sentence, needs no note.
3. **Append what adds.** Evidence found later, a correction worth reading as its own event, and an amendment that adds to a record go in an appended dated section, as before.
4. **Same topic, same record.** When a decision is reversed, replaced or rejected and the change stays on substantially the same topic, its record is altered: the Summary, Decision and Consequences are edited to argue for what is decided now, and the changed passages carry dated notes. A different direction or concept, or a change that overlaps the record by less than about half, is a new decision that names what it replaces through `supersedes` or `amends`, and the passages it replaces are edited to say so, with a dated note citing it. Where the line falls is the agent's judgement.

   > Edited 2026-10-24: the operator said "when things are reversed or replaced (or rejected) if it is substantially the same topic then I'd rather alter the existing adr. if it is a different direction or concept or less than half overlapping (thats a subjective call but acceptable) then it's a new record. The reason for this is a blanket 'new append' policy would create a bueacratic mess." It read: "**A new direction is a new record.** A decision reversed or replaced, or carried to ground its record did not cover, is a new decision that names what it replaces through `supersedes` or `amends`. The passages it replaces are edited to say so, with a dated note citing it." The Summary said the same, and the Alternatives read: "**Edit in place for every change, with no new records.** Folds a reversal into a record whose Summary, basis and probes argued the other way, so the record no longer argues for what it says. Left open as the not-confident probe."
5. **What stays as written.** Quotations of the operator, the Summary's probes as asked, `basis` and `considered` entries, and `concern:` entries are history of who said what. They are not edited to read current; a later answer is another entry. An evidence record's citations of records superseded since stay as written, since they are true to their moment.
6. **Git is the backstop.** Every earlier version, and who changed it, is read with `git log -p`.

The ADR way's Conventions, the consider way, the adr skill and the system-architect agent teach this.

## Consequences

### Positive

- A record reads as its current decision; a reader does not assemble it from appended notes.
- A fix to a wrong fact or a stale path no longer needs a note of its own.

### Negative

- Judging whether an edit changes a decision is left to the agent and to review. An unmarked edit that did change one goes unflagged until someone reads the diff.
- The record's first accepted text lives only in git and in the dated notes.
- Whether a change stays on the same topic, and how much it overlaps, is a judgement. Two agents may draw the line differently.

### Neutral

- Amends ADR-311 §4, ADR-304 §1 (decision bodies append-only), §4 and §6 (the frozen-record field and rule), §12 (a decision append-only and citable) and ADR-309 §1 (evidence frozen once accepted). The tool needs no change: ADR-311 already removed the frozen check.
- Existing appended notes stay. A record is folded to read current when someone next edits it, not in a sweep.

## Alternatives Considered

- **Keep append-only.** The operator overruled it.
- **Edit freely with no notes.** Loses, in the record itself, the operator's reason for a changed decision and what was decided before; git keeps the text but not why it changed.
- **A new record for every reversal or replacement.** The operator declined it: a blanket new-record policy leaves one topic spread across a chain of records.
- **Edit in place for every change, with no new records.** Folds a different concept into a record that argued for another, so the record no longer reads as one decision.
