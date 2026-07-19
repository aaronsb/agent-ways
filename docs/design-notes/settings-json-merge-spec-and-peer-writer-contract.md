# settings.json three-way merge — spec and peer-writer coexistence contract

Status: reference for ADR-169. Portable specification of the algorithm implemented
in `tools/ways-cli/src/cmd/settings_merge.rs`, extracted so an independent tool
(e.g. a dotfiles-side settings projector) can **port the proven algorithm** rather
than depend on the `ways` binary at runtime. Shared design lineage, not a shared
dependency.

This note is language-neutral. The reference implementation is Rust over
`serde_json::Value`; the reference tests are its `#[test]` suite. Any port should
reproduce the **test vectors** in the last section.

## Why a merge, not a compile

`~/.claude/settings.json` is a single, shared-write JSON object with several
independent writers, none coordinating:

- **Claude Code / `/config`** persists user preferences (theme, model,
  `autoCompactEnabled`, notification toggles) directly into the user-scope file.
- **Claude Code in situ** writes some keys mid-session (`advisorModel`,
  `effortLevel`).
- **One or more config tools** (agent-ways' baseline; a dotfiles projector) each
  want to own a *slice* of the file.

There is **no user-scope `settings.local.json`** to isolate `/config` into (that
override is project-scope only). So a tool that rewrote the file from its own
sources would clobber the operator's live toggles on every run. The only safe
design is a **three-way merge keyed on a persisted last-applied base**: write only
your slice, preserve everything else exactly, and prove you did.

## Data model

A writer owns a fixed, declared set of keys — its **owned slice**. It persists, in
its own state dir (host-local, gitignored), the exact content it wrote last time —
the **base**:

```
base = {
  <owned object/list keys> : <exact values last written>
}
```

Three inputs to every merge:

- **theirs** — the live `settings.json` as it is now (may contain foreign edits).
- **ours** — the slice the writer wants to assert this run.
- **base** — what this writer wrote last run (may be empty on first run, or stale).

## Core merge law

Per owned key, combine by the key's JSON type:

### Scalars and objects the writer owns exclusively

Exclusive-owner override: `result[k] = ours[k]`. The writer must own the key
outright — no other writer may declare it (see the coexistence contract). On
opt-out / retirement of a key, drop it (see deprecated-base removal).

### Lists the writer contributes to but does not own outright

`permissions.allow`, `permissions.deny`, and similar concat-semantics lists are
**shared**: the user and multiple tools all add entries. The rule is
**additive union with deprecated-base removal**:

```
deprecated = base[k] − ours          # entries we added before and no longer want
result[k]  = (theirs[k] − deprecated − ours) ++ ours
```

- Dropping `deprecated` removes entries this writer previously added and has since
  stopped asserting (e.g. an opt-out, or a renamed permission).
- Dropping `ours` before re-appending dedupes a re-apply (idempotency).
- Everything else in `theirs[k]` — the user's own entries and *other tools'*
  entries — is preserved.
- If the result is empty, remove the key entirely rather than leaving `[]`.

### Keyed collections (hooks, per event)

`hooks` is an object of event → array-of-entries. Merge **per event**, union of
event keys (theirs first, then any new ones), and within each event apply the list
law above, with one addition: a **structural ownership backstop**. Match "our"
entries against the base **and** by structure — an entry whose executable path
points into the tool's own projected tree (e.g. `.claude/hooks/`, `.claude/bin/`)
is recognized as ours even if the base is stale or the command changed. Inspect
only the **first (executable) token**, so a *user* hook that passes a projected
path as an *argument* is preserved, not captured.

Write the base in the **exact serialized form** you emit (e.g. with the executable
path quoted for Windows-space safety), so a settled install recognizes its own
entries on re-apply and does not duplicate them.

## Self-audit (mandatory)

The merge is only safe if it provably touched nothing outside the owned slice.

1. Compute `stripped_user_view(settings, base)` = the document with the owned slice
   removed (strip owned list entries and owned hook entries by base **and** by
   structural signature; strip owned scalars/objects).
2. Back up the live file.
3. Write the merged document **atomically** (temp file + rename).
4. Recompute `stripped_user_view` of what you just wrote. If it is not **semantically
   equal** (JSON `Value` equality, not byte equality — re-serialization may reorder
   or reformat) to the stripped view of the backup, **revert from the backup and
   fail loud**. A botched merge becomes a loud failure, never silent corruption.
5. Persist the new base only after a verified write. Persist it as `ours` unioned
   with the prior base (`union_owned`) so an under-recording base still strips
   everything you own on the next audit.

Early-return before writing if the merged document equals the live one — idempotent
runs are silent and touch nothing.

## Peer-writer coexistence contract

Multiple independent three-way writers (agent-ways' baseline; a dotfiles projector;
future tools) may share one `settings.json` **iff all of the following hold**. This
is the invariant ADR-169 and the dotfiles-side ADR both depend on.

1. **Disjoint scalar/object ownership.** No two writers may declare the same scalar
   or owned-object key. Exactly one writer owns `model`; exactly one owns each hook
   event's tool entries; etc. Overlap is the only true hazard — two writers fighting
   over one scalar will thrash on alternate runs.
2. **Additive-union on shared lists.** For `permissions.allow` / `permissions.deny`,
   every writer uses additive-union-with-deprecated-base-removal and removes **only
   what its own base recorded**. Never remove a list entry you did not add.
3. **Per-writer last-applied base.** Each writer keeps its own base in its own state
   dir. A writer's base describes only its own contribution.
4. **Self-audit hands-off.** Each writer's self-audit treats every key it does not
   own as part of the user view — so it reverts if it ever perturbs another writer's
   key or the user's own keys.

Under these rules the writers commute: the file converges to the same content
regardless of the order they run, and none can silently undo another's or the
operator's edits. `/config` and the in-situ writers are just another "foreign"
editor that every writer preserves.

## Test vectors

A port should reproduce these behaviors (names mirror the reference suite in
`settings_merge.rs`). Each is a merge over `(theirs, ours, base)` with an expected
outcome:

| Vector | Setup | Expected |
|--------|-------|----------|
| `fresh_merge_adds_hooks_and_perms` | empty base, empty theirs | owned hooks present; all owned allow-perms present |
| `merge_is_idempotent` | apply twice | second result == first |
| `preserves_unrelated_user_keys` | theirs has `model`, `theme`, user `deny` | all survive untouched |
| `deny_baseline_added_when_enabled` | deny opt-in | every baseline deny entry present; base records them |
| `deny_baseline_absent_when_opted_out` | deny opt-out | no `deny` key; base deny empty |
| `deny_preserves_user_deny_entries` | user has own `deny` + opt-in | user entry **and** baseline both present |
| `opt_out_removes_previously_owned_deny_keeps_user` | had baseline, now opt-out | only the user's own deny remains |
| `opt_out_first_reconcile_keeps_user_deny_matching_baseline` | first run, opt-out, user deny equals a baseline path | user's entry preserved (not treated as ours) |
| `deny_is_idempotent` | re-apply baseline | stable |
| `user_view_invariant_holds_with_deny` / `...across_merge` | any merge | stripped user view before == after |
| `preserves_user_authored_hooks` | user hook in same event | user hook survives; both present |
| `removes_our_deprecated_hooks_but_keeps_user` | base has an old hook of ours no longer asserted | old one removed; user hook kept |
| `quote_first_token_quotes_the_exe_path` | spaced exe path | first token quoted; already-quoted left alone |
| `migrating_existing_install_seeds_base_and_passes_self_audit` | pre-existing settings, empty base | base seeded from live; user keys preserved; old tool hook replaced not duplicated |
| `reapply_is_idempotent_when_base_hooks_were_lost` | base hooks lost | our hook not duplicated; user hook preserved |
| `update_that_changes_our_hooks_with_a_lost_base_converges` | our command changed + lost base | converges, no duplicate, no spurious revert |
| `command_is_ours_examines_the_exe_token_only` | projected path as exe vs as arg | exe → ours; arg → not ours |
| `entry_is_ours_requires_every_command_ours` | multi-command entry, mixed | ours only if **every** command is ours |
| `merge_preserves_user_hook_referencing_our_path_as_argument` | user hook, our path as arg | preserved |
| `shipped_hooks_are_all_recognized_as_ours` | every hook the tool ships | recognized by the structural backstop |

## See also

- ADR-169 — agent-ways relinquishes user-scoped settings.json; retains only its
  operational baseline (this note is its merge reference).
- ADR-142 — the projection model and the original shared-write seam.
- ADR-152 — the secret-path deny baseline (the opt-in/opt-out vectors above).
- ADR-163 — dotfiles as source of truth feeding the fragment store.
