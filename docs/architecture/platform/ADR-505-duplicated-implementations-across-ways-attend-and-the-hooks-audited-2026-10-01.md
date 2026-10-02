---
contract: adr/v1
kind: evidence
capability: cli
status: proposed
date: 2026-10-01
deciders:
  - aaronsb
related: [ADR-503, ADR-504, ADR-143, ADR-177, ADR-179, ADR-181]
---

# ADR-505: Duplicated implementations across ways, attend and the hooks, audited 2026-10-01

## Summary

Three read-only audits on 2026-10-01 at `main` `edee7f30` found the same jobs implemented many times across the ways crates, the attend crates and the shell hooks, with copies that disagree. Seven live defects traced to those disagreements are fixed in PR #689 (open); three known ones remain. This record lists the copies by job, the defects, the legacy and dead code the audits found, the crate-merge candidates and the Python scripts. Where an audit named a canonical owner for a job, it is marked *proposed*; nothing here is decided.

## Method

Three sweeps, one per area, each by a subagent reading source without running or changing it:

| Sweep | Scope | Limits it named |
|---|---|---|
| Ways crates | `tools/ways-*` (ways-core, ways-cli, ways-audit, ways-mcp, ways-agent) | none stated; line counts of copies are approximate (`~`) |
| Attend crates | `tools/attend*`, `tools/sensor-*`, `tools/agent-fmt`, `tools/agent-identity` | none stated |
| Outside `tools/*/src` | `hooks/`, `scripts/`, `Makefile`, `install.sh`, `.github/workflows/`, download scripts, skills, commands | `tools/*/src` excluded; gh-tasks, several macros, gh-monitor, attend examples and `agents/` sampled or skipped; `github/macro.sh:300-408` against `settings_merge.rs` not compared; workflows compared by diff size only |

Paths below are relative to `tools/` for crate sources (crate named where the file name is ambiguous) and to the repository root otherwise. Line numbers are at `edee7f30`.

## Findings by job

### Project slug and transcript location

| Copies | Difference | Defect |
|---|---|---|
| Slug rule: ways `session.rs:259,312`, `context.rs:194`, `rethink/frames.rs:198` and `hooks/ways/meta/memory/macro.sh:18` map `/` and `.`; attend `util.rs:43`, `sensor-peers:795` map `/ _ . \ :`; `attend-chat signal.rs:58`, `attend-groups:534`, `attend-instances:421` map `/ _ .`; `claude-projects:677` maps every non-alphanumeric | Claude Code maps every character outside `[A-Za-z0-9]` to `-`; only `claude-projects` matches. No slug function existed; `paths::transcripts_root` (`paths.rs:278`) is used only by `transcript.rs` | Ways: a path with `_` lost its transcript (token position 0 driving refire, context-window fallback, empty introspect timeline, wrong `project_memory_dir`). Attend: wrong for spaces and symbols |
| Transcript location ~8 copies in ways: `ways-core transcript.rs:33`, `context.rs:166,455,462`, `session.rs:256,310`, `rethink/frames.rs:197`, `memory_seed.rs:86`, `corpus.rs:179`, `list.rs`; attend `tune.rs:7`, `sensor-peers:523,609` | `get_token_position` takes the newest transcript in the project, ignoring session id (callers `scan/mod.rs:1079`, `show/mod.rs:401,435`, `list.rs:76`) | Concurrent sessions read each other's token position |
| Usage token sum 3x (`session.rs:270`, `context.rs:315`, `frames.rs:213`) plus `sensor-peers:550-575` | `sensor-peers` sums `cache_read` only | |
| Model detection 2x (`session.rs:350`, `context.rs:280`); newest-jsonl finder 2x identical (`session.rs:814`, `context.rs:486`) | | |
| `sensor-context lib.rs:42`, `sensor-keepwarm lib.rs:239` | Each runs `ways context --json` every poll and parses it differently | |

### Session-record reading

