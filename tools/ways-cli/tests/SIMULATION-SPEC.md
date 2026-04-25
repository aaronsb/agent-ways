# Ways Integration Test: Session Simulator

## Purpose

A deterministic simulator that exercises the full `ways` binary by replaying a synthetic Claude Code session. It pretends to be the Claude Code harness — sending hook events (UserPromptSubmit, PreToolUse:Bash, PreToolUse:Edit, etc.) to the `ways` binary and verifying that the correct ways fire at the correct times with the correct session state.

This is not a unit test for individual subcommands. This tests the **system behavior**: matching → showing → markers → re-disclosure → checks → epoch tracking, as experienced by a real session.

## What It Simulates

A session is a sequence of **turns**. Each turn has:

```yaml
- turn: 1
  type: prompt
  text: "refactor the authentication module for better testability"
  expect_ways:
    - softwaredev/code/quality       # semantic match on "refactor"
    - softwaredev/code/security/auth # semantic match on "authentication"
  expect_checks: []
  expect_epoch: 1

- turn: 2
  type: tool_bash
  command: "git commit -m 'refactor: extract auth middleware'"
  expect_ways:
    - softwaredev/delivery/commits   # commands: regex match on git commit
  expect_checks:
    - softwaredev/environment/makefile  # check: Makefile exists
  expect_epoch: 2

- turn: 3
  type: tool_edit
  file_path: "src/auth/middleware.ts"
  expect_ways: []                    # already shown in turn 1
  expect_checks: []
  expect_epoch: 3

- turn: 4
  type: prompt
  text: "now write tests for the auth middleware"
  expect_ways:
    - softwaredev/code/testing       # semantic match on "tests"
  expect_checks: []
  expect_epoch: 4
```

## Turn Types

| Type | Hook Event | What's sent to `ways` |
|------|-----------|----------------------|
| `prompt` | UserPromptSubmit | `ways scan prompt --query <text> --session <id>` |
| `tool_bash` | PreToolUse:Bash | `ways scan command --command <cmd> --session <id>` |
| `tool_edit` | PreToolUse:Edit/Write | `ways scan file --path <path> --session <id>` |
| `subagent_start` | SubagentStart | (future: `ways scan prompt` with subagent scope) |
| `response` | Stop | (epoch bump only, no matching) |

## What's Verified Per Turn

1. **Way firings** — which way IDs appeared in the output (compare against `expect_ways`)
2. **Check firings** — which check IDs appeared (compare against `expect_checks`)
3. **Epoch counter** — read `/tmp/.claude-epoch-{session}`, compare against `expect_epoch`
4. **Session markers** — verify markers exist for fired ways, don't exist for unfired
5. **Idempotency** — ways listed in `expect_ways` should NOT appear again in later turns unless token distance triggers re-disclosure
6. **No false positives** — ways NOT in `expect_ways` should not appear

## Session Scenarios

### Scenario 1: Basic Prompt Matching

```
Turn 1: "how do I write a unit test" → expect code/testing
Turn 2: "make it use mocks" → expect code/testing/mocking
Turn 3: "how do I write a unit test" → expect nothing (already shown)
```

Tests: semantic matching, idempotency, child-after-parent progressive disclosure.

### Scenario 2: Command Triggers

```
Turn 1: prompt "let's commit this"
Turn 2: bash "git commit -m 'fix: auth bug'" → expect delivery/commits
Turn 3: bash "npm install express" → expect environment/deps
Turn 4: bash "ssh user@server" → expect environment/ssh
```

Tests: `commands:` regex matching, multiple independent triggers.

### Scenario 3: File Edit Triggers

```
Turn 1: edit ".env" → expect environment/config
Turn 2: edit "src/api/routes.ts" → nothing (no files: pattern)
Turn 3: edit ".claude/ways/custom/custom.md" → expect knowledge/authoring
```

Tests: `files:` regex matching, project path filtering.

### Scenario 4: Check Scoring Curve

```
Turn 1: prompt about supply chain → expect code/supplychain (way fires)
Turn 2: bash "npm install sketchy-package" → expect supplychain.check (check fires, epoch_distance=1)
Turn 5: bash "pip install unknown" → expect supplychain.check (fires again, higher epoch_distance)
Turn 20: bash "cargo add foo" → expect supplychain.check (fires, but decay reduces effective score)
```

Tests: check epoch-distance factor, decay factor, fire count tracking.

### Scenario 5: Progressive Disclosure

```
Turn 1: prompt about code → expect code (parent)
Turn 2: prompt about testing → expect code/testing (child, threshold lowered 20%)
Turn 3: prompt about TDD → expect code/testing/tdd (grandchild)
```

Tests: parent-aware threshold lowering, tree depth tracking.

