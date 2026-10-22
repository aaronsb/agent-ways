---
description: claims, quotes, statistics and citations in a report, deck, whitepaper or summary someone will act on; open every link, ticket and DOI before citing it, attribute only what the source states, never invent a replacement reference, have code do the counting, date measured numbers and label estimates, raise conflicts between sources, and give the author a list of open items
vocabulary: source sources cite citation citations reference footnote link doi quote quotation statistic figure number where did that number come from traced verify fact check unverified estimate measured recompute recount tally total conflict discrepancy two exports disagree experts argue industry reports attribution placeholder oaicite whitepaper market size evidence open items dashboard which report
pattern: \bprovenance\b|where did (that|this) (number|figure|stat|statistic|quote) come from|not yet measured|oaicite|contentreference|turn0search|utm_source=chatgpt|\bfact.?check (this|the|these|my|that|each|every|all)\b|\b(experts|analysts|critics|scholars) (argue|agree|note|believe)\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: constraint -->
# Provenance for Claims and Numbers

A reader acts on what a report says. When a deliverable states a claim, a figure or a quote, give it a path back to where it came from, or state it as your own view.

## Check before you cite

- Open each link, ticket key and DOI you cite. Confirm it says what you claim. Check each quote against its source.
- Keep a link or key the user gave you even if you cannot open it. Say that you could not check it.
- Remove links and references you produced and could not confirm. Never invent a replacement.
- Attribute only what the source states, and mark your own reading as yours. Name the person, document or dataset behind each claim. Find the exact sentence before you write "clearly states".
- When a source lacks a fact, say which source you checked. Do not add what the answer likely is.

## Signs of weak sourcing

| Kind | Examples | Fix |
|---|---|---|
| Vague attribution | "experts argue", "industry reports", "analysts note", "several sources" for one | Name the source, or state it as your view |
| Borrowed authority | "[Source] highlighted" or "underscores" when the source does not say it | Quote what it does say |
| Gap hedges | "specific details are limited", "not widely documented" | Name the source you checked and what it lacks |
| Vague connection | a person or firm "associated with" another | The real role, such as "was CEO of" |
| Tool leftovers | oaicite, turn0search0, [cite: 1], [web:1], 【85†L261】, utm_source=chatgpt.com | Strip them |
| Placeholders | [Your Name], 2025-xx-xx | Fill each one, or ask the user |

## Numbers trace or recompute

- Every figure traces to a named source or a script. A model cannot reliably tally a long list, so the model classifies items and code does the counting.
- Check dates, attributions and counts against the primary record. The user's dictation and earlier drafts are leads to check. Flag wording that runs ahead of the evidence, such as "funded" when the record says "in motion".
- Before shipping, recompute a few figures independently and say how.
- Quote a measured number with its date. Mark an estimate with `~` or "not yet measured". Say what a measurement includes.
- When sources disagree, work from one value and flag the other, for example "using 100; the second export says 90". Do not pick one silently or edit a source to match.
- When a figure is challenged, recheck the math before defending it.
- Put a correction in a dated note beside the original. Keep the original.

## Open items go to the author

End the work with a list for the user or author: unverified numbers, discrepancies, facts to confirm and with whom, and decisions they need to make. Mark a fact recalled from memory "unverified". The list stays with the author. Resolve each item before the client copy goes out, or state the limitation in the copy plainly.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "The user told me the number" | Check it against the primary record. |
| "The two sources are close enough" | Raise the gap as an open question. |
| "I counted them myself" | Have code count them, then recompute a few. |
| "The reference looks right" | Open it. A plausible citation is still a claim. |

## See Also

- research(research) — parent: ranking sources and keeping found apart from inferred
- writing(writing) — structure, audience, and format
- freshness/groundtruth(softwaredev) — a citation is a claim to verify against the cited artifact
- delivery/groundwork/claims(softwaredev) — a contribution's account of itself is a claim
