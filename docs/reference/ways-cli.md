# ways CLI Reference

The `ways` binary is the command-line interface for the ways knowledge guidance system. Each entry below covers three things: **when** to reach for the command, **where** to run it from, and **what it tells you**.

---

## Runtime & Observation

Use these to see what's happening in a session.

### `ways list`

**When:** After a Claude conversation turn, to verify which ways fired and in what order.

**Run from:** Anywhere — auto-detects the current session from `/tmp`.

**Tells you:** A table of every way that fired this session: epoch (turn number), match distance, trigger type (keyword / semantic / state / file / bash), re-disclosure eligibility, and which agent received it.

```
ways list
ways list --session <id>      # target a specific session
ways list --sort name          # sort alphabetically instead of by epoch
ways list --json               # machine-readable output
```

---

### `ways status`

**When:** First-time setup check; after updating the binary or corpus; when ways stop working entirely.

**Run from:** Anywhere.

**Tells you:** Binary paths, embedding model path and status (OK / missing), corpus entry count split by EN vs. multilingual, and per-project way counts for known projects.

```
ways status
ways status --json
```

---

### `ways context`

**When:** During a long session to see how much context window is consumed before compaction kicks in.

**Run from:** Inside an active Claude session — requires a live transcript. Run from the project directory so it finds the right transcript.

**Tells you:** Token counts for the current transcript — total, by role, and remaining budget. Use this before starting expensive multi-agent work to confirm you have room.

```
ways context
ways context --json
```

---

### `ways stats`

**When:** Reviewing which ways are actually being used across sessions; identifying ways that never fire (dead vocabulary); understanding how triggers break down.

**Run from:** The project directory to scope to that project. Add `--global` to see across all projects.

**Tells you:** Top ways ranked by fire frequency with ASCII bar charts, trigger-type breakdown (keyword / semantic / state / file / bash / check-pull), check fire summary, and session count. Add `--days N` to narrow the time window.

```
ways stats
ways stats --days 7
ways stats --global
ways stats --json
```

---

### `ways rethink`

**When:** After a session where guidance seemed off, to replay exactly what fired and when. Also useful for onboarding — walk through a past session to see the system in action.

**Run from:** Anywhere — launches an interactive session picker.

**Tells you:** An animated frame-by-frame replay of way firings across the session timeline, showing epoch, way name, and trigger at each step. `--list` skips the animation and gives a plain session table.

```
ways rethink                          # interactive picker
ways rethink --session <id>           # jump to a specific session
ways rethink --list                   # non-interactive session table
ways rethink --speed 500              # faster animation (ms per frame)
```

---

## Testing & Debugging

Use these to check whether a way fires and why.

### `ways scan prompt`

**When:** Testing whether a way fires for a given user message before actually asking Claude. This is the same code the `UserPromptSubmit` hook runs — no surprises.

**Run from:** Anywhere. Add `--project <dir>` to include project-local ways alongside global ones.

**Tells you:** The exact markdown content that would be injected into Claude's context. No output means nothing fires — the query didn't cross any threshold.

```
ways scan prompt --query "how do I test if a way is working" --session dummy
ways scan prompt --query "git commit" --session dummy --project ~/my-project
```

> **Note:** `--session` is required — pass any string (e.g., `dummy`) for a dry-run that doesn't affect real session state.

---

### `ways match`

**When:** A way isn't firing and you want to see its raw score; understanding why the wrong way is winning; checking whether a vocabulary change moved the needle.

**Run from:** Anywhere.

**Tells you:** A ranked table of all ways with their EN and multilingual cosine similarity scores for the query. Higher = closer match. The firing threshold is typically ~0.4–0.5 depending on the way's configuration.

```
ways match "how do I test if a way is working"
ways match "git commit message format"
```

---

### `ways embed`

**When:** Debugging at the embedding layer only — bypasses keyword matching. Useful when you suspect the keyword layer is overriding semantic scores, or you want to see the pure vector similarity without any boosts.

**Run from:** Anywhere.

**Tells you:** Raw embedding similarity scores without keyword boosting applied. Compare against `ways match` output for the same query to see the keyword layer's effect.

```
ways embed "security vulnerability scanning"
```

---

### `ways show way`

**When:** Verifying what content Claude actually receives when a way fires; checking whether session-aware idempotency is suppressing a way you expect to see.

**Run from:** Anywhere.

**Tells you:** The rendered markdown content of the way exactly as it would appear in the prompt, including any session-state-aware sections.

```
ways show way meta/knowledge
ways show way softwaredev/code/testing
```

---

### `ways reset`

**When:** A way should fire but hasn't (stale session marker); checks are firing too aggressively (inflated epoch counter); after editing a way mid-session and wanting a clean re-run.

**Run from:** Anywhere — targets the current session by default.

**Tells you:** Dry run by default — prints what state files would be cleared without deleting anything. Add `--confirm` to actually delete.

```
ways reset                    # dry run — shows what would be cleared
ways reset --confirm          # actually clear current session state
ways reset --session <id>     # target a specific session
ways reset --all --confirm    # clear all sessions
```

