---
status: Proposed
date: 2026-07-18
deciders:
  - aaronsb
  - claude
related:
  - "[[ADR-142]]"
  - "[[ADR-147]]"
  - "[[ADR-149]]"
  - "[[ADR-152]]"
  - "[[ADR-163]]"
---

# ADR-169: agent-ways relinquishes user-scoped settings.json; retains only its operational baseline

## Context

`~/.claude/settings.json` is Claude Code's declarative user configuration file. It
is also a *shared-write object*: several independent parties mutate it, and none of
them coordinates with the others.

- **Claude Code owns it.** `/config` persists user preferences (theme, model,
  `autoCompactEnabled`, tui/notification toggles) straight into
  `~/.claude/settings.json` at user scope, and the write target cannot be
  redirected. Claude Code additionally writes some keys *in situ* during a session
  (`advisorModel`, `effortLevel` — the "known in-situ writers" of ADR-147).
- **agent-ways writes two slices** via `ways reconcile`
  (`tools/ways-cli/src/cmd/settings_merge.rs`): the hook entries it ships, and its
  permission strings (`WAYS_PERMS` in `permissions.allow`, `WAYS_DENY` in
  `permissions.deny`, the ADR-152 secret-path baseline). This is a three-way merge
  keyed on a persisted last-applied base — the "one shared-write seam" of ADR-142.
- **agent-ways also ships a config-management service** on top of that seam: the
  composable fragment store (ADR-147) and the operator interview skill (ADR-149),
  exposed as `ways settings` and the `ways-settings` skill. Its projector
  (`settings/project.rs`) is a *second* independent three-way writer that owns the
  fragment keys (`model`, `env`, `statusLine`, …) and deliberately skips
  hooks/permissions.

Three problems motivated this decision:

1. **Over-reach.** The fragment store makes agent-ways a general manager of
   *user-scoped* Claude Code configuration — config that is not agent-ways' concern.
   `~/.claude/` is properly a dotfiles-class directory (the user's own machine
   config), and an application framework offering to own the whole of it is an
   overstep. ADR-163 already recorded the first symptom: agent-ways was
   force-claiming `statusLine`, "exactly the anti-pattern ADR-147 set out to end,"
   and named dotfiles the cross-host source of truth — but left the *engine* in
   agent-ways.

2. **A concrete redundancy bug.** `WAYS_PERMS` ships both `Edit(~/.claude/**)` and
   `Write(~/.claude/**)`, and `WAYS_DENY` ships `Write(~/.ssh/**)` alongside its
   `Edit`/`Read` siblings. Claude Code's file-permission checks are satisfied by the
   `Edit(path)` rule (which covers the file-writing tools), so the `Write(...)`
   entries are inert and surface as a launch-time warning. Deleting them from the
   live `settings.json` does not stick: the reconciler re-adds them from the source
   constants on the next `ways reconcile`.

3. **Two things wanted to be true at once, and were assumed incompatible.** The
   user should own their Claude config through dotfiles; and a fresh agent-ways
   install with no dotfiles must still be safe and usable. An early proposal to
   collapse to a single writer — lifting the baseline into the dotfiles store — was
   examined and rejected: a security baseline that only holds when dotfiles is
   deployed has a hole.

Two empirical facts (confirmed against `code.claude.com/docs/settings` and the
Claude Code precedence chain) constrain any solution:

- **There is no user-scope `settings.local.json`.** The `.local.json` override
  exists only at *project* scope (`.claude/settings.local.json`). So there is no
  user-level side file into which a tool could isolate `/config`'s writes; every
  user-scope writer lands in the same `~/.claude/settings.json`.
- **Because `/config` and the in-situ writers mutate that single file, any tool
  that owns keys in it must use base-preservation (three-way) merge, not a naive
  compile-and-replace.** A compiler that rewrites the file from fragments would
  clobber the operator's live `/config` toggles on every deploy.

Finally, an explicit product constraint: **the dotfiles-side config tool must work
without agent-ways installed at all**, and (symmetrically) agent-ways must work
without dotfiles. Neither may depend on the other's binary.

## Decision

**agent-ways relinquishes management of user-scoped Claude Code configuration and
retains only its own operational and security baseline in `settings.json`.**

