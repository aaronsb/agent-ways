---
contract: adr/v1
kind: decision
verb: change
capability: adr
amends: [ADR-304#7]
basis:
  - operator: aaronsb
    level: guided
    said: "I think we need to make sure that the new adr tools can create the content and manage the lifecycle of data, and probably, we need to think about a 'foreign import' tool that would take any kind of decision record that's not directly lintable/usable, and can ingest the foreign record. this way, it becomes our cannonical 'migration' tool."
    via: session 2026-09-27
  - operator: aaronsb
    level: guided
    said: "I think the foreign import model needs a round trip data object template of some kind."
    via: session 2026-09-27
  - operator: aaronsb
    level: guided
    said: "we should assuem that reasonable foregin records have some sort of structured data (frontmatter, for example). a /very foreign/ import could literally be jira issues for example"
    via: session 2026-09-27
  - operator: aaronsb
    level: guided
    said: "I think optional, but always lint warnings. sometimes, the summary isn't obvious until the apparent motion of the complete dataset is visible. this means that adr record properties can be altered (like summaries). this is fine, because any alteration that gets tracked is a git commit. we don't have to overthink integrity here"
    via: session 2026-09-27, on whether imported records need a Summary
  - operator: aaronsb
    level: guided
    said: "we don't need to explicitly handle jira. all I'm saying is 'jira issues can be flattened to a record, just like any other record, and usually there's a description and a summary and various fields, and if we can selectively import jira issues, then we probably can take records from about anything'"
    via: session 2026-09-27
  - evidence: "kg (knowledge-graph-system) holds 123 records, all with v0 frontmatter (status, date, deciders, related); 60 Accepted, 28 Proposed, 16 Draft, 10 Rejected, 8 Superseded, 1 Deprecated"
  - evidence: "in a project that declares contract: adr/v1, `adr new` still writes a v0 record with no contract, kind, verb, capability, basis, agent or Summary"
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
  - 305
---

# ADR-306: adr import: foreign records through a round-trip import sheet

## Summary

- **Decided:** `adr import` is the canonical migration path into adr/v1. `scan` reads records from any structured source into one import sheet per record. The agent fills in what needs judgement. `apply` writes each finished sheet as a v1 record. `adr new` writes through the same writer, and `adr supersede` and `adr enact` complete the lifecycle commands.
- **Trades away:** hand migration's freedom to restructure a record while moving it. Import carries the body over byte for byte. Splits and rewrites happen after import, as ordinary edits.
- **One-way?** No. Sheets are staging files and records stay in git. A bad import is reverted like any commit.
- **Probes:** *Confident:* v0 records (this repo's and kg's) import with the body unchanged, since everything the reader needs is in the frontmatter. *Not confident:* whether a field map that flattens a structured item into fields and a body covers sources whose body isn't markdown, or whether some sources need a conversion step first.
- **Inversion:** at one end, a reader written in code for every format: exact, but never finished. At the other end, an agent reads each foreign record and writes v1 by hand: flexible, but manual across a hundred records. This decision maps structured fields mechanically and leaves only the judgement fields to the agent.

## Context

ADR-304 §7 moves a v0 record to v1 "when someone next edits it". That works for a trickle of edits, and it doesn't work for a corpus. kg holds 123 v0 records, and other projects hold records in adr-tools, MADR or tracker formats. #581 migrated two records here by hand, and the tier 2 rehearsal showed an agent can do it without inventing anything, but record by record.

Most of a migration is mechanical. Status maps by the §7 table, and date, deciders and links carry over. The body stays as written. A few fields need judgement: the verb, the capability, the basis, and a Summary. Those are the only fields an agent should have to touch.

The tool also has lifecycle gaps. `adr new` writes a v0 record in a v1 project. Supersession needs both sides' links edited by hand, and enactment is a hand-edited field.

## Decision

### 1. The import sheet

One YAML file per record is the round-trip object between a source and a v1 record:

```yaml
sheet: adr-import/v1
source: {path: docs/architecture/system/ADR-186-….md, format: v0, sha256: "…"}
target: {number: 186, domain: system}
record:                  # v1 frontmatter, filled as far as the reader can
  contract: adr/v1
  kind: decision
  status: accepted
  date: 2026-09-17
  verb: ~
  capability: ~
  basis: []
  agent: {name: Claude, model: unrecorded}
summary: ~
todo: [verb, capability, basis]
candidates: {capability: [testing, install]}
provenance: {status: "frontmatter status: Accepted"}
unmapped: {deprecation_note: "…"}
body: |
  …
```

- `record` holds v1 frontmatter. The reader fills what the source states, and `provenance` says where each value came from.
- `todo` lists what the reader could not fill. `candidates` ranks vocabulary matches to help whoever fills them.
- `unmapped` keeps every source field that has no v1 home. Nothing is dropped.
- `body` is the source body, verbatim.

### 2. Readers

A reader turns a source into sheets. Import assumes structured sources: frontmatter, a metadata block, or a structured export. Unstructured prose is out of scope.

- **Built in:** v0 (this tool's frontmatter), MADR, and adr-tools (inline `## Status`, `0001-` numbering).
- **Field maps:** any other structured source is flattened into fields and a body. A declarative map names which source field fills which sheet field, and how values translate. No source gets its own reader. The example is a tracker export, since a source like that flattening cleanly suggests most structured records will:

```yaml
reader: tracker
items: issues                    # a JSON export: one sheet per item, selected by --filter
fields:
  title: fields.summary
  date: fields.created
  status: {from: fields.status.name, map: {Done: accepted, "Won't Do": rejected, "To Do": proposed}}
  body: fields.description
  unmapped: [key, fields.labels]
```

No reader ever writes an `operator` basis from `deciders`, an assignee or any other metadata (ADR-304 §7, §11). A basis comes from what the record says, and it is filled during cleanup.

### 3. Commands

- `adr import scan <paths> [--reader NAME | --map FILE]` writes sheets to `docs/architecture/.import/`. That directory is gitignored: sheets are working files, and only the records they produce are committed.
- `adr import apply [sheets] [--partial]` writes each sheet whose `todo` is empty as a v1 record, then lints it. A sheet with open items is skipped. `--partial` writes it anyway, and lint reports what is missing.
- `adr new` builds an empty sheet from its arguments and applies it, so a new record and an imported record share one writer. In a v1 project it writes v1.
- `adr supersede <old> --by <new>` writes both sides of the link. `adr enact <n> <commit>` sets `enacted` on an accepted cut or retire.

### 4. Imported records

An imported record carries `imported: {from, format}` in its frontmatter. For an imported record, a missing Summary is a lint warning. The Summary may be written later, once the whole corpus has been imported and read together, and git history records when it was added.

### 5. Numbering

A source numbered inside the project's domain ranges keeps its number. Any other source gets a number from `target`, which the reader proposes from the domain and the agent may change. `apply` rewrites references within the imported set to the new numbers, and `imported.from` keeps the original identifier.

### 6. Round-trip guarantees

These are tested properties:

- Applying an unedited sheet keeps the body byte-identical, and every source field is either mapped into `record` or kept in `unmapped`.
- Scanning a v1 record and applying the result reproduces the record.
- v0 output of every existing command stays byte-identical.

## Consequences

### Positive

- Migrating a corpus becomes one scan, one cleanup pass over small structured files, and one apply. Both this repo's 93 v0 records and kg's 123 go through the same path.
- A new source needs a field map, not a code change, whenever it flattens to fields and a body.
- `adr new` produces a v1 record in a v1 project.

### Negative

- Records are imported as they stand. A mixed record that should be split is imported whole and split afterwards.
- Imported records may sit without a Summary, and lint keeps warning until they have one.
- Field maps are a small configuration language that has to be documented and kept stable.

### Neutral

- ADR-304 §7's status table is unchanged. The v0 reader applies it.
- A v0 record edited by hand still moves to v1 as before. Import is the bulk path alongside it.

## Alternatives Considered

- **An agent migrates each record by hand.** The #581 rehearsal shows this works, but across 123 records the mechanical fields would be retyped each time and the body could drift.
- **A code reader per format, with no field maps.** This is exact for known formats, but every tracker and template needs code in the vendored tool.
- **Migrate in place with no intermediate object.** A migration writes the record directly. Without a sheet there is no place to stage what needs judgement, and no object to test the round trip against.
