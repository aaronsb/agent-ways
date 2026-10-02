# ways CLI Reference

The `ways` binary is the command-line interface for the ways knowledge guidance system. Each entry below covers three things: **when** to reach for the command, **where** to run it from, and **what it tells you**.

---

## Runtime & Observation

Use these to see what's happening in a session.

### `ways list`

**When:** After a Claude conversation turn, to verify which ways fired and in what order.

**Run from:** Anywhere — auto-detects the current session under the sessions root (`ways sessions-root` prints it).

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
ways context --session <id>   # pin to one session instead of guessing from cwd
ways context --project <dir>  # resolve the transcript for another project
ways context --json
```

**`--json` output** carries `window_source` alongside `tokens_total` (ADR-166): `model_table` (the model was recognized), `env_override` (`CLAUDE_CONTEXT_WINDOW` was set and beat detection), or `default` (the model was **not** recognized and a conservative 200K was assumed — the percentage is then a guess, not a measurement). `CLAUDE_CONTEXT_WINDOW` overrides detection on every model.

---

### `ways stats`

**When:** Reviewing which ways are actually being used across sessions; identifying ways that never fire (dead vocabulary); understanding how triggers break down.

**Run from:** The project directory to scope to that project. Add `--global` to see across all projects.

**Tells you:** Top ways ranked by fire frequency with ASCII bar charts, trigger-type breakdown (keyword / semantic / state / file / bash / check-pull), a per-model breakdown, ways per hook invocation, check fire summary, and session count. Add `--days N` to narrow the time window.

```
ways stats
ways stats --days 7
ways stats --global
ways stats --project <dir>   # filter to one project path
ways stats --json
```

**By model:** fires and re-disclosures split by the model id stamped on each event at fire time (the `model` field, read from the transcript the invoking hook named). A second table shows the same top ten ways with one column per model. Two buckets are not model ids. `unknown` means no model was resolved for the fire: the scan ran without `--transcript` (a dry run, the task/SubagentStart lane, or a hook predating the flag), the named transcript could not be read, or it had no assistant turn yet. The model is never taken from a session-id or project lookup, since for a subagent those resolve the parent's or a sibling's transcript. `(unstamped)` means the row predates the field. This is identification only: no way is gated or presented differently by model.

**Ways per hook invocation:** how many ways one hook call delivered, per channel (`prompt`, `bash`, `file`, `state`, ...). An invocation is approximated as the `way_fired` rows that share a session, an agent (`agent_id`, since subagent hooks report the parent's session id), a timestamp (one-second resolution), and a channel; the table gives the invocation count, how many fired 1, 2, 3, or 4+ ways, and the maximum. Rows written before `agent_id` existed still merge parallel agents under one session, so on historical data the 4+ tail is an upper bound. The prompt channel folds the keyword and semantic prompt matchers together, and the bash channel folds `bash` with `semantic:bash:*`.

**`--json` output** adds `by_model` (`{model: {fires, redisclosures}}`), `by_way_model` (`{way: {model: fires}}`), and `ways_per_invocation` (`{channel: {invocations, "1", "2", "3", "4+", max}}`) beside the existing keys.

---

### `ways introspect`

**When:** After a session where guidance seemed off, to replay exactly what fired, when, and why. Also useful for onboarding: walk through a past session to see the system in action, or follow the current one as ways fire.

**Run from:** The project directory, which scopes the sessions to that project. `--project <dir>` picks another project and `--all` takes every project; when the current project cannot be detected, the command fails rather than reading every project.

**Tells you:**

- `replay` opens a session's timeline on screen: the ways active at each frame, with epoch, distance, trigger, re-disclosure forecast and the token gauge, and a scrubber across the session's frames and compaction windows. ←→ move a frame, space plays, `+`/`-` set the speed, ⏎ or Tab opens why the selected way fired (its trigger, criteria, matched span and the way's own text), Esc goes back and `q` quits. Without `--session` it opens on a picker of the sessions in scope, newest first, which also shows whether Claude Code still holds each session's transcript.
- `replay --json` writes the reconstructed timeline as a single JSON document instead, for agents, scripts and CI, where no terminal is needed.
- `live` follows the session that is writing events now, on the same screen, and stays on the newest frame until you move back; space or End resumes following.
- `list` prints the session table; `--json` gives it as data.
- `dump` writes the session's introspection model as JSON: turns, fired ways, their criteria, the keyed transcript join and matched spans.
- `fires` lists the semantic fires of a session with their scores, lowest first.

```
ways introspect replay                                 # picker of this project's sessions
ways introspect replay --session <id>                  # one session
ways introspect replay --project <dir>                  # sessions from another project
ways introspect replay --all                           # sessions across every project
ways introspect replay --speed 500                     # faster playback (ms per frame)
ways introspect replay --json                          # most recent session as JSON
ways introspect replay --session <id> --json           # a specific session as JSON
ways introspect live                                   # follow the active session
ways introspect list                                   # session table
ways introspect list --json                            # session list as data
ways introspect dump --session <id>                    # introspection model as JSON
ways introspect fires --session <id> --max-score 0.6   # the borderline semantic fires
```

**`replay --json` output:** a single object with `session`, `project`, `context_window_k`, a `summary` (epoch count, duration, distinct ways, total fires, re-disclosures, checks, near-misses, trigger breakdown, top ways, and the relevance gate's work), the full `frames` timeline (each with epoch, timestamp, token position, active ways, and what newly fired that turn), and `near_misses` (ways whose calibrated probability came within `near_miss_margin` of the semantic firing threshold but didn't fire — each with its EN/multilingual relevance probabilities (`prob_en` / `prob_multi`), the global semantic threshold `tau_s`, and the `margin` below it). The screen omits near-misses. Slice large sessions with `jq` — a multi-day session can run to thousands of frames.

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

**`--transcript <path>`** (also on `scan command`, `scan file`, and `scan state`): the transcript the hook payload names as `transcript_path`. Fired ways read the invoking agent's model id from it and stamp the event with a `model` field (and an `agent_id`), and resolve their `refire:` window from the same read. The hooks pass it automatically. Without it the binary still locates a transcript by session id for the window, but stamps `model: unknown`: that lookup finds the parent's transcript for a subagent, so it is not trusted for the model.

---

### `ways match`

**When:** A way isn't firing and you want to see why; understanding why the wrong way is winning; checking whether a vocabulary change moved the needle. This is the authoring tool: it shows how a query matches under the live matcher.

**Run from:** Anywhere. It covers global ways plus the project-local ways of `--project <dir>` (default: the current directory).

**Tells you:** The late-interaction diagnostic (ADR-160). A header line gives the gates in force (admit on share or peak, then confirm) and how many ways would fire, followed by the reduced surface the query was chunked from. Then, for the top 20 candidates ranked by share:

| Column | Meaning |
|--------|---------|
| `peak` | The way's strongest single-chunk cosine |
| `share` | Softmax mass the way won across chunks — what the share gate reads |
| `confirm` | Best match of the way's own body prose against the chunk it won (`—` when not admitted) |
| `outcome` | `fired ✓`, `< gate` (admitted by neither share nor peak), or `< confirm` (admitted, but the body did not corroborate) |
| `won chunk` | The surface chunk the way matched on |

When the query is too sparse to chunk, or the embedding engine cannot run late interaction, it says so on stderr and prints the single-vector view instead, mirroring the fire path's fail-safe.

On the fallback path the cosines are mapped through the calibrated logistic `g(s)` and fire when `g(s) ≥ τ_s`. See `../hooks-and-ways/engine-reference.md`.

```
ways match "how do I test if a way is working"
ways match "git commit message format" --project ~/my-project
```

---

### `ways show way`

**When:** Verifying what content Claude actually receives when a way fires; checking whether session-aware idempotency is suppressing a way you expect to see.

**Run from:** Anywhere. `--session` is required; pass any string (e.g., `dummy`) for a dry run that doesn't touch a real session's state.

**Tells you:** The rendered markdown content of the way exactly as it would appear in the prompt, including any session-state-aware sections. Because it is session-aware, a second call with the same session id prints nothing: the way has already been shown.

```
ways show way meta/knowledge --session dummy
ways show way softwaredev/code/testing --session dummy
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

