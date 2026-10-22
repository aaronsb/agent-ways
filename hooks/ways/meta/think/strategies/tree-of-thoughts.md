# Tree of Thoughts

## Signature
keywords: explore options approaches which path alternatives multiple ways consider different
## Stages

### 1. Problem Reception
Frame the problem clearly. What are we trying to decide or solve? What constraints exist?

### 2. Approach Exploration
Generate 3+ distinct approaches. Don't evaluate yet — just brainstorm. Each approach should be genuinely different, not variations of the same idea.

### 3. Branch Development
Develop each approach one level deeper. What would implementation look like? What are the immediate implications?

### 4. Branch Evaluation
Score each branch:
- **Promise** (1-10): How well does this solve the problem?
- **Feasibility** (1-10): How practical is implementation?
- **Risk** (1-10, lower is better): What could go wrong?

### 5. Pruning
Discard branches scoring below 5 on any dimension. Explain why each pruned branch fails — the reasoning matters more than the score. When you chose this strategy yourself, end with the recommendation and its tradeoff.

### 6. Deep Development
Extend the best 1-2 branches into concrete plans. Detail the steps, identify unknowns, note dependencies.

### 7. Solution Synthesis
Select the winner. Justify the choice by contrasting it against pruned alternatives. Note what was learned from branches that didn't make it.