1. **Retain the operational baseline, unchanged in mechanism.** agent-ways keeps
   `settings_merge.rs` as a three-way, base-preserving, self-auditing writer of
   exactly the slices it *must* own for the framework to function and to be safe
   standalone:
   - `hooks` — the entries agent-ways ships (SessionStart disclosure, the ADR-162/167
     deny backstop, etc.).
   - `permissions.allow` — the operational allows for its own binaries
     (`Bash(ways:*)`, `Bash(attend:*)`, `Bash(attend-chat:*)`, `Bash(way-embed:*)`,
     `Edit(~/.claude/**)`).
   - `permissions.deny` — the ADR-152 secret-path baseline (opt-out preserved).

   This slice is disjoint from any user-preference key, self-audits (reverts on any
   change to an unmanaged field), and ships *with the application* so a fresh install
   is safe and usable with no dotfiles present.

2. **Hand the user-config service to dotfiles; keep only self-management.** What
   `ways settings` *did* — author and project user-scoped configuration — did not
   work cleanly and conflicted with the premise that dotfiles owns the user's own
   config. That capability is handed to the dotfiles tool. agent-ways removes the
   fragment store and interview apparatus outright: the `ways settings` CLI
   subcommands (`settings/project.rs`, `settings/compile.rs`, and siblings), the
   `ways-settings` skill, and the now-orphaned schema plumbing
   (`settings_schema_*` in `paths.rs`, `settings_schema_url` in `config.rs`, the
   `refresh-settings-schema.sh` script, and the vendored
   `share/claude-code-settings.schema.json`). This supersedes **ADR-147** and
   **ADR-149**. agent-ways keeps *only enough configuration capability to manage
   itself* — the baseline in (1), possibly nothing more; it retains **no
   user-config surface**. User-scoped configuration (model, `statusLine`, `env`,
   user-authored permissions, prefs) is no longer agent-ways' concern.

3. **User config moves to dotfiles.** The dotfiles-side tool owns the fragment store
   and authoring experience and carries **its own standalone three-way merger** — it
   does not call the `ways` binary. Its architecture is recorded in a companion ADR
   in the dotfiles repository. agent-ways makes no claim on the keys that tool owns.

4. **Coexistence is by disjoint ownership, not a shared engine.** Because each tool
   must run standalone, each carries its own three-way base-preserving merger over a
   *disjoint* set of owned keys, each keying on its own last-applied base and each
   self-auditing hands-off on keys it does not own. Multiple such writers over one
   `settings.json` is safe precisely because their owned sets do not overlap — the
   model ADR-163 validated in practice. A single unified engine was rejected (see
   Alternatives): the standalone-independence requirement forecloses it. The exact
   invariant that makes independent writers safe — disjoint scalar/object ownership,
   additive-union on shared lists with each writer removing only what its own base
   recorded, per-writer last-applied base, and self-audit hands-off — is the
   **peer-writer coexistence contract** specified in
   `docs/design-notes/settings-json-merge-spec-and-peer-writer-contract.md`. The
   dotfiles-side tool ports the same merge algorithm from that spec (shared design
   lineage, not a runtime dependency), so the two mergers behave identically without
   coupling.

5. **Fix the redundancy.** Remove `Write(~/.claude/**)` from `WAYS_PERMS` and
   `Write(~/.ssh/**)` from `WAYS_DENY`. `Edit(...)` already covers the file-writing
   tools; the `Write(...)` entries are inert and only produce the launch warning.
   Removing them at the source constants makes the fix durable through reconcile.

6. **No user-scope local-override layer.** An earlier design sketch proposed a
   `settings.local.json` "escape hatch" layer; it does not exist at user scope and is
   dropped.

7. **Authoritative key-set partition.** Every user-scope `settings.json` key falls
   into exactly one of three buckets (mirrored in the dotfiles-side ADR-010 so the
   partition is agreed on both sides and disjointness holds by set-subtraction, not
   guesswork):

   - **(A) agent-ways baseline** — `hooks` (its shipped entries) +
     `permissions.allow` `{Bash(ways:*), Bash(attend:*), Bash(attend-chat:*),
     Bash(way-embed:*), Edit(~/.claude/**)}` + `permissions.deny` (`WAYS_DENY`). This
     is exactly the `WAYS_PERMS`/`WAYS_DENY` constants — the constant *is* the
     boundary.
   - **(B) user/dotfiles** — everything else user-authored, including the
     agent-ways-*adjacent* tooling that agent-ways does **not** ship as a core binary
     (`way-match`, `kg`, `mmaid`, `adr`/`adr-tool`, the knowledge-graph and
     thinking-strategies MCP servers, shell/prompt tooling, generic `Bash(...)`,
     `Read(~/**)`, …). Owned by the operator via the dotfiles config tool.
   - **(C) Claude Code runtime** — keys Claude Code writes autonomously (`model` and
     the `/config` toggles; `advisorModel`, `effortLevel`, and other in-situ writes).
     **Neither tool owns these**; both must leave them to base-preservation. They must
     never be declared as a managed fragment, or the tool would thrash against Claude
     Code on every run.