### Scenario 6: Scope Filtering

```
Turn 1 (agent scope): prompt → ways with scope:agent fire
Turn 2 (teammate scope): same prompt → only ways with scope:teammate fire
```

Tests: scope detection from teammate marker, scope field filtering.

### Scenario 7: When Preconditions

```
Turn 1 (project A): prompt → way with when.project=A fires
Turn 2 (project B): same prompt → that way does NOT fire
Turn 3 (project with Makefile): bash command → makefile check fires
Turn 4 (project without Makefile): same bash → check does NOT fire
```

Tests: `when:` project gate, `when:` file_exists gate.

### Scenario 8: Token-Gated Re-Disclosure (ADR-104)

```
Turn 1: prompt → way fires, token position stamped
... simulate 250K tokens of conversation ...
Turn N: prompt → same way fires AGAIN (re-disclosure)
```

Tests: token position tracking, context window detection, re-disclosure threshold.

### Scenario 9: Plugin Way Discovery (ADR-129)

Plugin ways are discovered from a session manifest (`plugin-ways.json`) written at session start. They sit between project-local and global in priority.

#### 9a: Basic plugin way fires on keyword match

```
Setup: plugin-ways.json manifest with one plugin pointing at a fixture plugin dir
Turn 1: prompt matching plugin way's pattern → expect plugin:{id}/{way} fires
```

Tests: plugin manifest reading, `collect_from_dir_with_prefix`, plugin way ID namespacing (`plugin:{id}/`).

#### 9b: Plugin way fires on semantic match

```
Setup: plugin way with description+vocabulary in corpus
Turn 1: prompt semantically matching plugin way → expect plugin way fires
```

Tests: corpus generation includes plugin ways, embedding scoring works for plugin-prefixed IDs.

#### 9c: Plugin way idempotency (fire-once)

```
Setup: plugin-ways.json manifest
Turn 1: prompt matching plugin way → fires
Turn 2: same prompt → does NOT re-fire (marker exists)
```

Tests: session markers work for `plugin:` prefixed IDs.

#### 9d: Plugin way does NOT fire when manifest is absent

```
Setup: no plugin-ways.json in session dir
Turn 1: prompt that would match a plugin way → nothing fires
```

Tests: graceful fallback when manifest is missing (no errors, no plugin ways).

#### 9e: Project-scoped plugin filtering

```
Setup: plugin-ways.json with a project-scoped plugin (scope=project, projectPath=/specific/project)
Turn 1 (project=/specific/project): prompt → plugin way fires
Turn 2 (project=/different/project): same prompt → plugin way does NOT fire
```

Tests: project-scoped plugins only contribute ways in their target project.

#### 9f: Plugin way does not shadow global way

```
Setup: plugin way with ID that differs from any global way
Turn 1: prompt matching both a plugin way and a global way → both fire
```

Tests: plugin and global ways coexist (different ID namespaces).

#### 9g: Global way still fires with empty plugin manifest

```
Setup: plugin-ways.json = [] (empty array)
Turn 1: prompt matching a global way → global way fires normally
```

Tests: empty manifest doesn't break the existing two-source discovery.

#### 9h: Plugin way check files

```
Setup: plugin with a .check.md file in hooks/ways/
Turn 1: command matching the check's commands pattern → check fires with score
```

Tests: `collect_checks_from_dir_with_prefix`, check scoring works for plugin ways.

#### 9i: Plugin way resolve for show

```
Setup: plugin-ways.json, plugin way fires
Verify: resolve_way_file("plugin:{id}/{way}", project_dir) returns the correct path
```

Tests: `resolve_plugin_install_path` reads installed_plugins.json, path reconstruction works.

#### 9j: Multiple plugins with ways

```
Setup: plugin-ways.json with two plugins, each having different ways
Turn 1: prompt matching plugin A's way → fires with plugin:A/{way} ID
Turn 2: prompt matching plugin B's way → fires with plugin:B/{way} ID
```

Tests: multiple plugin sources scanned, IDs namespaced independently.

#### 9k: Plugin macro trust gating

```
Setup: plugin way with macro: prepend and macro.sh
Turn 1: prompt matching plugin way → way fires but macro is skipped (not trusted)
```

Tests: plugin macros are not executed by default (security boundary).

### Scenario 9 Fixture Requirements

