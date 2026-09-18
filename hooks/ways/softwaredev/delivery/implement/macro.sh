#!/usr/bin/env bash
# Implementation way macro — progressive disclosure of parallelization guidance
#
# Lightweight: the way file always shows the briefing protocol
# This macro adds detailed planning tables only when the project
# signals enough complexity to warrant parallelization thinking.

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$PWD}"

# --- Complexity signals ---
has_adrs=false
file_count=0

# Check for ADRs (suggests architectural work, not a quick fix)
if [[ -d "$PROJECT_DIR/docs/architecture" ]]; then
  adr_count=$(find "$PROJECT_DIR/docs/architecture" -name "ADR-*.md" 2>/dev/null | wc -l)
  [[ "$adr_count" -gt 0 ]] && has_adrs=true
fi

# Estimate project size (rough proxy for parallelization relevance)
if command -v find >/dev/null 2>&1; then
  file_count=$(find "$PROJECT_DIR" -maxdepth 3 -name "*.md" -o -name "*.py" -o -name "*.js" -o -name "*.ts" -o -name "*.go" -o -name "*.rs" -o -name "*.sh" -o -name "*.c" -o -name "*.h" 2>/dev/null | wc -l)
fi

# --- Decide disclosure level ---
# Show full parallelization guidance if any complexity signal fires
if $has_adrs || [[ "$file_count" -gt 20 ]]; then
  cat <<'EOF'

## Safe Parallelization

Enter plan mode and think through work collision **before** creating tasks. Defend each parallelization decision — if you can't articulate why two tasks won't collide, they're sequential.

### Collision Analysis

| Risk | Example | Verdict |
|------|---------|---------|
| Same file | Two tasks editing `config.yaml` | Sequential — never parallelize |
| Import chain | Task A edits module, Task B edits its caller | Sequential — interface changes cascade |
| Independent modules | New utility + new test fixtures | Safe to parallelize |
| Read-only research | Investigating patterns across codebase | Safe as subagent (no edits) |
| Independent test files | Tests for unrelated features | Safe to parallelize |

### Isolation Strategies

| Strategy | When to Use | How |
|----------|-------------|-----|
| **Worktree** | Tasks editing different file sets with no shared imports | `Agent(isolation: "worktree")` |
| **Subagent** | Research, review, analysis — read-only work | `Agent(subagent_type: ...)` |
| **Sequential** | Tasks with file overlap or dependency chains | One after another in main context |

### Rules

- **When in doubt, go sequential.** Collision cost far exceeds waiting cost.
- **Never parallelize tasks that touch the same file.** Not even "different sections."
- **Worktree tasks must be self-contained.** They can't see main-tree edits until merged back.
- **Subagents for research are always safe.** Read-only work can always run in parallel.

### Execution Order

1. Start sequential tasks first (critical path)
2. Launch parallelizable tasks together when dependencies are met
3. Review worktree changes before integrating
4. Run tests after each integration point, not just at the end
EOF
else
  # Lightweight hint — don't dump tables for small/simple projects
  echo ""
  echo "For multi-file work, enter plan mode first and think about whether tasks can safely run in parallel (worktrees for independent file sets, subagents for read-only research, sequential for anything that shares files)."
fi
