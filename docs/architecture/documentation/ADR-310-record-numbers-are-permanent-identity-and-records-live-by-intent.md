---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-306#6]
basis:
  - operator: aaronsb
    level: guided
    said: "I think it's ok to reorganize where they're located too (make the directory tree more aligned with intention)"
    via: session 2026-09-27
  - operator: aaronsb
    level: guided
    said: "I'm not sure - nobody external is citing these adr numbers, except the github record. so, I'm leaning keeping numbers where range allows but I'm not sure"
    via: session 2026-09-27, on renumbering records during a reorganization
  - operator: aaronsb
    level: guided
    said: "I'm thinking about how wbs has systems numbers. for instance NQA and systems design in nuclear archtecture (where I was exposed to this first) has a standardized numbering scheme"
    via: session 2026-09-27
  - operator: aaronsb
    level: guided
    said: "or, think about parts systems, mercedes benz for example"
    via: session 2026-09-27
  - operator: aaronsb
    level: guided
    said: "Adopt it (Recommended)"
    via: "session 2026-09-27, selected from agent-written options: permanent numbers, capability as classification, moves change the folder and not the number; the label was written by the agent"
  - operator: aaronsb
    level: guided
    said: "Six intent folders (Recommended)"
    via: "session 2026-09-27, selected from agent-written options; the label was written by the agent"
  - operator: aaronsb
    level: guided
    said: "Next in the area's band (Recommended)"
    via: "session 2026-09-27, selected from agent-written options on how new numbers are allocated; the label was written by the agent"
  - standard: "10 CFR 50 Appendix B, Criterion VIII: an item's identity is maintained on the item or on records traceable to it, throughout fabrication, erection, installation and use"
  - standard: "MIL-STD-881F §4.3: WBS numbers show indenture only, and maintaining WBS numbering is not a requirement of the standard"
  - evidence: "a survey of systems numbering schemes (VMRS, ATA iSpec 2200, IEC 81346, IEEE 803, MasterFormat, UniFormat, MIL-STD-881, Mercedes-Benz part numbers, Dewey, UDC, ACM CCS, SWEBOK, ISO/IEC 25010, 12207 and 42010) found no commonly known systems code for software, and found that traceability-heavy industries keep a permanent identity separate from a classification that may change, e.g. SAP's equipment number versus functional location"
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-09-27
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 306
  - 309
---

# ADR-310: Record numbers are permanent identity, and records live by intent

## Summary

- **Decided:** a record's number is its permanent identity. It is never changed and never reused. What a record is about is classified by `capability`, and the folder it lives in follows that classification. A record that belongs in another area moves folders and keeps its number. agent-ways' records move into six intent areas: ways, governance, documentation, attend, platform and practice. New numbers come from the next free slot in their area's band.
- **Trades away:** reading a record's area from its number. After a move, the hundreds digit tells where a record started, not where it is.
- **One-way?** No. Folders can be reorganized again at the cost of rewriting path links. Numbers never change, so no citation breaks either way.
- **Probes:** *Confident (identity-stable):* commit messages, pull requests and issues cite records by number, and none of them can be rewritten. A number that never changes keeps every one of them pointing at the right record. *Not confident (band-hint):* whether allocating new numbers by band will mislead readers into treating the digits as meaning, once moved records sit outside their folder's band.
- **Inversion:** at one end, numbers carry the classification and are renumbered on every move, which breaks citations each time. At the other end, numbers are a single global sequence with no hint at all. This decision keeps identity permanent and lets the band hint only at a record's origin.

## Context

ADR-306 §6 made a record's number follow its domain: moving a record to another domain renumbered it and rewrote every reference. Planning a reorganization of agent-ways' records by intent put that rule to the test. It would have renumbered about 62 records and rewritten their citations. Citations in the git history, pull requests and issues can't be rewritten.

A survey of systems numbering found no commonly known systems code for software: schemes like VMRS and ATA chapters work only because every truck or aircraft has the same systems. It also found that the industries with the strictest traceability requirements keep identity and classification apart. 10 CFR 50 Appendix B requires an item's identity to persist through fabrication, installation and use. Asset management separates the equipment number, which never changes, from the functional location, which follows the structure. MIL-STD-881F declines to require stable WBS numbers, because its numbers show only position.

## Decision

### 1. The amended rule

ADR-306 §6 is amended. Its rule that a number follows its domain, and that a move renumbers and rewrites references, is replaced by:

- A record's number is its permanent identity. It is never changed, and never reused for another record.
- Under adr/v1, a record's area (domain) is the folder it lives in. The number range of an area only allocates numbers for new records.
- A record that belongs in another area moves to that area's folder, keeps its number, and has every path reference to it rewritten. `adr domain move` does this.
- A domain may still hold several ranges, and areas may be added, merged and split. None of these changes a number.

Under adr/v0 the area is still read from the number range, and v0 output is unchanged.

### 2. Classification

`capability` classifies what a record is about (ADR-304, ADR-308). The folder follows it: each area holds the capabilities it groups. No separate systems code is added. The capability vocabulary is the project's own list of its systems, validated by lint.

### 3. agent-ways' areas

| Area | Folder | Band | Capabilities |
|---|---|---|---|
| ways | `ways/` (renamed from `system/`) | 100–199 | matching, disclosure |
| governance | `governance/` | 200–299 | governance |
| documentation | `documentation/` | 300–399 | adr, docs |
| attend | `attend/` | 400–499 | attend |
| platform | `platform/` | 500–599 | install, config, cli, testing |
| practice | `practice/` | 600–699 | authoring, method, loop |

Each record moves to the area of its main capability (the first listed). Evidence and spec records (ADR-309) are placed the same way. The legacy folder empties, and its range stays reserved so that no number in it is ever reused.

## Consequences

### Positive

- No citation anywhere, including git history, pull requests and issues, ever goes stale because of a move.
- The directory tree reads by intent.
- Reorganizing costs a folder move and path rewrites, and never a renumbering.

### Negative

- A number's hundreds digit no longer tells where a record lives. Tools and readers must use the folder or `capability`.
- Path links break on every move until rewritten, so moves must go through `adr domain move`.

### Neutral

- ADR-306 §6's domain commands keep their purpose (add, merge, split, move) without renumbering.

## Alternatives Considered

- **Renumber on move (ADR-306 §6 as written).** Rejected: it breaks citations that can't be rewritten, which is the failure the traceability standards are built to prevent.
- **Numbers encode origin and are frozen, with a separate current-location field (the Mercedes-Benz part-number pattern).** Close to this decision, but the folder and `capability` already carry the current location, so a separate field would duplicate them.
- **A systems code borrowed from an existing standard.** None exists for software, and codes like ATA chapters are specific to one kind of machine.
- **One flat folder.** No path ever changes, but the tree says nothing about intent, which the operator asked for.
