---
contract: adr/v1
kind: decision
verb: constrain
capability: [adr, docs]
amends: [ADR-304#6]
basis:
  - operator: aaronsb
    level: directed
    said: "ADRs get sort of casually inserted into documentation or strings and other places, and they really need a better formal reference structure, so they don't cause a reader to feel the need to go chase them down in the codebase. they're rarely 'shipped' with the software that is built, but always with the codebase."
    via: this session, 2026-11-13, after the vtium session relayed the same finding from its code
  - operator: aaronsb
    level: directed
    said: "A comment's body explains in its own plain words what the code does and why, complete without following any reference. Decision-record references go in a block of their own at the end of the comment, like the references at the end of a paper or a wiki article, and each reference line says what that record decides for this code."
    via: "the vtium session (Nina), 2026-11-13, relaying the operator's decision for vtium at the operator's request; quoted from the relay, which paraphrases"
  - evidence: "rough greps on main at 962c162e: about 1,400 citations in Rust comments, about 100 in Rust string literals (for example tools/agent-fmt/src/permissions.rs:261, 'See ADR-116 for migration guidance.'), about 57 in hook script messages, 88 in way bodies and about 630 in docs outside docs/architecture; vtium reported 3,125 in its .rs files"
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-11-13
deciders:
  - aaronsb
  - Claude
related:
  - 304
  - 110
---

# ADR-314: A citation ends what it supports, and text that ships carries none

## Summary

- **Decided:** the text around a citation explains itself without the citation. In text that stays with the repository, such as code comments and docs pages, citations of decision records go in a references block at the end, and each entry says what that record decides for the code or page above it. Text that ships with the built software, such as program output, error messages, hook messages and the framework's way bodies, carries no record numbers. `adr cite` checks where each citation sits, at a strength each project sets, and can hold only the lines changed since a git ref to the rule.
- **Trades away:** a citation next to the sentence it supports. A reader who wants to know which record decided one clause reads the references block, and an author writes the explanation the citation used to stand in for.
- **One-way?** No. The rule is a check setting and a way, and `cite.placement: off` turns the check off.
- **Probes:** *Confident (shipped):* program output, error messages and injected way bodies should name no record, since their reader cannot open it. *Not confident (docs-body):* whether docs pages should follow the comment rule as strictly, or whether an explanation page may name a record in its body when the page is about that decision.
- **Inversion:** at one end every claim carries its citation inline, so provenance is exact and the prose is hard to read. At the other no text names a record, so prose reads cleanly and nobody can trace a rule to its decision. This keeps the trace in a block the reader can skip and drops it where the reader has nothing to trace to.

## Context

Code and docs cite decision records by number, and ADR-304 §6 gives those citations a maintenance loop: `adr cite` reports a citation that resolves to no record, or to a record out of force. It says nothing about where a citation sits. In practice citations are written into sentences as they come up: "the guard lights the boot display (ADR-1401 D9) unless the file is closed (D7)". A reader has to open each record to fill in what the sentence leaves out.

Two kinds of reader meet these citations. One works in the repository and can open the record, but following it breaks the reading. The other reads text the built software shows: a CLI message, a TUI label, a hook's message to an agent, a way injected into a session in another project. Records ship with the codebase and rarely with the software built from it, so that reader cannot open the record at all, and a number written for a different project's corpus can name the wrong record in theirs.

## Decision

1. **The text stands on its own.** A comment, page, message or way says what the thing does and why in its own words. A citation adds where the rule came from; it does not carry part of the explanation.

2. **Text that stays with the repository cites in a references block at the end.** In a code comment, a line whose text is `References:` opens the block, and the block runs to the end of that comment. In a Markdown page, a heading whose text is `References`, at any level, opens the block, and it runs to the next heading of the same or a higher level. A citation elsewhere in the comment or page is out of place. A page's frontmatter is metadata and may cite records anywhere in it.

   ```rust
   /// Tries to wake the displays again once a second while this VT is
   /// shown. Another program may let go of the display at any moment, and
   /// nobody may press a key, so it keeps trying on its own.
   ///
   /// References:
   /// - ADR-1000, "Replies and policy": the slow retry and its pace.
   /// - ADR-1400 D1: why it needs DRM master to draw.
   ```

3. **Each entry says what the record decides here.** An entry starts with `- ` and a citation, and goes on, on the same line or the lines under it, to say what that record decides for the code or page. An entry that is only a citation is a finding.

4. **Text that ships carries no record numbers.** In a program source file (Rust, Python, TypeScript, JavaScript, Go, shell), a citation outside a comment sits in a string or in code, which reaches the built program, so it is out of place. A Python docstring counts as a comment. A project lists further shipped paths in `adr.yaml` as `cite.shipped`; any citation in them outside a Markdown file's frontmatter is out of place. This repository lists its way bodies, skills, agent definitions and slash commands, which are projected into every user's sessions; its hook scripts are program files, so their messages fall under the rule above. A way that implements a decision names the record in its provenance sidecar (ADR-110), as a `policy` entry with `type: adr`, which is not injected.

5. **What the placement check leaves alone.** Values in data files (YAML, JSON, SQL outside comments) and the records under `docs/architecture/` are not checked for placement. Commit messages and PR bodies are outside the tree. A line carrying `adr-cite-ignore` stays skipped. Test code is held to the comment rule but not the shipped rule: files under a `tests/` directory, files named as tests (`*_test.*`, `test_*.py`, `*.test.*`), and in Rust everything after a `#[cfg(test)]` line.

6. **The check's strength is the project's.** `cite.placement` in `adr.yaml` is `error`, `warn` or `off`; unset reads as `warn`, so a project that refreshes its copy of the tool sees findings without failing its build. `adr cite --since <ref>` holds the lines added or changed since the merge base of `<ref>` and the working tree to the rule as errors, unless placement is `off`. A project runs it on pull requests so new text follows the rule while older text is swept. Resolution findings from ADR-304 §6 are unchanged.

7. **A vendored tool cites none of its home project's records.** A tool copied into other projects, such as `docs/scripts/adr`, carries no record numbers from the project that wrote it, in its comments or its messages: in the project it is copied into, the same numbers name that project's records. The sweep of this repository's existing citations, the tool's included, is tracked in #1271.

## Consequences

### Positive

- A comment or page reads through without lookups, and its references block says in one line per record what each decided.
- A user of the built software, or an agent in another project, no longer meets a record number it cannot open.
- A references entry that says what the record decides here makes a stale citation visible in review: the entry's claim stops matching the code.
- `--since` lets a project with thousands of existing citations hold new text to the rule from the day it adopts the check.

### Negative

- The check now reads comment syntax for each language it scans: a line-based scanner for comments, strings, Python docstrings, Rust raw strings and shell heredocs. Code it misreads produces false findings until a fixture covers it.
- An author writes the explanation a citation used to stand in for, which takes reading the record. Sweeping an existing tree is large work (#1271).
- A reader who wants the record behind one clause looks it up in the references block rather than beside the clause.

### Neutral

- `adr.yaml` gains `cite.placement` and `cite.shipped`. A project that sets neither gets warnings and no failures.
- CI that runs `--since` needs the merge base, so a shallow checkout fetches enough history or the check falls back to the whole tree.

## Alternatives Considered

- **Keep citations inline and check only that they resolve.** This is ADR-304 §6 as it stood. It leaves the reading cost and the shipped numbers in place.
- **Inline citations as links.** A link is one click on a forge, but the reader still leaves the sentence to finish it, and in shipped text the link points into a repository the reader may not have.
- **Footnote markers in the body, such as `[1]`, resolved in the block.** Keeps a pointer at each clause, but the marker invites the same chase, and a renumbered block breaks every marker in the comment.
- **A separate tool or lint for placement.** `adr cite` already walks the tree and resolves every citation; a second walker would disagree with it about which files and lines count.