**Tells you:** Scaffolds the way file at the given path with a frontmatter template, body placeholder, and locale stubs. `-d` (description) is required; `-V` sets the vocabulary; `--scope` sets `agent`, `subagent`, or `teammate` (comma-separated, default `agent`).

```
ways template softwaredev/myteam/workflow -d "team deployment workflow and release process"
ways template itops/alerts -d "alerting runbooks" -V "alert pager oncall runbook" --global
```

---

### `ways lint`

**When:** After editing a way's frontmatter; before committing; in CI pipelines.

**Run from:** Project directory to scan all project ways. Pass a specific file path to lint just one file. Add `--global` to lint global ways instead.

**Tells you:** Validation errors and warnings per file against the frontmatter schema. `--schema` prints the full frontmatter schema reference. `--check` exits non-zero for CI use.

`--fix` auto-corrects what is fixable, and **takes its scope from `path`, not from the flag** — the same way `eslint --fix` does. Without a path it refuses rather than rewriting every way in the resolved corpus; pass `--all` when that is what you actually want. Each correction is disclosed on its own `FIXED:` line, and fixes are writes, so review them with `git diff`.

```
ways lint                                          # scan project ways
ways lint ~/.claude/hooks/ways/meta/knowledge/knowledge.md  # single file
ways lint <path> --fix                             # auto-correct within that path
ways lint --fix --all                              # auto-correct the whole resolved corpus
ways lint --check                                  # CI mode — non-zero exit on errors
ways lint --schema                                 # show the frontmatter schema
```