---

## Authoring

Use these when creating or maintaining ways.

### `ways template`

**When:** Creating a new way from scratch. Using the template ensures correct frontmatter structure, valid YAML, and locale stub files.

**Run from:** Project directory to create in `.claude/ways/`. Add `--global` to create in `~/.claude/hooks/ways/`.

**Tells you:** Scaffolds the way file at the given path with a frontmatter template, body placeholder, and vocabulary hints derived from the description.

```
ways template softwaredev/myteam/workflow -d "team deployment workflow and release process"
ways template itops/alerts -d "alerting runbooks" --global
```

---

### `ways lint`

**When:** After editing a way's frontmatter; before committing; in CI pipelines.

**Run from:** Project directory to scan all project ways. Pass a specific file path to lint just one file. Add `--global` to lint global ways instead.

**Tells you:** Validation errors and warnings per file against the frontmatter schema. `--fix` auto-corrects multi-line YAML and missing check sections. `--schema` prints the full frontmatter schema reference. `--check` exits non-zero for CI use.

```
ways lint                                          # scan project ways
ways lint ~/.claude/hooks/ways/meta/knowledge/knowledge.md  # single file
ways lint --fix                                    # auto-correct what's fixable
ways lint --check                                  # CI mode — non-zero exit on errors
ways lint --schema                                 # show the frontmatter schema
```

---

### `ways suggest`

**When:** A way exists but match scores are low for queries you expect it to catch. The vocabulary in frontmatter doesn't align with how users actually phrase things.

**Run from:** Anywhere — pass the absolute or relative path to the way file.

**Tells you:** Ranked list of vocabulary terms to add to the `vocabulary:` or `aliases:` frontmatter fields, based on term frequency analysis of the way body.

```
ways suggest ~/.claude/hooks/ways/meta/knowledge/knowledge.md
ways suggest .claude/ways/myteam/deploy/deploy.md
```

---

### `ways init`

**When:** Setting up ways support in a new project for the first time.

**Run from:** The project root directory.

**Tells you:** Creates the `.claude/ways/` directory structure and seeds a `MEMORY.md` template for the project.

```
ways init
ways init --project ~/my-other-project
```

---

## Tuning

Use these after ways are working to improve match quality and re-disclosure cadence.

### `ways tune`

**When:** After authoring locale stubs for multilingual support; auditing whether translations actually match the English content semantically.

**Run from:** Anywhere. Use `--way <substring>` to filter to a specific domain.

**Tells you:** Fidelity score (cross-lingual cosine similarity) and discrimination gap per way — how well the locale alias matches its English counterpart and how distinctly it scores against other ways. Flags entries below threshold as needing re-authoring.

```
ways tune
ways tune --way "meta/knowledge"
ways tune --fidelity-threshold 0.7    # stricter fidelity requirement
```

---

### `ways tune-curves`

**When:** A way fires too often or not enough relative to how useful it actually is; recalibrating re-disclosure decay curves from real firing data.

**Run from:** Anywhere. Requires at least 3 firing samples per way (configurable with `--min-fires`).

**Tells you:** Suggested `refire:` curve updates for each way based on observed cadence. Dry run by default — add `--apply` to rewrite the frontmatter in place.

```
ways tune-curves                   # dry run
ways tune-curves --apply           # rewrite frontmatter
ways tune-curves --way "softwaredev/code"
```

---

### `ways tune-precision`

**When:** Auditing whether ways are landing in irrelevant sessions — e.g., a `softwaredev` way firing during a writing session. Requires session history.

**Run from:** Anywhere. Requires 5+ sessions per way (configurable with `--min-sessions`).

**Tells you:** Off-domain fire rate per way. Ways at or above the flag threshold (default 50%) are marked for vocabulary tightening.

```
ways tune-precision
ways tune-precision --flag-threshold 0.3   # stricter — flag at 30% off-domain
ways tune-precision --way "itops"
```

---

### `ways siblings`

**When:** Checking if two ways are semantically too similar (risk of both firing for the same query, or one shadowing the other); validating that a new way is distinct enough from existing ones.

**Run from:** Anywhere. Pass `all` as the ID to get the full similarity matrix.

**Tells you:** Cosine similarity score between the target way and all other ways above the threshold. High similarity (>0.7) suggests vocabulary overlap that may need resolution.

```
ways siblings meta/knowledge
ways siblings softwaredev/code/testing
ways siblings all --threshold 0.5   # only show high-similarity pairs
```

---

## Analysis

Use these for structural and coverage insight across the ways corpus.

### `ways tree`

**When:** Understanding how a domain's progressive disclosure tree is structured; checking threshold and token-size settings across a subtree before editing.

**Run from:** Anywhere. Pass a way name or path (e.g., `softwaredev` or `meta/knowledge`).

**Tells you:** Hierarchical table showing depth, type (way / check), disclosure threshold, vocabulary count, and token size for each node in the subtree. Add `--jaccard` to see vocabulary overlap between siblings.

```
ways tree softwaredev
ways tree meta/knowledge
ways tree softwaredev --jaccard
```

