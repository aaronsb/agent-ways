# Ways Functional Audit — 117 ways

Assessed all 117 frontmatter ways against the functional firing contract (sonnet fan-out, 24 batches).

**Verdicts:** keep 109 · revise 7 · investigate 1 · remove 0


## Action items (non-keep), verified against source


### [INVESTIGATE] `meta/attend/reflection-overdue/reflection-overdue.md`
- firing paths: `attend` · trigger: ok · alignment: na · yaml: complete · strayed: True
- **recommendation:** Correctly-shaped attend handler (ADR-114), but no sensor emits its `reflection-overdue` signal, so it is unreachable today. Verified follow-up to the agent's note: its *guidance* (capture decisions / what-didn't-work / state / save to memory) is already delivered by wired ways — `context-pressure` (attend, emitted by sensor-context at ≥85%) and `compaction-checkpoint` (`trigger: context-threshold, threshold: 85`), plus the on-demand `/wrap` skill. Its one **distinct** dimension is firing on *time since last reflection independent of context level* — the deferred "motivation sensor" of ADR-119 (steps 8–9) / ADR-123, explicitly out of scope there. So: do **not** build a bespoke sensor to chase it (it would duplicate sensor-context + compaction-checkpoint), and it is not a clean vestigial delete either (the ADRs mark it as an intended placeholder). Disposition — remove as redundant vs. keep as the motivation-sensor placeholder — belongs to the ADR-baseline consolidation, not a way-file edit.

### [REVISE] `meta/knowledge/authoring/authoring.md`
- firing paths: `file` · trigger: ok · alignment: na · yaml: missing_recommended · strayed: False
- **recommendation:** Add description:/vocabulary: for a semantic path — right now this way (itself titled 'Authoring Ways' and preaching 'Use semantic matching... this is the primary matching strategy') has NO semantic lane at all and only fires on file-path match, so a prompt like 'how do I add a keyword trigger to my way' before any file is touched will never surface it.
  - No `description:`/`vocabulary:` pair — the way that most emphatically argues for semantic matching ('Use semantic matching. This is the primary matching strategy for prompt-triggered ways') has no semantic firing path itself, only `files:`.

### [REVISE] `meta/knowledge/authoring/pii-free/pii-free.md`
- firing paths: `semantic` · trigger: misconfigured · alignment: aligned · yaml: complete · strayed: False
- **recommendation:** Fix the `files:` regex — as written it can never match any way file, including its own, so the file-trigger silently contributes nothing and only the semantic lane is actually live.
  - `files: \.claude/(hooks/)?ways/.*way\.md$` requires the path to end in the literal string `way.md`. No way file in this project (or anywhere findable on the filesystem) ends that way — not even this file itself (`pii-free.md`). Verified via `find / -name '*way.md'` returning nothing. Compare to the sibling `authoring.md`'s correct, working regex `(\.claude|agent-ways)/ways/.*\.md$` (ends in plain `.md$`) — this looks like a copy/typo bug that turned a broad file trigger into a dead one.

### [REVISE] `meta/knowledge/authoring/tool-agnostic/tool-agnostic.md`
- firing paths: `semantic` · trigger: misconfigured · alignment: aligned · yaml: complete · strayed: False
- **recommendation:** Same regex bug as its sibling pii-free.md — fix `files:` so the file trigger actually matches way files.
  - `files: \.claude/(hooks/)?ways/.*way\.md$` — identical bug to pii-free.md: requires the path to end in literal `way.md`, which never matches (not even `tool-agnostic.md` itself). Only the semantic lane is functionally live; the declared file trigger is dead.

### [REVISE] `meta/subagents/subagents.md`
- firing paths: `semantic, keyword` · trigger: ok · alignment: aligned · yaml: complete · strayed: False
- **recommendation:** Tighten the keyword pattern's `review.{0,30}\bpr\b` alternation — it fires on any 'review this PR' phrasing regardless of whether subagent delegation is actually in play, which is broader than the way's intent.
  - `pattern: subagent|delegat|spawn.{0,30}agent|review.{0,30}\bpr\b|organiz.{0,30}docs` — the `review.{0,30}\bpr\b` alternative matches generic 'please review this PR' requests that have nothing to do with spawning a sub-agent, risking an over-broad keyword floor hit distinct from the way's semantic alias (delegate/spawn/background/parallel/worker).