Exit codes: `0` clean, `1` errors found (with `--check`), `2` the invocation was wrong — kept distinct so a caller can tell "the corpus has problems" from "you used the flag wrong".

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

**Tells you:** Creates the `.claude/ways/` directory structure and seeds a `MEMORY.md` template for the project. It writes `.claude/.gitignore` (keeps developer-local files out of git) and `.claude/ways/_template.md` (a starting point for a project way that never fires) when they are missing. SessionStart runs it on `startup` and `clear`, so a fresh repo gets both files without running it by hand. Projects may commit or ignore them.

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
ways tune --lang es                   # audit one language (default: the active one)
ways tune --fidelity-threshold 0.7    # stricter fidelity requirement
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

**When:** After adding or editing ways — the corpus is what `match` queries. Also run when `ways status` shows the corpus as stale.

**Run from:** Anywhere. Use `--if-stale` to skip the rebuild if no way files have changed since the last build (safe to add to CI pre-flight).

**Tells you:** Progress output during the rebuild. Writes a `.jsonl` corpus file to the XDG cache directory.

```
ways corpus
ways corpus --if-stale              # skip if current
ways corpus --quiet                 # suppress progress output
ways corpus --verbose               # trace every phase; diagnose a stalled build
```

#### Diagnosing a build that appears to hang

A quiet corpus build is silent for long stretches: it walks every project's ways,
then shells out to `way-embed` three times, and the child's per-way progress is
discarded. A slow pass and a wedged pass look identical.

`--verbose` (which overrides `--quiet`) stamps each phase with elapsed time and
prints it *before* the work starts, so the last line on screen names the step that
stalled. It also streams `way-embed`'s own `[n/total] <way-id>` output, which
pinpoints a hang to a single way, and echoes each child's argv so you can rerun that
pass standalone:

```
ways corpus --verbose
```

Phases worth recognizing, in order: path resolution → user/core way scans →
per-project resolution (each project named as it is resolved — a stalled network
mount or a cloud-storage placeholder directory hangs here, under that project's
name) → corpus write → the three `way-embed generate` passes (`en`, `multi`, then
`combined`, which re-embeds everything the first two already did and so is the
longest) → the two calibration lanes → manifest write.

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

### `ways-audit report --json`

**When:** Auditing which ways have governance metadata (ADR links, control references, policy derivations). This lives in the sibling `ways-audit` binary (ADR-151), not `ways`; the raw manifest is `ways-audit report --json`.

