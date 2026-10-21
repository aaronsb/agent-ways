---
description: Weak sourcing in a draft a person reads. Fix vague attribution like experts argue, borrowed authority, hedges about missing information, and vague associations between people or firms. Open every link, DOI, ticket key, and quote before citing it, remove references you cannot confirm, strip leftover chatbot citation tokens and tracking parameters, and fill every placeholder.
vocabulary: citation cite footnote bibliography reference doi isbn attribution provenance sourcing link url fact-check verify quote placeholder oaicite hallucinated weasel experts-argue industry-reports unverified
pattern: oaicite|contentreference|turn0search|utm_source=chatgpt|\b(experts|analysts|critics|observers|scholars) (argue|agree|note|say|believe)\b|\b(does|do|did) (this|that|these|the) (reference|citation|source|quote|study|paper)s? (really |actually )?exist\b|\bfact.?check (this|the|these|my|that|each|every|all)\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: constraint -->
# Sources and claims

Name a source the reader can check for each claim, or state the point as your own view.

## Check before you cite

- Open each link, ticket key, and DOI you cite. Confirm it says what you claim. Check each quote against its source.
- Keep a link or key the user gave you even if you cannot open it. Say that you could not check it.
- Remove links and references you produced and could not confirm. Never invent a replacement.
- Give a page for each book. Find the exact sentence before you write "clearly states".

## Signs to fix

| Kind | Examples |
|---|---|
| Vague attribution | "experts argue", "some critics argue", "industry reports", "analysts note", "several sources" for one |
| Borrowed authority | "[Source] highlighted" or "underscores" when the source does not say it |
| Gap hedges | "specific details are limited", "not widely documented", "in the available sources" |
| Vague connection | a person or firm "associated with" another, with the real role unstated |
| Tool leftovers | oaicite, turn0search0, [cite: 1], [web:1], grok_card, :::writing, Name+2, 【85†L261】, utm_source=chatgpt.com |
| Placeholders | [Your Name], 2025-xx-xx |

## Fixes

- Name the person, document, or dataset behind each claim.
- Attribute only what the source states. Mark your own reading as yours.
- Use "such as" only when the source names more examples than you list.
- When a source lacks a fact, say which source you checked. Do not add what the answer likely is.
- State the real role ("was CEO of") in place of "associated with".
- Strip tool tokens and tags. Fill each placeholder or ask the user.

## See Also

- research(research) — source authority and synthesis
- writing(writing) — audience and revision passes