### [REVISE] `softwaredev/code/code.md`
- firing paths: `semantic` · trigger: ok · alignment: partial · yaml: complete · strayed: False
- **recommendation:** Bring the 'See Also' list in sync with the children table (add code/performance and code/supplychain, or drop them from the table if intentionally excluded).
  - The children table lists six ways: `code/quality`, `code/testing`, `code/security`, `code/performance`, `code/errors`, `code/supplychain`. The 'See Also' section below it only links four: "code/quality(softwaredev) — measurable quality thresholds / code/security(softwaredev) — secure coding defaults / code/testing(softwaredev) — test structure and coverage / code/errors(softwaredev) — error handling boundaries" — `code/performance` and `code/supplychain` are present in the table but silently missing from See Also, a documentation-drift inconsistency within the same file (not a functional firing bug).

### [REVISE] `softwaredev/environment/environment.md`
- firing paths: `semantic` · trigger: na · alignment: aligned · yaml: complete · strayed: False
- **recommendation:** Keep as the semantic index for the domain, but fix the stale child table/See Also list.
  - The directory contains a child way `environment/attend/attend.md` (attend binary / awareness sensor loop) that is not listed in the 'Children of this way' table (line 12-18) - the table is out of sync with the actual children on disk.
  - 'See Also' (lines 22-23) links only `environment/config` and `environment/deps`, omitting `environment/debugging`, `environment/ssh`, and `environment/makefile`, all of which are listed one section above in the table - internally inconsistent.

### [REVISE] `softwaredev/environment/makefile/makefile.md`
- firing paths: `semantic, file, bash` · trigger: misconfigured · alignment: aligned · yaml: complete · strayed: False
- **recommendation:** Anchor the `commands` regex to avoid substring collisions with unrelated tools.
  - `commands: make` (line 5) is matched via unanchored regex substring search against the full raw command (confirmed in tools/ways-cli/src/cmd/scan/mod.rs via `regex_span(p, cmd)`), so it fires on any command containing the literal substring "make" anywhere - e.g. `cmake --build .`, `makepkg -si`, `remake`, `npm run makebundle` - none of which are GNU Make invocations. Sibling way `ssh/ssh.md` shows the correct pattern (`^ssh\ |^scp\ |...` with anchors/word boundaries); this field should similarly be tightened, e.g. `(^|/| )make(\s|$)` or `\bmake\b` combined with a start-of-command anchor.

## Full verdict table

