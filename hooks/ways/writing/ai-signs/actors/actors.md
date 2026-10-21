---
description: rewriting a report, postmortem, status update or message so each sentence names who did what with a plain verb; fix passive and agentless sentences such as it was decided and mistakes were made, things doing people's actions such as the data shows, ideas counted like objects, and log-style fragments, while labels and captions stay labels
vocabulary: who did what passive voice passive constructions agentless active voice name the actor whose responsibility it was decided it has been noted mistakes were made was changed the data shows impact none completed reads like a log nominalization subject verb postmortem status update each action has a name it was determined who actually did
pattern: \b(agentless|de-?nominali[sz]\w*|nominali[sz]\w*)\b|\bwho (did|does) what\b|\bpassive (voice|constructions?|sentences?)\b|\bmistakes were made\b|\breads? like a (log|changelog|ticket)\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Who Did What

When rewriting prose so it says who acted, give each full sentence the person or team that did the thing and a plain verb. Name only people the facts give, or write "we" for the user's team. If the record does not say who acted, ask or leave the sentence passive. Do not guess a name.

## Four habits to rewrite

| Habit | Before | After |
|---|---|---|
| Agentless passive | It was decided to roll back | The on-call team decided to roll back |
| A thing does a person's action | The data shows a delay | We are two weeks behind |
| An idea counted like an object | Data import completion stands at 90 percent | We have imported 90 percent of the data |
| A noun or tool term used as a verb | We'll action this | We'll fix it this week |

## Do not overcorrect

- Labels, captions, tags and headings stay labels. A margin note reads "wrong weekday" with no subject added.
- Keep one thought in one sentence. "The draft got the weekdays wrong" beats "The draft added weekdays. They are wrong."
- Do not credit a script, tool or source for every number. Name who did it when the reader would ask.
- Replace a log-style fragment ("Review completed") with a noun phrase or a sentence a person would say.
- Leave quotes and quoted drafts word for word.

## See Also

- writing(writing) — active voice among the prose defaults
- writing/ai-signs(writing) — parent: other marks of generated prose
- trust/prose(meta) — decoration and self-explanation in long prose
