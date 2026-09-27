---
status: Accepted
date: 2025-12-30
deciders:
  - aaronsb
  - claude
related: []
---

# ADR-004: Way Macros for Dynamic Context Injection

## Context

### The Ways System Philosophy

The ways system is a **domain-agnostic guidance framework**. It provides automated, consistent guidance triggered by keywords, commands, and file patterns. While this repository ships with software development ways (GitHub, commits, testing), the mechanism itself is general-purpose.

A different user might have ways for:
- Excel/Office productivity (formulas, pivot tables, VBA macros)
- AWS operations (EC2, S3, IAM policies)
- Financial analysis (portfolios, tax lots, rebalancing)
- Research workflows (citations, data collection, peer review)

The ways system doesn't care about the domain—it just matches triggers and injects guidance.

### The Limitation

Static markdown guidance can't adapt to the user's actual environment. The way says "how to do X" but doesn't know "what X looks like right now."

**Examples across domains:**

| Domain | Way | What static misses |
|--------|-----|-------------------|
| Software dev | GitHub | Is this solo or team? Who are reviewers? |
| Software dev | SSH | Which tools are available? sshpass? ssh-agent? |
| AWS ops | IAM | Which account/region? Prod or dev? |
| Finance | Trading | Market hours? Account type? |
| Office | Excel | Which version? What add-ins? |

### The Insight

**Ways = guidance (the "how")**
**Macros = state detection (the "what is")**
**Combined = contextual guidance (the "how, given what is")**

Macros provide optional, domain-specific state detection that contextualizes way guidance for the user's actual environment.

## Decision

Extend the ways system to support **way macros**—shell scripts that generate dynamic context injected alongside static way content.

### Mechanism

1. **Frontmatter control**: New `macro:` field in way markdown
   ```yaml
   ---
   keywords: github|pull.?request
   macro: prepend           # Run {wayname}.macro.sh, prepend output
   ---
   ```

2. **Macro file convention**: `{wayname}.macro.sh` alongside `{wayname}.md`
   ```
   hooks/ways/
   ├── github.md            # Static guidance
   ├── github.macro.sh      # Dynamic state detection
   ├── ssh.md
   ├── ssh.macro.sh
   ├── commits.md           # No macro = static only
   ```

3. **Position options**:
   - `macro: prepend` - State context before guidance
   - `macro: append` - State context after guidance
   - No `macro:` field = current behavior (static only)

4. **Execution**: `show-way.sh` checks for macro field, runs script if present, combines output

### Macro Contract

