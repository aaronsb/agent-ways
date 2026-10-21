---
description: Rewriting a report, status update, or message so each sentence says who did what with a plain verb. Fixes things doing people's actions (the data shows), ideas counted like objects (cost impact none), log-style fragments (migration completed), and tool nouns used as verbs, while captions and labels stay labels.
vocabulary: who did what actor subject verb person team sentence passive nominalization agentless fragment log-style label caption tag heading the-data-shows impact completed sounds-like-a-person
pattern: \b(objectif\w*|de-?nominali[sz]\w*|nominali[sz]\w*|agentless|noun (labels?|stacks?)|who (did|does) what)\b|\bsays? who did (what|each thing|it)\b|\breads? like a (log|changelog|ticket)\b
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: convention -->
# Who does what

In a full sentence, say who did the thing and use a plain verb. Name only people the facts give, or write "we" for the user's team. This rule is for prose. Labels, captions, tags, and headings stay labels.

## 3 habits to rewrite in prose

| Habit | Before | After |
|---|---|---|
| A thing does a person's action | What round 1 taught us | What we learned in round 1 |
| | The data shows a delay | We are 2 weeks behind |
| An idea is held or counted like an object | Cost impact: none. | The cost does not change. |
| | Data import completion stands at 90 percent | We have imported 90 percent of the data |
| A noun or tool term is used as a verb | We'll action this | We'll fix it this week |
| | The judges flagged 3 claims | The judges listed 3 claims |

## Do not overcorrect

- Do not staple a subject onto a label. A margin note reads "wrong weekday", not "The draft added weekdays. They are wrong."
- Do not split one thought into 2 sentences to give each a subject. "The draft got the weekdays wrong" beats "The draft added weekdays. They are wrong."
- Do not credit a script, tool, or source for every number. Name who did it only when the reader would ask.
- Replace log-style fragments ("Weekdays added", "Both errors caught") with a plain noun phrase ("Wrong weekdays") or a sentence a person would say.
- Leave quotes and quoted drafts word for word.

Read each sentence aloud. If nobody would say it that way to this reader, rewrite it.

## See Also

- writing(writing) — active voice and the revision passes
- trust/prose(meta) — decoration and self-explanation in long prose