| Copies | Difference | Defect |
|---|---|---|
| `~/.claude/sessions/*.json`: `attend-session:152` `find_session_in`, `:269,277`; `sensor-peers:806,868`; `attend-chat sessions.rs:36-86`; sessions dir built 3x; `sensor-peers:891` pass-through wrapper | `sensor-peers` copy is not whitespace-tolerant. Proposed owner: `attend-session` | |
| Hand JSON extractors `sensor-trait:421`, `tune.rs:278` | `serde_json` is already a dependency | |
| Agent-ways sessions root: `hooks/ways/sessions-root.sh` copies `session.rs:34` ("MUST stay identical"), sourced by 8 consumers; `ways sessions-root` (`main.rs:349`) used only by `check-portability.sh:98` | | |
| `clear-markers.sh:21-22` overlaps `ways reset`; writes `session_start` with jq (`:30-35`), which only Rust reads | With no session id, `:24-25` ran `rm -rf "$SESSIONS_ROOT"` | Every live session's state wiped |

### Frontmatter splitting

| Copies | Difference | Defect |
|---|---|---|
| Private `frontmatter.rs:206` `extract_frontmatter_str`; byte-identical `candidates.rs:304`, `permissions.rs:101`, `lint/helpers.rs:10` | Proposed owner: `frontmatter.rs`, made public | |
| Field scanners `show/helpers.rs:7,120`, `tree.rs:233`, `scanner.rs:51`; body strippers `show/helpers.rs:31`, `tree.rs:252`, `rethink/drilldown.rs:70`, `late_interaction.rs:381` | Way detection differs (`scanner.rs` requires `description:`) | `introspection.rs:481` fails on CRLF; `body_text` and `check_sections_text` dropped `---` rules in way bodies (two shipped ways lost lines). Six more readers still count every `---` as a fence |
| Shell awk: `hooks/ways/macro.sh:35-105`, `meta/knowledge/optimization/macro.sh:15-29,45-53`, `inject-subagent.sh:91-110` | | |

### Way-file walking

| Copies | Difference | Defect |
|---|---|---|
| ~12 near-identical WalkDir loops: `scanner.rs:17`, `candidates.rs:129,183`, `introspection.rs:507`, `provenance.rs:184`, `status.rs:285`, `permissions.rs:61`, `lint/scanning.rs:23`, `tree.rs:166,200`, `corpus.rs:625,1053,1079,1098`, `tune.rs:187`, `language.rs:153` | | |
| `inject-subagent.sh:60-152` re-implements `ways show way`: root resolution `:91-101`, awk macro `:110`, trust `:114-123`, body strip `:132`, event logging `:144-152` (duplicates `session::log_event`, `session.rs:528`) | Uses `command -v ways` (`:67`) unlike other hooks; omits the XDG user ways root (ADR-143) that `check-post.sh:38` includes | Subagents never received user ways. The SubagentStart hook takes about 3.4 s |
| `hooks/ways/macro.sh:35-105` core table | Global root only, via `ways show core` (`show/mod.rs:784-790`) | |

### Core ways root and way-embed location

| Copies | Difference | Defect |
|---|---|---|
| `paths::core_ways_root` (`introspection.rs:555`) vs `~/.claude/hooks/ways` hard-coded at 13 sites: `candidates.rs:77` (fire path), `status.rs:11`, `language.rs:18`, `lint/mod.rs:73`, `tune.rs:78`, `permissions.rs:12`, `show/mod.rs:781,831`, `show/metrics.rs:49`, `session.rs:653,673`, `disable.rs:117`, `provenance.rs:125`; also `graph.rs:61`, `corpus.rs:35` | Proposed owner: `paths::core_ways_root` | |
| way-embed finder 4x: `scoring.rs:240`, `tune.rs:418`, `status.rs:269`, `corpus.rs:805` | `paths::bin_root` (`paths.rs:154`) unused. Model file names are literals at ~15 sites | |
| way-embed match invocation and output parsing: `scoring.rs:194`, `tune.rs:239`, `reduce.rs:643` (test), `late_interaction.rs:269,357` | | |

