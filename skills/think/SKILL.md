---
name: think
description: Structured reasoning strategies (tree-of-thoughts, trilemma, self-consistency, step-back, ReAct). Use when facing complex decisions, weighing trade-offs, comparing multiple approaches, balancing competing objectives, or stuck. `/think <mode>` runs one strategy; `/think` alone shows the menu.
allowed-tools: Read, Bash, Glob, Grep
argument-hint: [tree|trilemma|consistency|stepback|react]
---

# Think Strategies

One skill, five structured reasoning strategies. Each surfaces your reasoning step-by-step instead of jumping to an answer. The think way's metacognitive check normally escalates here on its own; `/think` is the manual override — use it to **force** an external strategy, **choose** a specific one, or make the reasoning **visible**.

## Modes

| Mode | Strategy | When |
|---|---|---|
| `tree` | Tree of Thoughts | Multiple viable approaches — branch, evaluate, prune, select |
| `trilemma` | Trilemma | Three competing objectives — satisfice, don't optimize all |
| `consistency` | Self-Consistency | High-stakes — run independent paths, take consensus |
| `stepback` | Step-Back | Stuck — abstract to principles, then apply back |
| `react` | ReAct | Investigation / debugging — reason → act → observe |

**`/think` with no mode:** present this table, ask which shape fits the problem, then run that mode.

## Running a mode

Each mode maps to a strategy id (written to the session file) and a definition file under `hooks/ways/meta/think/strategies/`:

| Mode | Strategy id | Definition file |
|---|---|---|
| `tree` | `tree-of-thoughts` | `tree-of-thoughts.md` |
| `trilemma` | `trilemma` | `trilemma.md` |
| `consistency` | `self-consistency` | `self-consistency.md` |
| `stepback` | `step-back` | `step-back.md` |
| `react` | `react` | `react.md` |

**1. Session guard** — only one think session at a time:

```bash
cat /tmp/.claude-think-session 2>/dev/null
```

If a session is active, ask the user to finish or abandon it before starting another. Do NOT start a second concurrent session.

**2. Register** the chosen strategy id:

```bash
echo "<strategy-id>" > /tmp/.claude-think-session
```

**3. Work the stages.** Read the strategy definition and follow its numbered stages in order — present your work for each stage before moving to the next:

```bash
cat ~/.claude/hooks/ways/meta/think/strategies/<definition-file>
```

**4. Complete or abandon.** After the final stage — or if the user says "never mind" / "skip it" / changes topic — clean up so the think way can fire again for new problems:

```bash
rm -f /tmp/.claude-think-session /tmp/.claude-way-meta-think-*"${CLAUDE_SESSION_ID:+-$CLAUDE_SESSION_ID}" 2>/dev/null
```

## Not for

- Routine work that doesn't need structured reasoning — the strategies add deliberation overhead. Reach for them on genuinely hard problems (branching approaches, competing objectives, high-stakes calls, being stuck), not every task.