```
tests/fixtures/
├── ways/                          # existing fixture ways
├── plugin-a/                      # fixture plugin A
│   └── hooks/ways/
│       └── plugindomain/
│           └── fruit/
│               └── fruit.md       # pattern: banana|apple, refire: 0.15
├── plugin-b/                      # fixture plugin B
│   └── hooks/ways/
│       └── plugindomain/
│           └── color/
│               └── color.md       # pattern: red|blue|green, refire: 0.15
└── plugin-with-check/
    └── hooks/ways/
        └── plugindomain/
            └── audited/
                ├── audited.md
                └── audited.check.md

Session helper: Session::with_plugins(name, plugins: &[PluginFixture]) that:
  1. Creates plugin-ways.json in the session dir
  2. Points at fixture plugin dirs
  3. Optionally creates a fake installed_plugins.json for resolve tests
```

## Implementation Design

### Language

Rust (integration test in `tools/ways-cli/tests/`), or a standalone script. The simulator needs to:
- Set up a temp directory with a minimal ways tree (a few test ways)
- Create a test session ID
- Call `ways scan` for each turn, capture stdout
- Parse output for way IDs
- Check `/tmp` markers
- Clean up markers between scenarios

### Test Ways Tree

A minimal fixture set, not the full 90-way tree:

```
tests/fixtures/ways/
├── testdomain/
│   ├── parent/
│   │   ├── parent.md          # description + vocabulary for "code quality"
│   │   ├── child/
│   │   │   └── child.md       # description + vocabulary for "testing"
│   │   └── child2/
│   │       └── child2.md      # description + vocabulary for "refactoring"
│   ├── command-way/
│   │   └── command-way.md     # commands: ^git commit
│   ├── file-way/
│   │   └── file-way.md        # files: \.env$
│   ├── scoped-way/
│   │   └── scoped-way.md      # scope: teammate
│   └── gated-way/
│       └── gated-way.md       # when: { project: /specific/path }
├── testdomain/
│   └── with-check/
│       ├── with-check.md
│       └── with-check.check.md
```

Each test way has controlled vocabulary so embedding matching is deterministic.

### Corpus

The simulator generates a test corpus from the fixture ways (`ways corpus --ways-dir tests/fixtures/ways`) before running scenarios.

### Assertions

```rust
fn assert_ways_fired(output: &str, expected: &[&str]) {
    for way_id in expected {
        assert!(output.contains(&format!("<!-- way: {way_id} -->")),
            "Expected {way_id} to fire but it didn't");
    }
    // Also check no unexpected ways fired
}

fn assert_epoch(session_id: &str, expected: u64) {
    let path = format!("/tmp/.claude-epoch-{session_id}");
    let actual: u64 = std::fs::read_to_string(&path)
        .unwrap().trim().parse().unwrap();
    assert_eq!(actual, expected);
}

fn assert_marker_exists(way_id: &str, session_id: &str) {
    let name = way_id.replace('/', "-");
    let path = format!("/tmp/.claude-way-{name}-{session_id}");
    assert!(Path::new(&path).exists(),
        "Expected marker for {way_id} but it doesn't exist");
}
```

### Output Tagging

To make assertions reliable, `ways show` should emit a machine-readable tag (e.g., `<!-- way: softwaredev/code/quality -->`) in its output. This doesn't affect the human-readable content but gives the simulator something exact to grep for. Alternatively, the simulator can check markers (which are side effects of show) rather than parsing content.

**Recommendation:** Use markers for assertions, not output parsing. Markers are the source of truth for what fired. Content could change; markers are the state machine.

### Invocation

```bash
# Run all simulation scenarios
cargo test --test session_sim

# Or as a make target
make test-sim
```

### Coverage Metrics

After all scenarios run, report:
- Total ways exercised / total in fixture tree
- Scenarios with 0 assertion failures
- Turn types exercised (prompt, bash, edit, etc.)
- Session state features exercised (epoch, markers, re-disclosure, checks, scope, preconditions)

## What This Catches That Unit Tests Don't

- **Cross-turn state corruption** — marker from turn 1 affecting turn 5
- **Epoch drift** — counter incrementing wrong
- **Re-disclosure timing** — firing too early or too late
- **Check decay curve** — effective score going negative or NaN
- **Progressive disclosure** — child firing without parent lowering threshold
- **Scope leaks** — agent-scoped way firing for teammate
- **Precondition bypasses** — when: gate not blocking

## Not In Scope (For Now)

- Macro execution (macros are bash, tested separately)
- Embedding matching (requires GGUF model, tested by `test-embedding.sh`)
- Performance benchmarking (separate concern)
- Multi-session interaction (each scenario is independent)

## Build Order

1. Fixture ways tree with controlled vocabulary
2. Corpus generation from fixtures
3. Scenario 1 (basic matching + idempotency) — proves the harness works
4. Scenarios 2-3 (commands + files)
5. Scenario 5 (progressive disclosure)
6. Scenario 4 (check scoring)
7. Scenarios 6-7 (scope + preconditions)
8. Scenario 8 (re-disclosure) — requires fake transcript for token position