### Date and duration maths

| Copies | Difference | Defect |
|---|---|---|
| Civil date: `ways-core util.rs:227` `days_to_ymd`, `stats.rs:487`, `memory_seed.rs:215`, `attend tune.rs:334`, `agent-fmt when.rs:50` | Proposed owner: `agent-fmt::when` | |
| ISO parse: `util.rs:53` `parse_ts_secs`, `tune.rs:302`, `context.rs:422`, `rethink/layout.rs:301`; `session.rs:605` equals `util::now_utc` (`util.rs:211`); `stats.rs:478` `format_ts` | | |
| Duration/ago: `rethink/layout.rs:316`, `rethink/sessions.rs:259`, `sensor-peers:990`, `sensor-keepwarm:88`, `agent-fmt when.rs:17` | | |
| Epoch arithmetic inlined ~20x beside `sensor_trait::epoch_secs` (`lib.rs:259`); `attend-instances:436` `now_secs` | | |

### events.jsonl readers

| Copies | Difference | Defect |
|---|---|---|
| `firing::load_events` (`firing.rs:19,39`); `stats.rs:19`, `list.rs:323`, `tune_precision.rs:124`; `hooks/ways/events-log.sh:23` | `load_events` unions the legacy log; the three others read only the current one. Proposed owner: `firing::load_events` | Stats, list and precision tuning omit legacy-log events |

### Atomic writes and locks