**Run from:** Anywhere.

**Tells you:** The claim manifest — ways with provenance sidecar files and their metadata (ADR references, control IDs, and verified dates), as JSON.

```
ways-audit report --json
```

---

## Administration

### `ways reconcile`

**When:** Installing, updating, or repairing the `~/.claude` projection (ADR-144). The installer and `ways update` run it for you; run it by hand after pulling the app source or when a projected root is missing.

**Run from:** Anywhere.

**Tells you:** Which projection roots it linked or relinked, one line each, plus a one-line summary unless `--quiet`. Stops with a non-zero exit, before touching anything, when a projected root (`skills/`, `agents/`, `commands/`, `hooks/ways/`, `hooks/check-config-updates.sh`, `bin/*`) is already a real directory or file rather than a symlink; the message lists the paths. It never deletes a real path.

With no `--dest`, it runs over every target in the user config ([ADR-184](../architecture/platform/ADR-184-installation-and-activation-are-separate-states-targets-as-the-unit-of-activation.md)): each enabled target is converged, and each disabled one is withdrawn, meaning our symlinks are unlinked and our hooks block and permissions are removed from its `settings.json` through the same merge base that wrote them. With no `targets` key the one target is `~/.claude`, enabled. An explicit `--dest` is a single-target run and leaves the list alone.

```
ways reconcile                       # every target in config.yaml; default: ~/.claude
ways reconcile --dry-run             # preview; prints "refused <root>" for real paths, exit 0
ways reconcile --force               # rename each real path to <name>.ways-backup-<seconds>, then link
ways reconcile --source <checkout> --dest <dir>   # dogfood a development checkout
ways reconcile --mode copy           # copy files instead of symlinking (default: symlink)
ways reconcile --quiet               # suppress the summary line
```

---

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

**Tells you:** The resolved configuration as a table: language, scope, the project switch, disabled collections, matching thresholds, refire presets, and the targets. `--json` prints the stored user file as one document; `--json --effective` prints the resolved state with defaults applied ([ADR-185](../architecture/platform/ADR-185-cli-output-contract-structured-output-for-people-json-for-machines.md)). Only the stored form is meant to be written back.

```
ways config show
ways config show --json            # the stored file
ways config show --json --effective
ways config path                   # where the config file lives
ways config init                   # create config at XDG path if missing
```

---

### `ways config targets`

**When:** Finding out where agent-ways is active on this machine, or activating and deactivating it for a Claude Code config directory ([ADR-184](../architecture/platform/ADR-184-installation-and-activation-are-separate-states-targets-as-the-unit-of-activation.md)).

**Run from:** Anywhere.

**Tells you:** Each target with its enabled and observe flags, its converged state (`active`, `pending`, `partial`, `refused`, `stale`, or `withdrawn`), and its own config file when one exists. With no `targets` key in the user config the list is the implicit default, `~/.claude`.

