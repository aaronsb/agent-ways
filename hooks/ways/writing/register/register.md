---
description: Defining and holding a writing register for anything a person reads. Capture the user's style rules as one checkable clause, ban constructions and phrases by name, rewrite instead of swapping banned punctuation, honor a requested length, word cap, or shape exactly, cut a wordy draft or summary without dropping facts, and self-edit in two passes before delivery.
vocabulary: register style-guide house-style style-clause plain-language simplify shorter concise construction fragments cadence word-limit word-count readable eli5 semicolon em-dash en-dash mannered ai-isms slop trim cut banned-phrases banned-constructions writing-rules wordy summary word-cap tighten condense keep-facts
pattern: mannered|\beli(5|15)\b|\b(em|en).?dash|\bai.?isms?|workslop|\bai slop|way too long|too wordy|too many words|\b(under|less than|fewer than|no more than|max(imum)?( of)?) \d+ words|\b(my|your|our|a|the) (house )?style.?guide|\bhouse style\b
refire: 0.15
scope: agent, subagent
---
<!-- epistemic: convention -->
# Writing Register

A register is the set of rules a user sets for how prose reads. Once set, it applies everywhere a person reads the output. That includes chat, chat posts, email, docs, project instructions, commit messages, code comments, test cases, slides, UI copy, and image prompts. The prose defaults in the writing way give way to it.

## Write the style clause

- Capture the user's rules as one short clause in their words. Put it in the project instructions so every session gets it verbatim.
- Make each rule checkable. "Use em dashes sparingly" has no threshold. "No em dashes" can be searched for.
- Say what to do as well as what to avoid. A ban on long sentences with no positive rule pushes the writer into fragments and a robotic cadence.

Here is an example clause a user might set: "Write very simply, but without oversimplifying. Use short, complete, declarative sentences with one idea each. Use no analogies, metaphors, em dashes, semicolons, or sentence fragments."

## Punctuation

When the register bans a mark, rewrite the sentence. Do not swap in a near substitute. A colon or a double hyphen in the place of a banned dash keeps the same construction. Use a period, a comma, parentheses, or a real bullet instead. Leave the marks alone in the user's own titles and quotes.

## Banned lists

Keep two lists, one for constructions and one for phrases. Constructions are harder to spot, so name each one with a short template.

| Kind | Examples a user might ban |
|---|---|
| Constructions | "It's not X, it's Y", "Not X. Y.", "Not X, but Y", "the X is the Y", bullets that open with a bold label, taglines, aphorisms, fragment punchlines, cheesy closers, significance inflation, hero framing of the user |
| Phrases | seamless, revolutionary, unlock, supercharge, empower, delve, "I'd be happy to", "I hope this finds you well", "please don't hesitate", "circling back" |

A banned word may stay when it comes from the user's own text. Emojis can go on the list too.

## Length and shape

- Write much less than feels natural.
- When the user names a shape, such as "1 paragraph", "yes or no", or "3 bullets", deliver exactly that shape.
- Treat a cap such as "under 80 words" as hard. "Reduce by 30%" means cut 30% of the words.
- If a draft is still over the cap, cut more and offer a ranked list of the cheapest remaining cuts. Do not regrow earlier cuts.
- "Simpler" means removing duplicated framing. It does not mean compressing syntax.
- Write status updates and quick summaries as flat bullets of quick hits. Use a few key numbers and link the rest.
- A register can carve out exceptions. Essays, READMEs, and long write-ups often read better as flowing paragraphs with few headers.

## Markdown docs and decks

A register can set conventions for markdown too. Common choices are numerals instead of spelled-out numbers ("9", not "nine"), no period at the end of a bullet, and bold only where it helps scanning. Another is one line per paragraph with no hard wrap. Wrapped prose breaks exact-match edits.

## Reader and facts

- Keep every verifiable fact when you simplify. Reword instead of deleting.
- Assume the reader does not know internal tools. Spell out terms on first use.
- Describe events as the reader saw them.
- Show a skeptical reader the reasoning and the numbers.

## Self-edit before delivery

1. Cut decoration. This means significance clauses, counterweight contrasts, and recaps that repeat the body. Delete them outright.
2. Check the cap and both banned lists. Search the draft for each banned mark and expect zero hits. Confirm that you dropped no fact.

## See Also

- writing(writing) — structure, audience, and genre defaults that a register overrides
- trust/prose(meta) — decoration and the two-pass self-edit in detail
- trust/voice(meta) — whose voice to write in, a separate question from how it reads
- markdown/density(documentation) — the postcheck that counts what you just wrote
