---
status: Accepted
date: 2026-08-13
deciders:
  - aaronsb
  - claude
related:
  - ADR-174
---

# ADR-178: Register transfers by demonstration - core.md carries policy

## Context

ADR-174 addressed decorated prose with three tiers of guidance and a postcheck that counts. Its own test returned a null result and closed as "unfalsified, not validated." The operator reports output quality has degraded since, and identified `core.md` as the source.

The measurements support that reading, on a mechanism ADR-174 did not consider.

### Core.md contains the construction it bans

`core.md` line 23 reads: *"Don't resolve with 'not X but Y' or 'I can't do X, so I'll do Y' to sound measured."*

The file uses that construction eleven times by regex count and fourteen by inspection, in 1,002 words. Two are section headers:

- **Reasoning runs; it doesn't pose.**
- **Prose delivers; it doesn't decorate.**

Others: "Write to be read, not admired," "Directness is the absence of detour, not short sentences," "The filesystem is not a task queue," "Memory is for what's load-bearing across sessions, not prostration gestures," "The ledger is not the whole method," "what's checkable is the contract, not the prose."

The ban and its licensed exception arrive in the same sentence, so line 23 demonstrates the shape twice while forbidding it.

Measured against the corpus: 73 way files over 200 words, 30,555 words, 142 matches, a baseline of **4 per thousand**. `core.md` runs **11 per thousand**, and it is the one file that fires on every session before any work begins.

### The existing check cannot see this

Running the `documentation/markdown/density` postcheck logic against `core.md` yields **1 significance clause and 9 em-dashes per thousand words**, against thresholds of 3 and 15. The file passes the instrument this project built to catch exactly this problem.

The postcheck counts a surface form. What transfers across a session is a rhetorical shape, and the shape survives every rewrite that dodges the regex.

### The corrected examples teach the dodge

`meta/trust/prose/prose.md` presents paired before/after rows. Both "delivered" versions relocate the evaluation into a subordinate clause rather than deleting it:

| Labeled decorated | Labeled delivered |
|---|---|
| "…three dead triggers. That is the mark of a system with no linting." | "…three dead triggers, none of which the system would have surfaced on its own." |
| "Core fires once per session. This is the most serious gap in the retention model." | "Core fires once per session and is never refreshed until compaction." |

The string `That is the` dies; the move survives. A model reading these learns where the counter looks.

### The mechanism

In-context style transfer is imitative before it is instructed. A file's register is a sample; its rules are a claim about a sample. When the two disagree, the sample wins, and `core.md` is the largest register sample in the always-on window — 677 of its 1,002 words sit under Posture, and eight bolded aphorisms lead its paragraphs.

ADR-174 established the adjacent failure: negative constraints are the weakest instruction form, failing at 22-30% on frontier models. This ADR adds the reason a stronger negative constraint cannot help. Listing "there's a tension here," "two things are true," and "it's worth naming that" as forbidden strings places those strings in every session's context. Polarity is cheap; presence is not.

ADR-174's null result reads consistently with this. Both arms scored 0.9 significance clauses per thousand with zero antithesis constructions at ~4,400 fresh-session words. The instrument had power and found nothing, because the register had not yet had a long draft to propagate through.

ADR-174 closes its own test section with **"unfalsified, not validated."** The phrase is the construction under discussion, in the document written to remove it, at the point of maximum epistemic self-regard. A remediation that reproduces its own subject at its own conclusion is the clearest available evidence that the demonstration channel outranks the rule channel.

### Register now has a better home

Claude Code output styles inject register into the system prompt, unconditionally, for the whole session. The operator has authored two (`finnish-direct`, `simplified-modified-technical`). The Finnish-direct style carries the same posture as core's Posture section in 90 words, at a flat temperature, with no demonstration of a banned form.

`core.md` fires at turn 1 and, per ADR-174's re-disclosure finding, never again. Its register instruction is therefore both weaker in placement and louder in demonstration than the surface that supersedes it.

## Decision

**`core.md` carries policy and epistemics. Register instruction leaves it.**

Five changes.

**1. Strip register content from `core.md`.**

Removed: the reasoning-tic ban list with its verbatim forbidden strings; the prose delivery/decoration bullets, which ADR-174 already assigned to tiers 2 and 3; the eight bolded aphorism lead-ins.

Retained: the uncertainty taxonomy, which is epistemic routing carried nowhere else; the collaboration and post-compaction guidance; Method; Language; Line handling; Attribution. One line of the ban list survives as reasoning guidance rather than phrasing guidance — a one-sided claim is stated once and stopped.

Result: 632 words, all policy, measured by the same prose extraction the checks use.

