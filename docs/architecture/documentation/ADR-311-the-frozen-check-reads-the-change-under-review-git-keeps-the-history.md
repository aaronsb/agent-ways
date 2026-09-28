---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-304#6]
basis:
  - operator: aaronsb
    level: directed
    said: "git is always the backstop. we should never try to recreate anything that git does"
    via: session 2026-09-27, after reviewing the friction in #604
  - operator: aaronsb
    level: guided
    said: "my hunch is that the amount of questions you're asking and things we're running into is making this a high friction solution"
    via: session 2026-09-27
  - evidence: "two reviews of #604: rebuilding a record's history from git produced a false error on three committed moves, missed and invented edits through rename detection, ran silently on shallow clones, varied with the origin remote, and needed a former_paths alias table for files that became records"
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "Holds"
    via: session 2026-09-27, selected from agent-written options when the probes were asked; the label was written by the agent
    covers: [catches-at-edit]
  - operator: aaronsb
    said: "Links by number (Recommended)"
    via: session 2026-09-27, selected from agent-written options for what happens if comparing references by record grows special cases; the label was written by the agent
    covers: [moves-in-change]
status: accepted
date: 2026-09-27
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 309
  - 310
---

# ADR-311: The frozen check reads the change under review; git keeps the history

## Summary

- **Decided:** `adr lint` checks a frozen record against one version: the same record on the base the change will merge into. It reads that version with git and compares. It does not walk history, follow renames, or keep aliases for old paths. Git is the record of every earlier version.
- **Trades away:** detection after the fact. An edit that reached the default branch without passing the check is visible only in git history.
- **One-way?** No. The check can be widened again later; nothing in the records depends on it.
- **Probes:** *Confident (catches-at-edit):* an in-place edit to an accepted record is flagged in the change that makes it, where the author can still append a correction instead. *Not confident (moves-in-change):* whether comparing links by the record they resolve to, at the base and at the change, covers a move made in the same change without new special cases.
- **Inversion:** at one end nothing checks an accepted record and review is the only guard. At the other the tool rebuilds each record's past from git and judges every version, duplicating what git already holds. This checks the one difference a reviewer is looking at.

## Context

ADR-304 §6 freezes a decision's frontmatter outside `mutable_after_accept`, and §1 lets a frozen body grow only by appending. The first implementation read every accepted version of a record from git history. Once records could move between folders (ADR-310) and cite each other by path, every move edited frozen text, and the check had to decide which historical edits were legitimate. Each fix added a case: rename detection pairing files by similarity, copies, shallow clones, the choice of remote, repeated moves, and files that became records without a number (ADR-309). That work rebuilt what git already provides.

## Decision

1. **One comparison.** For each frozen record in the working tree, lint reads the record with the same number at the merge base of `HEAD` and the default branch (`origin/HEAD`), and compares frontmatter and body against it. A record absent at the base is new and has nothing to compare.
2. **References by record.** A reference that resolves to a record, at the base against the base tree and in the change against the working tree, compares as that record's number. Moving a record changes its path in other records' text without counting as an edit. Pointing a reference at a different record counts. If this comparison needs special cases of its own, records cite each other as `ADR-N` instead, so a move leaves frozen text untouched and the comparison is a plain diff.
3. **No history.** The check reads no log, follows no renames, and carries no alias table. When the base cannot be read (no default branch, a shallow clone without it), lint says the frozen check did not run.
4. **Git is the backstop.** Past versions, who changed them and when, are read with git. The tool does not store or reconstruct them.

## Consequences

### Positive

- An accepted record's in-place edit surfaces in its own pull request, while it can still become an appended correction.
- The frozen check no longer depends on rename detection, clone depth beyond the base, or remote naming. `former_paths` goes.

### Negative

- Edits already on the default branch are not re-checked. ADR-306 and ADR-307 show that such edits happen; they surfaced only because the old check read history.

### Neutral

- CI fetches the base branch. The workflow already fetches full history.
- The path rewriter's safety rules from #603 (exact rewrites, permalinks, fenced code, backslash paths) are unchanged.

## Alternatives Considered

- **Keep the history walk, keyed by record number.** Built and reviewed in #604. It fixed the rename cases and still needed an alias table and shallow-clone handling; each fix added configuration.
- **No automated check.** Review alone catches an edit only when the reviewer reads the record's body; the Summary format makes it easy to skim past one.
