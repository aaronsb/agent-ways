---
description: version-numbered identifiers, function/class/variable names, and docstrings/comments — process_v2, HandlerV2, _FOO_V0, 'v0 seed for the namespace'; the symbol name should describe what the thing is, not which revision it is
vocabulary: identifier symbol naming rename suffix function class variable module docstring comment glueball twin revision snapshot versioned
refire: 0.15
scope: agent,subagent
---
<!-- epistemic: convention -->
# Versioning In Code

## The Antipattern

Putting a version number in something the codebase *owns* — a function, class, variable, module, or its docstring/comment:

```python
def process_v2(...)            # what happened to process? is v1 still called?
_NAMESPACE_V0 = frozenset(...) # v0 of what? against which v1?
"""v0 seed for the namespace.""" # "v0" describes nothing the reader can act on
# v1 of this logic — revisit later
```

The version label is a snapshot of *when it was written*, frozen into a name that lives forever. The old version rarely gets deleted, so `process` and `process_v2` coexist, callers split between them, and every future reader has to reverse-engineer which is current. Repeat across a codebase and you get a glueball: parallel half-migrations nobody dares finish. **Git already holds the history.** The name should describe what the thing *is*, not which revision it is.

## What To Do Instead

- **Rename in place.** Changing `process`? Edit `process`. If the contract changed, the new name should say *how it differs* (`process_streaming`, `parse_strict`) — a behavioral distinction a reader can reason about — not `_v2`.
- **Let VCS carry the timeline.** "Seed for the `crowd-dc` namespace" — not "v0 seed." The reader cares what it seeds, not that it's the zeroth cut.
- **Migrating with a real overlap window?** Then the *old* name gets the deprecation marker and a removal condition (`# remove after all callers move off — tracked in #123`), not a `_v2` twin that silently becomes permanent.

## Not This — Legitimate Version References

This is about identifiers the codebase authors. Versions that name an *external, independently-versioned contract* are correct and expected:

- API/URL paths and protocol versions: `/v1/users`, `apiVersion: apps/v1`
- The package's own release identity: `__version__`, semver in manifests
- A schema/wire-format version that is genuinely a data field: `schema_version`, `payload_version`
- Migration filenames following the tool's convention

The test: *does the version refer to something versioned outside this code?* If yes, keep it. If it's just "the revision I happened to write," strip it.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "I'll keep `_v1` around until callers migrate" | Then mark `_v1` deprecated with a removal condition. An unmarked twin is permanent — that's the glueball. |
| "v0 signals it's an early/partial seed" | "Early" is not actionable. Say what's partial: "anchor type only; full surface accrues with introspectors." |
| "Renaming touches a lot of call sites" | The rename is the cheap part now and only gets more expensive. A versioned alias defers nothing — it adds a second thing to maintain. |
| "The version makes intent clear" | A version number is the absence of intent. `parse_strict` carries intent; `parse_v2` carries a date stamp. |

## See Also

- code/quality(softwaredev) — parent: measurable quality thresholds
- code/errors(softwaredev) — naming clarity at boundaries