---

### `ways corpus`

**When:** After adding or editing ways — the corpus is what `match` and `embed` query. Also run when `ways status` shows the corpus as stale.

**Run from:** Anywhere. Use `--if-stale` to skip the rebuild if no way files have changed since the last build (safe to add to CI pre-flight).

**Tells you:** Progress output during the rebuild. Writes a `.jsonl` corpus file to the XDG cache directory.

```
ways corpus
ways corpus --if-stale              # skip if current
ways corpus --quiet                 # suppress progress output
```

---

### `ways graph`

**When:** Visualizing the full ways knowledge graph in an external tool; generating data for dashboards or dependency analysis.

**Run from:** Anywhere.

**Tells you:** JSONL output (stdout by default) with node records (id, description, type) and edge records (parent → child relationships).

```
ways graph
ways graph -o ways-graph.jsonl     # write to file
```

---

### `ways language`

**When:** Before deploying to multilingual teams; checking which ways have locale stubs for a given language; auditing coverage gaps.

**Run from:** Anywhere. `--filter <lang>` to see only ways supporting a specific language. `--audit` for full per-way detail instead of the summary.

**Tells you:** Active language and model availability, corpus breakdown (EN vs. multilingual), language coverage across 17+ languages, and which ways are English-only vs. multilingual-routed.

```
ways language
ways language --filter fr          # French coverage
ways language --audit              # full per-way detail
```

---

### `ways provenance`

**When:** Auditing which ways have governance metadata (ADR links, control references, policy derivations).

**Run from:** Anywhere.

**Tells you:** List of ways with provenance sidecar files and their metadata — ADR references, control IDs, and verified dates.

```
ways provenance
```

---

## Administration

### `ways disable`

**When:** A global way keeps firing in a project where it isn't relevant (e.g., `itops/incident` showing up in a writing project).

**Run from:** The project root directory — writes the exclusion to `.claude/ways.yaml` in that project.

**Tells you:** Confirmation the way is added to the project's disabled list. `--list` shows currently disabled ways without making changes.

```
ways disable itops/incident
ways disable --list                 # see what's currently disabled
ways disable --list --names-only    # machine-readable list
```

---

### `ways enable`

**When:** Re-enabling a way that was previously disabled in the project.

**Run from:** The project root directory.

**Tells you:** Confirmation the way is removed from the disabled list.

```
ways enable itops/incident
```

---

### `ways config show`

**When:** Diagnosing unexpected matching behavior — thresholds too high/low, wrong language, unexpected disabled collections.

**Run from:** Anywhere.

**Tells you:** Full resolved configuration — default scope, language, matching thresholds, refire presets (frequent / normal / rare / once), disabled collections. This is the config the engine actually uses at runtime.

```
ways config show
ways config path                   # where the config file lives
ways config init                   # create config at XDG path if missing
```

---

### `ways permissions audit`

**When:** After adding `requires:` fields to way frontmatter; verifying Claude has the permissions those ways depend on.

**Run from:** Project directory to check against the project's `settings.json`. Add `--global` to check user-level settings instead.

**Tells you:** Per-way permission requirements vs. granted status — green for granted, red for denied. Use this to catch permission gaps before deploying a way to a team.

```
ways permissions audit
ways permissions audit --global
```

---

### `ways governance`

**When:** Compliance reporting; finding ways that lack ADR traceability; identifying ways with stale verified-dates; cross-referencing governance controls with firing activity.

**Run from:** Anywhere. Add `--global` to restrict to global ways.

**Tells you:** Depends on subcommand — see below. Add `--json` to any subcommand for machine-readable output.

| Subcommand | What it shows |
|------------|---------------|
| `report` | Coverage summary — how many ways have provenance vs. gaps |
| `gaps` | Ways with no provenance sidecar |
| `stale` | Ways with outdated `verified:` dates |
| `active` | Provenance cross-referenced with actual firing stats |
| `matrix` | Flat spreadsheet: way → control → justification |
| `lint` | Provenance integrity check |
| `trace <id>` | End-to-end provenance trace for a single way |
| `control <id>` | Which ways implement a given control |
| `policy <id>` | Which ways derive from a given policy |

```
ways governance report
ways governance gaps
ways governance trace meta/knowledge
ways governance matrix --json > coverage.jsonl
```

---

## Plumbing

These are used internally by hook scripts. You rarely need to call them directly, but they're useful when writing custom hooks or debugging the hook pipeline.

| Command | Used by |
|---------|---------|
| `ways scan command` | `PreToolUse` hook — fires ways based on bash commands Claude runs |
| `ways scan file` | `PreToolUse` hook — fires ways based on files Claude is editing |
| `ways scan state` | `UserPromptSubmit` + `SessionStart` — evaluates context-threshold and file-exists triggers |
| `ways scan task` | `SubagentStart` hook — injects ways into teammate/subagent sessions |
| `ways response-topics-path` | `PostToolUse` Stop hook — single source of truth for the response-topics state file location |
| `ways sessions-root` | Hook scripts — verifies all hooks resolve the same sessions root path |