Macros must:
- Be executable shell scripts
- Output markdown to stdout
- Handle missing tools gracefully (degrade, don't fail)
- Not require user interaction

### Execution Model

- **Once per session**: Macro runs when way triggers, output cached with way marker
- **Coupled to way**: If way doesn't fire (already shown), macro doesn't run
- **No runtime babysitting**: We don't enforce timeouts or output limits at runtime

### Macro Author Responsibilities

Authors are responsible for writing macros that:
- Don't infinite loop (internal code is trusted)
- Keep output reasonable (aim for < 20 lines)
- Timeout external calls appropriately
- Provide helpful context on failure, not silent exit

### Recommended Patterns

```bash
#!/bin/bash
# Pattern 1: Early exit if precondition not met
gh repo view &>/dev/null || {
  echo "**Note**: Not a GitHub repository"
  exit 0
}

# Pattern 2: Timeout external calls
RESULT=$(timeout 2 some_api_call 2>/dev/null)
[[ -z "$RESULT" ]] && {
  echo "**Note**: Could not reach API"
  exit 0
}

# Pattern 3: Trap and report errors with context
if ! DATA=$(some_command 2>&1); then
  echo "**Note**: Could not acquire info - $DATA"
  exit 0
fi

# Pattern 4: Degrade gracefully based on available tools
if command -v some_tool &>/dev/null; then
  echo "- some_tool available"
else
  echo "- some_tool not installed"
fi
```

### Output Guidelines

- No top-level headers (`#` or `##`) - the way provides structure
- Use bold for key info: `**Context**: Solo project`
- Keep concise - macro adds state, way provides guidance
- Always exit 0 (non-zero reserved for future error signaling)

### Example Macros by Domain

**Software dev (github.macro.sh)**:
```bash
#!/bin/bash
gh repo view &>/dev/null || { echo "**Note**: Not a GitHub repository"; exit 0; }

CONTRIBUTORS=$(timeout 2 gh api repos/:owner/:repo/contributors --jq 'length' 2>/dev/null || echo "0")

if [[ "$CONTRIBUTORS" -le 2 ]]; then
  echo "**Context**: Solo/pair project - PR optional, direct merge acceptable"
else
  echo "**Context**: Team project ($CONTRIBUTORS contributors) - PR recommended"
fi
```

**AWS ops (iam.macro.sh)**:
```bash
#!/bin/bash
ACCOUNT=$(timeout 2 aws sts get-caller-identity --query Account --output text 2>/dev/null)
[[ -z "$ACCOUNT" ]] && { echo "**Note**: AWS credentials not configured"; exit 0; }

REGION=$(aws configure get region)
echo "**Context**: Account $ACCOUNT, Region $REGION"

if [[ "$ACCOUNT" == "123456789" ]]; then
  echo "- ⚠️ This is PRODUCTION"
fi
```

**Office (excel.macro.sh)**:
```bash
#!/bin/bash
if command -v xlsx2csv &>/dev/null; then
  echo "- xlsx2csv available for data extraction"
fi
if [[ -f "$FILE" && "$FILE" =~ \.xlsm$ ]]; then
  echo "- ⚠️ Macro-enabled workbook - VBA content present"
fi
```

### Testing

Add `hooks/ways/tests/` directory with validation:
- `test-frontmatter.sh` - Validate all ways have valid frontmatter
- `test-macros.sh` - Validate macros are executable and exit cleanly
- `test-triggers.sh` - Validate keyword/command patterns

### Project-Local Macros

Same precedence rules as ways:
- Project-local macro (`$PROJECT/.claude/ways/foo.macro.sh`) shadows global
- If project-local way exists but no project-local macro, global macro does NOT run
- Macro is coupled to its way - they travel together

```
Lookup order:
1. $PROJECT/.claude/ways/foo.md + foo.macro.sh (if exists)
2. ~/.claude/hooks/ways/foo.md + foo.macro.sh (if exists)

No mixing: project-local way with global macro is not supported.
```

## Consequences

### Positive
- Ways become environment-aware without losing domain-agnosticism
- Guidance adapts to actual state (solo vs team, prod vs dev, tools available)
- Framework remains simple: bash + jq, no new dependencies
- Maintains backward compatibility (no macro = current behavior)
- Users can create macros for any domain, not just software dev

### Negative
- Additional complexity in show-way.sh
- Macros add execution time (mitigated by once-per-session caching)
- More files to maintain per way

### Neutral
- Macros are optional - ways work without them
- Testing infrastructure needed
- Documentation for macro authors needed
- **Security**: Macros execute arbitrary shell code. Project-local macros are trusted by convention (same as project-local ways). Users cloning untrusted repos should review `.claude/ways/` contents.

## Alternatives Considered

### 1. Hook-based injection (instead of macros)

Use existing `UserPromptSubmit` hook to run detection scripts and prepend context to prompts.

**Why rejected**: Hooks are event-driven, not way-coupled. Would require duplicating trigger logic. Macros are semantically "part of a way"—the state detection is specific to that way's domain.

### 2. Template engine in way files (Jinja-style)

Embed logic directly in way markdown:
```markdown
{% if contributors <= 2 %}
**Context**: Solo project
{% endif %}
```

**Why rejected**: Requires a template engine dependency. Bash is already available and more powerful. Template syntax is limiting for real environment detection. Violates "bash + jq only" philosophy.

### 3. Sandboxed scripting (Lua, WASM)

Use a sandboxed language that's safer and more portable than shell.

**Why rejected**: Adds significant complexity and dependencies. Macro authors are trusted (same as way authors). The security boundary is at the project level, not the macro level.

### 4. Embed logic in frontmatter

Extended frontmatter with conditional fields:
```yaml
---
keywords: github
context_if: gh repo view
context_text: "GitHub repo detected"
---
```

**Why rejected**: Too limited. Can't do contributor counting, tool detection, or complex state queries. Frontmatter should declare triggers, not implement behavior.

### 5. Always-run convention (no frontmatter opt-in)

If `foo.macro.sh` exists alongside `foo.md`, always run it.

**Why rejected**: Less control. Some ways may want the macro file present but conditionally disabled. Explicit `macro:` frontmatter is clearer and allows prepend/append control.

## Implementation Plan

1. Update `show-way.sh` to parse `macro:` frontmatter
2. Implement prepend/append logic with output combination
3. Create `github.macro.sh` as proof of concept
4. Create `ssh.macro.sh` to demonstrate tool detection pattern
5. Add `hooks/ways/tests/` validation suite
6. Update `knowledge.md` way with macro authoring documentation
7. Update README with macro documentation for other-domain users