8. **Relinquish protocol for the ownership handoff.** The steady-state disjoint
   contract assumes ownership never moves. The retirement *moves* ownership of the
   user-fragment keys (`statusLine`, `attribution`, `env`, …) from agent-ways'
   projector to the dotfiles tool, and that transition has a hazard the contract does
   not cover: if agent-ways' final act treated those keys as *deprecated-ours* (in its
   base, absent from `ours`), its three-way merge would **remove** them from
   `settings.json` — clobbering the value the dotfiles tool now asserts, order-
   dependently. The retirement therefore **relinquishes** rather than deprecated-
   removes: it clears agent-ways' fragment base for those keys and **leaves the live
   values in place** as foreign keys for the dotfiles tool to adopt. In practice, because
   the projector is *removed entirely* (not shipped in a deprecation mode), it simply
   never runs deprecated-removal again — removal *is* the relinquish, **provided the
   removal performs no final "cleanup" projector pass**. Adoption on the other side is
   the ordinary "migrating" behavior: a live foreign value is asserted as `ours`, the
   base is seeded from it, and the result is idempotent. Once agent-ways stops touching
   the keys, steady-state disjointness is restored and run order no longer matters.

**Ratification:** the "how minimal" question — total removal of the user-config
service vs. keeping a minimal affordance for no-dotfiles operators — was ratified in
favour of *self-management only*: agent-ways keeps the baseline in (1) and no
user-config surface. An agent-ways-only operator uses the baseline plus raw
`/config`; managed user config is a dotfiles-tool adoption away. Retaining any
user-config service was rejected as reintroducing the over-reach this ADR removes. A
minimal user surface, if ever wanted, is a purely *additive* future change and does
not gate this decision.

## Consequences

### Positive

- agent-ways stops owning config that isn't its concern; `~/.claude/settings.json`
  returns to being the user's own file plus one narrow, auditable framework slice.
- The launch-time permission warning is fixed durably.
- The security/operational baseline still travels with the app, so a standalone
  install is safe and usable with no dotfiles.
- Each tool is independently installable and testable; neither depends on the other.
- Removes a whole class of dual-control confusion: agent-ways no longer has two
  writers into `settings.json` (the fragment projector goes away).

### Negative

- The three-way merge logic is duplicated across repositories (agent-ways keeps its
  baseline merger; dotfiles builds its own). This is the deliberate price of the
  standalone-independence constraint — one tested engine would have been less code
  but would have coupled the tools.
- Operators who used `ways settings` must migrate their fragments to the dotfiles
  tool. A migration note is required.
- Superseding two Accepted ADRs (147, 149) is a non-trivial reversal of recent
  design; the reasoning must be legible to anyone who read those first.

### Neutral

- ADR-152's deny baseline is retained as-is, now framed explicitly as part of the
  operational baseline agent-ways keeps.
- ADR-163's "dotfiles is the source of truth" direction is carried to completion:
  the *engine* follows the *store* to dotfiles, rather than the store feeding an
  engine that stayed behind.
- ADR-142's projection model is unchanged except that the shared-write seam narrows
  to the baseline slice only.

## Alternatives Considered

- **Single unified engine, layered fragment stores (L1 app-baseline / L2 user /
  L3 host / L4 local).** One loader composing ordered fragments, borrowing the
  dotfiles `zshrc` `conf.d` shape. Rejected on two grounds: (a) the standalone
  constraint requires each tool to write `settings.json` without the other, so a
  single engine cannot live in only one repo; (b) the L4 user-scope
  `settings.local.json` layer it relied on does not exist in Claude Code.

- **Lift the baseline (hooks + `WAYS_DENY`) into the dotfiles store; one writer
  total.** Rejected: a security baseline that only exists when dotfiles is deployed
  strands a fresh agent-ways install. The baseline must ship with the application.

- **dotfiles depends on the `ways` binary as its merge engine (store-only
  dotfiles).** The least-code option and initially preferred. Rejected by the
  explicit constraint that the dotfiles tool must work with no agent-ways installed.

- **A naive compiler (compile-and-replace) instead of a three-way merger.**
  Rejected: `/config` and the in-situ writers mutate the same user-scope file, and a
  compiler would clobber the operator's live toggles on every deploy. Base
  preservation is mandatory.

- **Keep `ways settings` as-is and merely re-posture it as opt-in.** Rejected as
  insufficient: leaving the engine in agent-ways keeps the over-reach and the
  dual-writer surface the user objected to; ADR-163 already showed re-posturing
  alone does not stop the framework from claiming user keys.