| Way | Verdict | Firing paths | Trigger | Align | YAML |
|-----|---------|--------------|---------|-------|------|
| `collaboration/onboarding-share/onboarding-share.md` | keep | semantic, keyword | ok | aligned | complete |
| `collaboration/teams/teams.md` | keep | state | ok | na | complete |
| `data/data.md` | keep | semantic, keyword | ok | aligned | complete |
| `data/documentation/documentation.md` | keep | semantic, keyword | ok | aligned | complete |
| `data/migrations/checkpoint/checkpoint.md` | keep | semantic, keyword | ok | aligned | complete |
| `data/migrations/idempotent/idempotent.md` | keep | semantic, keyword | ok | aligned | complete |
| `data/migrations/migrations.md` | keep | semantic, keyword | ok | aligned | complete |
| `data/migrations/numbering/numbering.md` | keep | semantic, keyword | ok | aligned | complete |
| `documentation/adr-context/adr-context.md` | keep | semantic | ok | aligned | complete |
| `documentation/adr/adr.md` | keep | semantic, keyword, file | ok | aligned | complete |
| `documentation/adr/migration/migration.md` | keep | semantic | ok | aligned | complete |
| `documentation/api/api.md` | keep | semantic | ok | aligned | complete |
| `documentation/diataxis/diataxis.md` | keep | semantic, keyword | ok | aligned | complete |
| `documentation/docstrings/docstrings.md` | keep | semantic | ok | aligned | complete |
| `documentation/documentation.md` | keep | semantic, keyword, file | ok | aligned | complete |
| `documentation/mermaid/mermaid.md` | keep | semantic | ok | aligned | complete |
| `documentation/readme/readme.md` | keep | semantic | ok | aligned | complete |
| `documentation/standards/standards.md` | keep | semantic | ok | aligned | complete |
| `ea/briefing/briefing.md` | keep | semantic | ok | aligned | complete |
| `ea/calendar/calendar.md` | keep | semantic | ok | aligned | complete |
| `ea/comms/comms.md` | keep | semantic | ok | aligned | complete |
| `ea/comms/recap/recap.md` | keep | semantic | ok | aligned | complete |
| `ea/ea.md` | keep | semantic | ok | aligned | complete |
| `ea/email/drafting/drafting.md` | keep | semantic | ok | aligned | complete |
| `ea/email/email.md` | keep | semantic | ok | aligned | complete |
| `ea/intelligence/intelligence.md` | keep | semantic | ok | aligned | complete |
| `ea/tasks/tasks.md` | keep | semantic | ok | aligned | complete |
| `ea/tasks/time/time.md` | keep | semantic | ok | aligned | complete |
| `itops/incident/incident.md` | keep | semantic, keyword | ok | aligned | complete |
| `itops/policy/policy.md` | keep | semantic, keyword | ok | aligned | complete |
| `itops/proposals/proposals.md` | keep | semantic, keyword | ok | aligned | complete |
| `itops/runbooks/runbooks.md` | keep | semantic, keyword | ok | aligned | complete |
| `meta/attend/build-complete/build-complete.md` | keep | attend | ok | na | complete |
| `meta/attend/context-pressure/context-pressure.md` | keep | attend | ok | na | complete |
| `meta/attend/reflection-overdue/reflection-overdue.md` | investigate | attend | ok | na | complete |
| `meta/choices/choices.md` | keep | semantic, keyword | ok | aligned | complete |
| `meta/compaction-checkpoint/compaction-checkpoint.md` | keep | state | ok | na | complete |
| `meta/deployment/deployment.md` | keep | semantic, keyword | ok | aligned | complete |
| `meta/goals/goals.md` | keep | semantic | ok | aligned | complete |
| `meta/governance/governance.md` | keep | semantic | ok | aligned | complete |
| `meta/introspection/introspection.md` | keep | semantic, keyword, bash | ok | aligned | complete |
| `meta/knowledge/authoring/authoring.md` | revise | file | ok | na | missing_recommended |
| `meta/knowledge/authoring/pii-free/pii-free.md` | revise | semantic | misconfigured | aligned | complete |
| `meta/knowledge/authoring/tool-agnostic/tool-agnostic.md` | revise | semantic | misconfigured | aligned | complete |
| `meta/knowledge/knowledge.md` | keep | semantic, keyword | ok | aligned | complete |
| `meta/knowledge/optimization/optimization.md` | keep | semantic | na | aligned | complete |
| `meta/knowledge/optimization/tuning/tuning.md` | keep | semantic | na | aligned | complete |
| `meta/memory/memory.md` | keep | semantic, keyword, file, state | ok | aligned | complete |
| `meta/skills/skills.md` | keep | semantic, keyword | ok | aligned | complete |
| `meta/subagents/subagents.md` | revise | semantic, keyword | ok | aligned | complete |
| `meta/think/think.md` | keep | semantic, keyword | ok | aligned | complete |
| `meta/todos/todos.md` | keep | state | ok | na | complete |
| `meta/tracking/tracking.md` | keep | semantic, keyword, file | ok | aligned | complete |
| `meta/trust/autonomy/autonomy.md` | keep | semantic | na | aligned | complete |
| `meta/trust/delegation/delegation.md` | keep | semantic | na | aligned | complete |
| `meta/trust/trust.md` | keep | semantic | ok | aligned | complete |
| `meta/trust/voice/voice.md` | keep | semantic | ok | aligned | complete |
| `meta/workflows/workflows.md` | keep | semantic, keyword | ok | aligned | complete |
| `meta/wrap/wrap.md` | keep | semantic, keyword | ok | aligned | complete |
| `research/research.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/architecture/architecture.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/architecture/design/design.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/architecture/design/prototype/prototype.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/architecture/threat-modeling/threat-modeling.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/code.md` | revise | semantic | ok | partial | complete |
| `softwaredev/code/errors/errors.md` | keep | semantic, keyword | ok | aligned | complete |
| `softwaredev/code/overbuild/overbuild.md` | keep | postcheck | ok | na | missing_recommended |
| `softwaredev/code/performance/performance.md` | keep | semantic, keyword | ok | aligned | complete |
| `softwaredev/code/quality/quality.md` | keep | semantic, keyword, postcheck | ok | aligned | complete |
| `softwaredev/code/quality/versioning/versioning.md` | keep | semantic, postcheck | ok | aligned | complete |
| `softwaredev/code/security/auth/auth.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/security/contributions/contributions.md` | keep | semantic, keyword | ok | aligned | complete |
| `softwaredev/code/security/injection/injection.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/security/secrets/secrets.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/security/security.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/automation/automation.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/depscan/depscan.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/depscan/go/go.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/depscan/node/lockfile/lockfile.md` | keep | semantic, file | ok | aligned | complete |
| `softwaredev/code/supplychain/depscan/node/node.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/depscan/python/python.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/depscan/rust/rust.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/historysever/historysever.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/repoaudit/repoaudit.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/sourceaudit/sourceaudit.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/supplychain/supplychain.md` | keep | semantic, bash | ok | aligned | complete |
| `softwaredev/code/testing/mocking/mocking.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/testing/tdd/tdd.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/code/testing/testing.md` | keep | semantic, bash | ok | aligned | complete |
| `softwaredev/delivery/branching/branching.md` | keep | semantic, file | ok | aligned | complete |
| `softwaredev/delivery/commits/commits.md` | keep | semantic, keyword, bash | ok | aligned | complete |
| `softwaredev/delivery/delivery.md` | keep | semantic | na | aligned | complete |
| `softwaredev/delivery/github/github.md` | keep | semantic, keyword, bash | ok | aligned | complete |
| `softwaredev/delivery/implement/implement.md` | keep | semantic | na | aligned | complete |
| `softwaredev/delivery/patches/patches.md` | keep | semantic, keyword, file, bash | ok | aligned | complete |
| `softwaredev/delivery/release/release.md` | keep | semantic, keyword | ok | aligned | complete |
| `softwaredev/environment/attend/attend.md` | keep | semantic, keyword, bash | ok | aligned | complete |
| `softwaredev/environment/config/config.md` | keep | semantic, file | ok | aligned | complete |
| `softwaredev/environment/debugging/debugging.md` | keep | semantic | ok | aligned | complete |
| `softwaredev/environment/deps/deps.md` | keep | semantic, keyword, bash | ok | aligned | complete |
| `softwaredev/environment/environment.md` | revise | semantic | na | aligned | complete |
| `softwaredev/environment/makefile/makefile.md` | revise | semantic, file, bash | misconfigured | aligned | complete |
| `softwaredev/environment/ssh/ssh.md` | keep | semantic, keyword, bash | ok | aligned | complete |
| `softwaredev/freshness/freshness.md` | keep | semantic, state | ok | aligned | complete |
| `softwaredev/freshness/groundtruth/groundtruth.md` | keep | semantic, keyword | ok | aligned | complete |
| `softwaredev/tooling/tooling.md` | keep | semantic, keyword, file | ok | aligned | complete |
| `softwaredev/visualization/charts/charts.md` | keep | semantic, keyword | ok | aligned | complete |
| `softwaredev/visualization/diagrams/diagrams.md` | keep | semantic, keyword | ok | aligned | complete |
| `softwaredev/visualization/visualization.md` | keep | semantic, keyword | ok | aligned | complete |
| `workstation/pkghistory/pkghistory.md` | keep | semantic, keyword | ok | aligned | complete |
| `workstation/shell/gitconfig/gitconfig.md` | keep | semantic, file, bash | ok | aligned | complete |
| `workstation/shell/prompt/prompt.md` | keep | semantic, bash, file | ok | aligned | complete |
| `workstation/shell/shell.md` | keep | semantic | ok | aligned | complete |
| `workstation/shell/shellrc/shellrc.md` | keep | semantic, file | ok | aligned | complete |
| `workstation/shell/sshagent/sshagent.md` | keep | semantic, file, bash | ok | aligned | complete |
| `workstation/shell/tools/tools.md` | keep | semantic | ok | aligned | complete |
| `writing/writing.md` | keep | semantic | ok | aligned | complete |
