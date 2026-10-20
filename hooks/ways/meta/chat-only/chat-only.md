---
description: The user wants your thoughts first and will decide later what gets made. Hold off on any changes this turn. Answer in chat only with no files, edits, plan files, tickets, drafts, or sends, or do exactly the one named step, report briefly, and stop to await further instructions.
vocabulary: chat-only here-in-chat return-thoughts reflect brainstorm align-first no-edits no-action-required await-further-instructions just-confirm whether-possible understand-the-ask don't-do-it-yet don't-jump-ahead just-answer your-understanding findings-first read-only-turn thoughts-first hold-off decide-later no-changes-yet think-before-acting
pattern: in chat (here )?only|(return|answer|respond|reply|present|align)\b[a-z ,']{0,30}in chat\b|just here,? in chat|write it here in chat|return (your|my) (thoughts|understanding)|await (further|commit) instructions|without (editing|creating|updating|deleting|modifying)(,? (or )?(editing|creating|updating|deleting|modifying))*,? (or )?any(thing)?\b|without making any (edits|changes)|(do not|don'?t) (draft|write) anything|no edits allowed|no action required|think \(not act|just (read and )?reflect\b|confirm whether (it'?s |this is )?possible|(must not|do not|don'?t) jump ahead
refire: 0.15
scope: agent
---
<!-- epistemic: constraint -->
# Chat Only, One Step, Then Stop

Some users think in chat first and decide later what gets persisted or sent. Phrasings include "in chat only", "return your thoughts here", "just reflect", "no edits allowed", "think, don't act", and "without editing, updating, deleting, or creating anything".

## Thinking turns

- Reading and querying are fine. Writing anything is not. That means no files, report files, plan files, docs, tickets, comments, drafts, approvals, calendar changes, or sends in any system.
- Investigate, then report in chat. Give a short plain readout with sources, options, and a recommendation. Then wait for the user to choose. Ask a structured question only when you truly need one.
- "Could we..." and "just confirm whether it's possible" mean check feasibility and report back. Do not run the action.
- A braindump that ends with "just read and reflect" gets your understanding in chat before anything is built. Do not offer drafts or edits during a thinking task.
- Shared live documents stay read-only.
- If the user leaves plan mode with "just present your thoughts in chat", write no plan file. If they interrupt a file write with "I mean just here in chat", stop and answer inline.
- For persuasive deliverables, lay out the reasoning in chat as indented bullets first. Write the document only after the user approves that logic.

## Step boundaries

"Then await further instructions", "return your understanding", "create the feature branch", and "finish the deploy, then wait for commit instructions" each name one step.

- Do exactly that step. Summarize what you did, what you understood, and any weak spots. Then stop.
- Do not commit, configure, analyze, or start the next phase. Do not offer to start it or pre-stage it either.
- When context arrives in stages, give only your current understanding.
- A requested length is exact. "One concise paragraph" means one paragraph.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "A quick notes file will help them review" | They asked for chat. Put it in the reply. |
| "The next step is obvious, so I'll start it" | The user named one step. Report and wait. |
| "I'll just stage the edit so it's ready" | Staging is a write. Describe the edit instead. |

## Boundary

This way governs the turn, not the topic. Building a read-only guard hook or writing a read-only subagent brief is ordinary build work.

## See Also

- choices(meta) — presenting options once the readout is done
- trust/delegation(meta) — hard boundaries on sends and writes through borrowed accounts
