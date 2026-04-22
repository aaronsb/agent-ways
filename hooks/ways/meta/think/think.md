---
description: structured reasoning, thinking frameworks, cognitive scaffolding for complex decisions
vocabulary: explore options approaches trade-off balance alternatives stuck principle abstract reasoning framework systematic
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Structured Thinking

When you encounter complexity, don't reach for a framework first. Evaluate whether you need one.

## The Metacognitive Check

Before solving, pause and assess: **is your understanding trending toward clarity or away from it?**

Do not attempt to solve in this first cycle. Just evaluate the direction:

1. **Trending clear** — You can see the shape of the answer. Proceed normally. No scaffolding needed.
2. **Trending unclear** — The problem has competing concerns, hidden dependencies, or you're uncertain which direction to go. Escalate.

## Escalation Gradient

| Level | What happens | When |
|---|---|---|
| **Internal reasoning** | Think harder silently — extend your reasoning, consider more angles | Unclear but likely resolvable with more thought |
| **External strategy** | Use a structured strategy (below) — surfaces your reasoning step-by-step | Internal reasoning isn't converging; the human should see the work |
| **Collaborative** | Discuss with the human — they have context you lack | Strategy hits unknowns that tools can't resolve |

Most problems resolve at level 1. The strategies exist for when they don't.

## External Strategies

**When you decide to escalate, act immediately.** Invoke the skill — don't announce your intention, don't ask permission, don't hedge with "I might want to use..." The decision to escalate IS the decision to act. The human cannot follow your reasoning speed; by the time they'd read a proposal to use a strategy, you should already be working through it.

| Problem Shape | Strategy | Invoke |
|---|---|---|
| Multiple viable approaches | Tree of Thoughts | `/think-tree` |
| Three competing objectives | Trilemma | `/think-trilemma` |
| High-stakes, need confidence | Self-Consistency | `/think-consistency` |
| Stuck, need first principles | Step-Back | `/think-stepback` |
| Investigation or debugging | ReAct | `/think-react` |

Each strategy is a step-by-step scaffold that surfaces your reasoning visibly.

## Engaging the Human

During any strategy, if you encounter unknowns that your tools can't resolve, use `AskUserQuestion` to engage the human. Your job is to **frame the unknown precisely** — not "what should I do?" but:

- "I've narrowed this to X or Y. Here's what I can't determine — which is true in your context?"
- "This depends on [specific constraint] that I don't have visibility into."
- "I've identified three approaches with these trade-offs. Which trade-off is acceptable?"

Your reasoning speed and breadth does the framing work. The human's context and ground truth fills the gap. That exchange — the agent framing, the human resolving — is the actual collaboration.

## See Also

- trust/autonomy(meta) — thinking strategies support autonomous reasoning
