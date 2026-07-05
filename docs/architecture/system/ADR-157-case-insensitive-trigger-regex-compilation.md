---
status: Accepted
date: 2026-07-04
deciders:
  - aaronsb
  - claude
related:
  - ADR-155
  - ADR-156
---

# ADR-157: Case-insensitive trigger regex compilation

## Context

The keyword lane compiles each way's `pattern:` (and the `cmds:` / `files:`
patterns) with `Regex::new(...)` and matches it against the **original-case**
text. `mask_nonlinguistic` (ADR-155 §2) strips fences and URLs but does not
lowercase, and the query path (`match_prompt`, `scan/mod.rs:596`) passes the
masked query straight through. Only the tool-description path
(`scan/mod.rs:354`) lowercases its input, and it does so on the *text*, not the
pattern.

The consequence: a lowercase author pattern silently misses the uppercase
acronyms users actually type. `\bssh\b` misses `SSH`; `\berd\b` misses `ERD`;
`\bpr\b`, `ADR`, `DBML`, `MTTR` all leak the same way. This surfaced in the
PR #301 review, which patched the acute offenders by prepending an inline
`(?i)` flag to five patterns:

- `hooks/ways/meta/subagents/subagents.md`
- `hooks/ways/meta/introspection/introspection.md`
- `hooks/ways/workstation/pkghistory/pkghistory.md`
- `hooks/ways/softwaredev/environment/ssh/ssh.md`
- `hooks/ways/data/documentation/documentation.md`

That is per-way cruft. It fixes the five patterns someone happened to notice and
leaves every other acronym-bearing pattern latent — the next author who writes
`\bpr\b` re-introduces the bug and won't know why their way never fires on `PR`.
Case sensitivity is the wrong default for a trigger channel: an author writing a
keyword means the *concept*, not a specific casing.

**Blast-radius survey of the current corpus** (why a global fix is safe):

| Lane | Cased patterns today | Effect of case-insensitivity |
|------|----------------------|------------------------------|
| `pattern:` (keyword) | 5× `(?i)` + `SKILL\.md` | Intended fix. `SKILL\.md` still matches `skill.md` — same concept. |
| `cmds:` | none | No-op — no uppercase-bearing command patterns exist. |
| `files:` | `README\.md$`, `Makefile$\|makefile$\|GNUmakefile$`, `Makefile$` | Desirable — READMEs and Makefiles have real casing variants; the `makefile$` alternation branch becomes redundant-but-harmless. |

No pattern in the corpus relies on case-sensitivity to *avoid* a match. The
helpers (`regex_matches`, `regex_span`) are private to `scan/mod.rs` and serve
all three lanes, so the cleanest change lives in one place.

## Decision

Compile trigger regexes **case-insensitively** by building them with
`regex::RegexBuilder::new(pattern).case_insensitive(true)` in the two shared
helpers `regex_matches` and `regex_span` (`scan/mod.rs`). This applies uniformly
to the keyword, command, and file lanes.

Because the keyword regex now matches case-insensitively, the tool-description
path no longer needs to pre-lowercase its text: `scan/mod.rs:354` changes from
`regex_span(pat, &desc.to_lowercase())` to `regex_span(pat, desc)`, which also
yields a truer original-case `matched_span` in telemetry (ADR-153 §3).

Then **retire the five inline `(?i)` flags** — they become redundant. The
patterns revert to their plain form; behavior is preserved by the global flag.

The invariant, stated once so future authors inherit it: *the keyword lane
matches case-insensitively; write patterns in lowercase and mean the concept.*
This lands in the engine-reference and the authoring surfaces.

## Consequences

### Positive

- Every acronym-bearing pattern (`PR`, `ADR`, `SSH`, `ERD`, `DBML`, `MTTR`, …)
  matches the uppercase form users type — corpus-wide, not just the five noticed.
- Removes per-way `(?i)` cruft and the latent-bug trap it papered over.
- Truer `matched_span` telemetry on the description path (original case, not
  lowercased).
- One compile-site invariant replaces a convention every author had to remember.

### Negative

- An author who *wants* case-sensitive matching (e.g. to distinguish `OK` from
  `ok`) can no longer get it via the shared helpers. No current pattern needs
  this; if one ever does, it can carry an inline `(?-i)` scope — the regex crate
  supports per-pattern override, so the global default is not a hard ceiling.
- Marginally wider matching on `files:`/`cmds:` (e.g. `.ENV` now matches an
  `\.env$` pattern). Reviewed as desirable, not a regression, for the current
  corpus.

### Neutral

- The `makefile$|GNUmakefile$` explicit-casing alternations are now redundant;
  they are left as-is (harmless) rather than churned in this ADR's scope.
- `RegexBuilder` is already in the `regex` crate dependency — no new deps.

## Alternatives Considered

- **Lowercase the text instead of the pattern.** Rejected: globally lowercasing
  the query breaks any deliberately-uppercase pattern (`SKILL\.md` would need
  the text cased to match) and mangles the captured span. The regex flag matches
  the *pattern* case-insensitively without touching the text — strictly safer.
- **Keyword-lane-only case-insensitivity** (a separate helper used only at
  `:596`, leaving `cmds:`/`files:` case-sensitive). Rejected: the same
  `pattern:` field is consumed at both `:354` and `:596`, so splitting behavior
  by call-site would make one field match two ways; and the survey shows
  case-insensitivity is *desirable* for `files:` (READMEs, Makefiles) and a
  no-op for `cmds:`. A uniform rule is simpler and correct.
- **Keep patching per-way with `(?i)`.** Rejected: it is the status quo that
  produced the bug — it fixes only noticed patterns and re-arms the trap for the
  next author.