| Copies | Difference | Defect |
|---|---|---|
| tmp+rename: `settings_merge.rs:694`, `ways-core config.rs:408`, `session.rs:589`, `keys.rs:145,293`, `profile.rs:248`, `corpus.rs:486`, `sensor-keepwarm:178`, `send.rs:203`, `attend-chat signal.rs:173`, `attend-groups:474`, `attend-instances:560` | `profile.rs:248` used a fixed tmp name; `attend-groups:474` is last-writer-wins | Concurrent `agent.yaml` writers collided on the tmp file |
| Locks, 5 styles: create_new `attend-state:335` (steal by rename), `ways-core config.rs:84,87` `TargetsLock` (steal by remove, racy); flock `attend-instances:444`, `attend-heartbeat:167`, `ways-agent server.rs:50`; `engagement.rs:153` `File::lock` | `config.rs:334` `Config::write_user_targets` bypassed `TargetsLock` (no callers; removed in PR #689) | |

### XDG and HOME resolution

| Copies | Difference | Defect |
|---|---|---|
| `paths::xdg_base` (`paths.rs:49`) vs `config.rs:684`, `util.rs:6`; `siblings.rs:168` unused `_xdg` | Only `xdg_base` rejects empty and relative values. Proposed owner: `paths::xdg_base` | |
| `.cache/attend` hard-coded at `attend util.rs:22`, `attend-heartbeat:38`, `attend-instances:104`, `attend-state:160,314`, `sensor-peers last_inbound.rs:40`, `inbox.rs:697` | None honour `XDG_CACHE_HOME` | |
| Private `home_dir` 4x (`attend-heartbeat:199`, `sensor-peers:775`, `attend-instances:430`, `agent-fmt permissions.rs:128`); ~20 raw `HOME` reads | | |
| `CLAUDE_PROJECT_DIR` read at 23 sites; helpers `scoring.rs:275`, `disable.rs:78`, `config_cmd.rs:19`, `config.rs:17`; `ways-audit helpers.rs:70` copies `util::detect_project_dir` (`util.rs:131`); 9 macros default the project dir, 3 variants | Fallbacks differ | |
| Attend config paths: user 5x (`scenes.rs:69`, `config_lint.rs:109`, `config.rs:347`, `tune.rs:212`, `main.rs:185`), project 3x (`config.rs:218`, `config_lint.rs:118`, `main.rs:189`); `settings.json` at `permissions.rs:18` beside `ways-core::paths::settings_json` | `HOME`-unset fallbacks differ | |

### Colour, width and truncation

| Copies | Difference | Defect |
|---|---|---|
| `visible_len`/`truncate_visible`/pad: `compositor.rs:27-90` vs `agent-fmt table.rs:220-287`; `attend-chat chip/render.rs:124`; `match_cmd.rs:174`; `reflow.rs:257`; `peers.rs:114` duplicates `attend-identity-view:110` | Escape-sequence end detection differs. Proposed owner: `agent-fmt` | `render.rs:557` truncate sliced bytes and panicked on multibyte ids (caller `render.rs:265`) |
| ~180 raw `\x1b[` in ways-cli and ways-audit | No `NO_COLOR` support | |

### Signal wire format and receive dirs

| Copies | Difference | Defect |
|---|---|---|
| Write 2x "keep in lockstep": `send.rs:186-221`, `attend-chat signal.rs:159-178` | | |
| Parse 3x: `inbox.rs:48`, `sensor-peers:1097`, `attend-chat signal.rs:78`; `is_valid_signal_id` 2x (`inbox.rs:36`, `sensor-peers:1079`) | `sensor-peers` drops `re`; attend-chat used `splitn(5)` without the id fence. Proposed owner: a signal module in `agent-identity` | attend-chat showed a message starting `re:` with a `\|` as a reply. The sensor-peers copy remains |
| `signals_base` 3x: `attend util.rs:12`, `attend-chat signal.rs:45`, `sensor-peers lib.rs:788` | | |
| `identify_sender` + `detect_terminal` 2x: `send.rs:331-385`, `signal.rs:278-314`; `USER`/`LOGNAME` 4x | Only `send.rs` falls back to `TERMINAL` | |
| Project label `send.rs:172` | `rsplit` diverges from `agent_identity::cwd_basename` (`identity.rs:77`) | Wrong label (audit: "buggy") |
| Receive-dir set 6 copies: `attend-groups:325` `receive_dirs` (no callers), `inbox.rs:75,139,525`, `sensor-peers:257-264`, `attend-chat watcher.rs:166`; `"_broadcast"` literal ~15 places | `watcher.rs` scans every `@*` dir, joined or not | |
| Live-member count: `send.rs:112-121` (running claude or fresh heartbeat), `attend-chat groups.rs:199-217` (heartbeat only), `attend-groups::member_alive:529` | Three predicates | |

### YAML subset parsers

| Copies | Difference | Defect |
|---|---|---|
| Six hand-rolled: `attend config.rs:397-668`, `scenes.rs:86`, `attend-groups:546,620`, `attend-instances:468,542`, `config_lint.rs:389-470`, `tune.rs:236` | `config_lint` `remove_top_level_field` resembles `ways-cli lint/helpers.rs:162` | |

### Process inspection

| Copies | Difference | Defect |
|---|---|---|
| pid to argv: `sensor-peers:823,839`, `ways-mcp session.rs:35`; `status.rs:7` runs `ps`; `attend-session:232,246` | | |

### Shell hook preambles and the postcheck dispatcher

| Copies | Difference | Defect |
|---|---|---|
| Stdin preamble with 3-10 jq calls in 12 hooks; session/agent/cwd/transcript block near-identical in `check-bash-pre.sh:18-28`, `check-file-pre.sh:10-21`, `check-prompt.sh:20-41`, `check-queued.sh:20-31`, `check-state.sh:10-26`, `check-task-pre.sh:18-24`, `check-post.sh:18-26` | Runs on every prompt or tool call. No `ways` mode reads hook JSON from stdin; proposed owner: `ways hook <event>` | |
| Deploy-skew retry guard 5x: `check-prompt.sh:49-75`, `check-state.sh:32-40`, `check-file-pre.sh:26`, `check-bash-pre.sh:33`, `check-post.sh:78` | Transitional | |
| Postcheck dispatcher `check-post.sh:33-93` | Runs `find` over every way root per PostToolUse with its own root precedence; 5 postchecks re-parse stdin (reflow `:27`, density `:29`, overbuild `:19`, quality `:15,22`, versioning `:20,27`). No post-tool scan mode in the binary | |
| Stop/prompt round trip: `check-response.sh:33` parses the transcript with tail/grep/jq and writes state `:46-50`; `check-prompt.sh:35-38` reads it back as `--response-context` | `transcript.rs` and `response-topics-path` (`main.rs:342`) exist in the binary | |

### Macro boilerplate

| Copies | Difference | Defect |
|---|---|---|
| About 14 of 20 `macro.sh` files share boilerplate: context-budget block (introspection `:6-20`, memory `:7-15`, todos `:6-13`, start `:5-8`); plugin probe spawning the Node CLI (`macro.sh:17`, research `:9`, writing `:10`); ADR/doc tool discovery (`adr/macro.sh:183`, `adr-context/macro.sh:11`, `documentation/macro.sh:23`); pending-file claim/consume, ~45 lines shared by reflow and density macros and postchecks; git-repo guard (quality `:6`, branching `:7`, freshness `:23`, adr `:171`) | | |

### Build plumbing

| Copies | Difference | Defect |
|---|---|---|
| Six `tools/*/download-*.sh`, 171 lines each; `way-embed/download-binary.sh` a near-copy | ~40-line diffs; `tools/scripts/prebuilt-lib.sh` exists | |
| `Makefile:223-420` six get-or-download-or-cargo blocks and six `-rebuild` blocks | | |
| Suite binary list at `Makefile:31`, `Makefile:188`, `install.sh:217` | Lists disagree; `install.sh:224` links `claude-projects`, `make link` does not | |
| Seven `.github/workflows/build-*.yml`, 136-149 lines | No `workflow_call` | |

### Other

- GitHub owner/repo parsing: `check-config-updates.sh:147,177`, `install.sh:185`, `statusline.sh:83-84`.
- Repo-health checks: `github/macro.sh:21-88` vs `commands/project-audit.md:28,172`.
- `corpus.rs:1046` `content_hash` uses `DefaultHasher`, which is not stable across Rust versions; `agent-identity` uses `fnv1a_64`.

## Defects caused by the duplication

| Defect | Status |
|---|---|
| Ways slug rule omitted `_` (and every other non-alphanumeric) | Fixed in PR #689 (open): `ways_core::paths::project_slug` |
| `get_token_position` read another session's transcript | Fixed in PR #689 |
| `clear-markers.sh` ran `rm -rf` on the sessions root with no session id | Fixed in PR #689 |
| Byte-slice truncation panicked on multibyte ids | Fixed in PR #689: agent-fmt `truncate_visible`, now public |
| Subagent injection skipped the user ways root | Fixed in PR #689: `ways show way <id> --subagent` |
| attend-chat misread `re:` messages | Fixed in PR #689: parser in `agent_identity::signal` |
| `agent.yaml` temp-name collision; frontmatter CRLF and body `---` rules | Fixed in PR #689: per-writer temp name; `ways_core::frontmatter::split` |
| Attend's five slug copies wrong for spaces and symbols | Open |
| sensor-peers' signal-parser copy | Open |
| Six more readers count every `---` as a fence | Open |
| SubagentStart hook takes about 3.4 s on `main` and on the PR #689 branch | Open |

## Legacy past its migration window, and dead code

**Ways crates.** Dead: `util.rs:6` `xdg_cache_dir`, `paths.rs:159` `frontmatter_schema`, `reflow.rs:512` `is_wrapped`, `agents/mod.rs:122` `resolve_to_lang_code`, `session.rs:158,178` `way_fired_scope(s)`, `config.rs:334` `Config::write_user_targets` (removed in PR #689), `reduce.rs:608-640` ignored test with claude-ways paths, three `not(tui)` stubs never compiled (the `tui` feature is always on). Legacy: `ways embed` alias ignoring `--model`; `ways rethink`; `match --cosine`; config layers `ways.json` and `$XDG/ways/config.yaml`; ADR-179 fallbacks (a policy call); `scoring.rs:122` combined-corpus fallback. Kept on purpose: `tests/project_ways.rs:38` contract copy.

**Attend crates.** Dead: `attend-groups` `receive_dirs`, `legend.rs:149` `best_group_completion`, agent-fmt `print_commands`, `DeltaAccumulator::summary`, `siblings.rs:169` `_xdg`; test-only pubs `legend.rs:103`, `groups.rs:121`; docs naming a nonexistent `session_alive` (`run/mod.rs:273`, `attend-heartbeat:8`, `sensor-peers:237`). Legacy: `--focus` alias (`cli.rs:109,132`), `attend focus` subtree (`cli.rs:174,264`, `main.rs:153-169`), field `focus` (`main.rs:75`); rooms vs channels (`scenes.rs:22-65`, `attend-groups:330`); `send --broadcast` no-op (`send.rs:168`) and `cmd_reply(_broadcast)` (`:317`); one-shot `@open` migration on every run (`groups.rs:45`, `run/mod.rs:142`); empty `ENGAGEMENT_DEPRECATED`; `burst_window` rejection (`config.rs:363-390`); legacy `signal_salience` rows (`sensor-peers:761`); attend's six optional sensor features, never built off.

**Outside `tools/*/src`.** `check-config-updates.sh` scenarios 0 and 1-5 (~170 of 301 lines); sync-to-home (`scripts/sync-to-home.sh` 201 lines, its test 123, `Makefile:204-211`, `commands/sync-to-home.md`, `show/metrics.rs:193-196`, `reconcile.rs:93` comment); Makefile `install`/`hooks-install`/`INSTALL_HOOKS` (`:16,22,162-185`), `make update` (`:195-198`; `scripts/update.sh` and `make relink` are used by `update.rs`), `make uninstall` overlap, `release:` (`:508-515`); stale `make install` advice (`optimization/macro.sh:6`, `attend/macro.sh:7`, `skills/attend/SKILL.md:34`); `check-setup.sh:15-18` legacy cache and `:44-80` duplicating `ways status`; pre-ADR-155 `.topics` in `events-log.sh:23`, `signal-report.py:76`, `check-prompt.sh:38`; `statusline.sh:41-47` old `attend whoami` parse; the five deploy-skew guards; `skills/ways-update/SKILL.md:16-60` hand-rolling `ways update`; stale `ways.json` mentions (`bug_report.md:36`, ways-localize `SKILL.md:55`, `knowledge/provenance.yaml:10`, `inject-subagent.sh:70`); unreferenced `attend-demo.sh`, `attend-format-probe.sh`, `hook-fire-detector.sh`, `audit-permissions.sh`, `tools/scripts/docs-enact-*.mjs`, `*-fanout.mjs`; `install.sh` migration messaging tied to the ways-v1.8.3 migrator (a judgment call).

## Crate-merge candidates

| Candidate | Into |
|---|---|
| `attend-identity-view` (181 lines) | `attend-instances` or `agent-identity` |
| `attend-heartbeat`, `attend-session`, possibly `attend-state` | One presence crate |
| `sensor-git`, `sensor-context`, `sensor-disclosure` | Modules (their features are always on) |
| `sensor-peers` `signal_id.rs` | Inline |

## Python inventory

Tracked in issue #688.

| Script | Lines | Notes |
|---|---|---|
| `claude-projects` | 1422 | Successor work under `ways projects` |
| `probe-measure.py` | 178 | Stale single-vector cosine; hard-codes `~/.cache`; owner `ways match`/`embed` |
| `fire-panel.py` | 171 | Same as `probe-measure.py` |
| `signal-report.py` | 248 | matplotlib |
| `test-locales.py` | 45 | Run by `Makefile:492`; owner unverified |
| `check-bash-bound.py` | 235 | Guard hook, not wired (ADR-181) |
| adr-tool, `doc-tool/doclint.py` | | Vendored by policy (ADR-177); out of scope |
| chart-tool, `docs/research` scripts | | Out of scope |