**2. The constraint on `core.md` is structural and checked, in `scripts/check-register.sh`.**

Any file in the always-on path is a style demonstration whether or not it intends to be. "Write plainly" would drift the way "sparingly" drifted, so the constraint is four counts, all at zero for `core.md`: antithesis constructions, significance clauses, bolded paragraph lead-ins, and paired em-dash asides. The script runs in the pre-commit hook and reports the corpus baseline under `--corpus`.

The fourth count comes from ASD-STE100, by way of the operator's `simplified-modified-technical` output style. STE turns a parenthetical aside into its own sentence, and applying that rule to `core.md` is what made the constructions visible in the first place. A structural constraint governs sentence form, so it holds where a taste rule dissolves: "Reasoning runs; it doesn't pose" fails an STE check mechanically, on the semicolon antithesis and on the second meaning loaded into "run."

**3. Re-pick the examples in `meta/trust/prose/prose.md`.**

The "delivered" column deletes the evaluation instead of relocating it. The way also loses its own antithesis constructions, including the sentence that defines directness by metaphor and then reasons from the metaphor across three clauses.

**4. Register authority belongs to the output style.**

Ways carry method, evidence discipline, and domain practice. An operator without an output style loses the turn-1 register nudge; that is accepted, because the nudge was net-negative in the measured configuration.

**5. Sweep the corpus.**

Fourteen way files measured at or above 8 antithesis constructions per thousand words. Each was edited construction by construction, splitting a load-bearing distinction into two positive statements and deleting a decorative counterweight outright. The corpus moved from **4 per thousand to 2**, with no file left above the advisory line.

### The tic reproduces under active removal

While editing `CLAUDE.md` to remove one antithesis construction, the replacement text written into it read: *"Register — how output reads — comes from the active output style, not from the ways corpus."* That is a paired em-dash aside wrapped around a counterweight, authored one line after removing the same two shapes from the same file, by a writer holding the rule in working memory.

The same thing happened one step earlier. The first repair of `meta/subagents/subagents.md` turned "This is not an override" into "so nothing here overrides it," which demotes the negation into a subordinate clause. That is the exact dodge this ADR documents in `prose.md`'s example table, committed by the author of the table.

Both are recorded because they bound what any of this can achieve. The register is not held in the rules; it is resident in the writer. A checked count catches it after the fact, and nothing catches it during.

### Not decided here

Adding an antithesis counter to the `density` postcheck was considered and deferred. That check reads the text just written on any markdown Edit, so its false-positive cost is paid on every file in the repository, and the regex that catches the tic also catches ordinary negation. ADR-174's threshold discipline holds: a surface that nags trains its reader to ignore it. `check-register.sh` takes the strict thresholds instead, because it is scoped to one file whose count is zero.

## Consequences

### Positive

- The largest register sample in the always-on window stops demonstrating the constructions the project is trying to remove.
- `core.md` drops from 1,002 to 632 words, all of it policy that no other surface carries.
- The output style becomes the single register authority, with no competing voice in the hook path.
- The forbidden strings leave the context window.

### Negative

- Operators running no output style lose the turn-1 register guidance entirely. Tiers 2 and 3 of ADR-174 remain, and tier 2 fires only on explicit long-form requests.
- The `density` postcheck still cannot see the shape this ADR is about. Only `core.md` is checked for it.
- The corpus sweep touched fourteen files in one change, so any behavioral effect measured after this lands cannot be attributed to `core.md` alone.

### Neutral

- ADR-174's three-tier structure stands. Tier 1 shrinks to nothing for prose guidance; tiers 2 and 3 are unchanged apart from the example rewrite.
- The core re-disclosure gap ADR-174 recorded is unaffected, and a shorter core makes a future distance-based re-disclosure cheaper to consider.

## Alternatives Considered

- **Rewrite the examples only, keep core.md intact.** Rejected. The two section headers are the loudest instances in the file, and they arrive before any example does.
- **Strengthen the ban with more explicit prohibitions.** Rejected. ADR-174 measured negative-constraint compliance at 22-30%, and each added prohibition adds its forbidden string to every session.
- **Add antithesis counting to the density postcheck now.** Deferred rather than rejected — see "Not decided here."
- **Leave the outlier way files for a later pass.** Rejected. Register propagates from every sample in the window, so a clean `core.md` sitting among fourteen dyed way files leaves the channel open. The cost is recorded above: the change is now unattributable between core and corpus.
- **A "write plainly" instruction in the authoring way.** Rejected on ADR-174's own finding. An unmeasurable rule has no moment at which compliance can be tested, which is how "use em dashes sparingly" survived to 118 em-dashes.