Each target can carry its own configuration: a `config.yaml` at `$XDG_CONFIG_HOME/agent-ways/targets/<key>/config.yaml` (or the path in the entry's `config:` field) with the same keys as the user config, layered over it for sessions under that target's config directory. `ways config show` names the layer it applied.

`target plan` previews activation without touching anything: every projection root as `linked`, `link`, `relink`, or `refused`, and the settings merge as what is kept of yours, what is added, what of a prior install is replaced, and what would be removed. `target add` prints that plan and stops with exit code 3 when a real path sits at a root or an entry of yours would go; `--force` moves real paths aside and proceeds. `disable` withdraws and keeps the record; `remove` withdraws and drops it.

```
ways config targets
ways config target plan ~/.claude-work
ways config target add ~/.claude-work            # record, then reconcile into it
ways config target add ~/.claude-work --dry-run  # the plan only
ways config target disable ~/.claude-work        # withdraw our links and hooks
ways config target enable ~/.claude-work
ways config target remove ~/.claude-work
```

A project can switch ways off for itself with `enabled: false` in its `.claude/ways.yaml`; every scan lane then injects nothing there.

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

### `ways projects`

**When:** Finding a project's session history, seeing what `~/.claude/projects` holds, clearing out empty project entries, or moving a project's history after the project directory moved. It replaces the `claude-projects` script.

**Run from:** Anywhere. It reads Claude Code's `~/.claude/projects`, never your project directories.

**Tells you:** Depends on subcommand. `cleanup` and `hygiene` list what they would remove, ask first, and move it to a `.trash-<stamp>` dir under `~/.claude/projects` rather than deleting it; `--dry-run` only lists. `relocate` prints a plan and changes nothing unless given `--execute`. It refuses while a session is running in the project, and a rerun after a failed step resumes from where it stopped.

| Subcommand | What it shows or does |
|------------|-----------------------|
| `list` (default) | Projects, most recently active first; `--active`, `--memory`, `--stale` filter, `--urls` prints `file://` links |
| `search <query>` | Projects whose path, session summaries or first prompts match; `--deep` also searches transcript text |
| `show <fragment>` | One project: dates, branch, transcripts, memory, recent sessions |
| `stats` | Totals and the projects using the most disk and holding the most sessions |
| `cleanup` | Moves project entries with no sessions and no transcripts to a `.trash-<stamp>` dir; skips entries modified in the last 5 minutes |
| `hygiene` | Large transcripts, and empty session dirs, which it moves to a `.trash-<stamp>` dir |
| `relocate OLD NEW` | Moves the history of sessions started in OLD to NEW: the project dir, transcript `cwd`s, `sessions-index.json`, the `~/.claude.json` key and `history.jsonl`. `--merge` combines with an existing project, `--keep-transcript-cwd` leaves transcripts untouched, `--force` proceeds past a live-session warning |

```
ways projects
ways projects search orbit
ways projects cleanup --dry-run
ways projects relocate ~/old/repo ~/new/repo            # preview
ways projects relocate ~/old/repo ~/new/repo --execute
```

---

### `ways-audit`

**When:** Compliance reporting; finding ways that lack ADR traceability; identifying ways with stale verified-dates; cross-referencing governance controls with firing activity.

**Run from:** Anywhere. Add `--global` to restrict to global ways. This is the sibling `ways-audit` binary (ADR-151), not a `ways` subcommand.

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
| `control <id>` | Which ways *claim* a given control |
| `policy <id>` | Which ways derive from a given policy |
| `assemble [--way <id>] [--write]` | Build the classifier-ready **finding dataset** (ADR-201) — one row per `(way, control)` with firing evidence and an *empty* determination. `--write` appends to the finding ledger |
| `findings` | Show the assembled finding ledger |

```
ways-audit report
ways-audit gaps
ways-audit trace meta/knowledge
ways-audit matrix --json > coverage.jsonl
ways-audit assemble --json > findings.json   # dataset for an external classifier
```

`assemble` never writes a determination — it leaves the label empty for a separate,
out-of-scope classifier (ADR-201). Assembly is not assessment.

---

## Plumbing

These are used internally by hook scripts. You rarely need to call them directly, but they're useful when writing custom hooks or debugging the hook pipeline.

| Command | Used by |
|---------|---------|
| `ways scan command` | `PreToolUse` hook — fires ways based on bash commands Claude runs |
| `ways scan file` | `PreToolUse` hook — fires ways based on files Claude is editing |
| `ways scan state` | `UserPromptSubmit` + `SessionStart` — evaluates context-threshold, file-exists, and session-start triggers; `--query` carries the prompt so a harness envelope (Monitor notification, task hand-back, skill body) skips the scan |
| `ways scan task` | `SubagentStart` hook — injects ways into teammate/subagent sessions |
| `ways hook <event>` | Every script under `hooks/ways/` — reads the hook's JSON payload on stdin and prints what the hook returns. Events: `prompt`, `state`, `command`, `file`, `task`, `post-tool`, `queued`, `stop`, `subagent-start`, `session-start`, `tasks-active` |
| `ways sessions-root` | Scripts the binary does not run (`gh-tasks`); macros and postchecks get it as `WAYS_SESSIONS_ROOT` |
