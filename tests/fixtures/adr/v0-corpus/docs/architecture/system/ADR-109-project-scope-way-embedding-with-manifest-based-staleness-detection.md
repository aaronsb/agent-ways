---
status: Accepted
date: 2026-03-23
deciders:
  - aaronsb
  - claude
related:
  - ADR-108
  - ADR-107
  - ADR-105
  - ADR-111
  - ADR-125
---

# ADR-109: Project-Scope Way Embedding with Manifest-Based Staleness Detection

## Context

ADR-108 shipped embedding-based way matching using all-MiniLM-L6-v2. The corpus currently contains only global ways (`~/.claude/hooks/ways/`). But Claude Code's way system supports project-local ways at `/path/to/project/.claude/ways/`, and these participate in matching via BM25 and NCD — the embedding engine should cover them too.

Projects are tracked by Claude Code in `~/.claude/projects/<encoded-path>/`. Each may have its own way tree at the project root's `.claude/ways/`. These ways follow the same n-deep progressive disclosure structure as global ways (ADR-105) and may change significantly between sessions as projects evolve.

The gap: a user working in a project with custom ways gets BM25 fallback matching for those ways while global ways get embedding-quality matching. This creates an inconsistent experience — the same prompt triggers different matching quality depending on whether the way is global or project-local.

Additionally, runtime artifacts need clear separation from source. The corpus is a cache (generated from way files), not source data. ADR-108 moved it to `${XDG_CACHE_HOME:-~/.cache}/claude-ways/user/`. All runtime artifacts — binary, model, corpus, and now the embedding manifest — live outside `~/.claude/`.

## Decision

### Unified Corpus Generation

`generate-corpus.sh` scans both global and project-local ways into a single corpus:

1. Embed global ways (`~/.claude/hooks/ways/`) — same as today
2. Scan `~/.claude/projects/` for decoded project paths
3. For each project with `.claude/ways/`:
   a. Lint the ways (must pass to proceed)
   b. Check the inclusion marker at `<project>/.claude/.ways-embed`
   c. Embed included ways into the same corpus

Global ways keep their current format (`softwaredev/code/testing`). Project ways are namespaced by a **project key derived from the project's real path**: `<project-key>/<way-tree-path>`.

> **Implementation note (supersedes the original "encoded project path" wording).**
> The corpus id prefix is *not* the `~/.claude/projects/<encoded-path>` directory name. That encoding (`/`→`-`, and `:`→`-` on Windows) is one-way and lossy, and — critically — the matcher (`ways scan`) only knows the *real* project directory (from `--project` / `CLAUDE_PROJECT_DIR`), never the encoded form. If the corpus prefixed ids with the encoded dir name, the matcher's lookup key would never equal the corpus id and project ways would never match semantically on any platform.
>
> Instead, both sides compute one shared key, `encode_project_key(real_path)` (`ways-cli/src/util.rs`): canonicalize the path, strip Windows verbatim prefixes (`\\?\`), lowercase on Windows, then flatten every separator and `:` to `-`. The result is a flat token, so the single `/` in `<project-key>/<way-tree-path>` is the namespace boundary. Way-tree path segments are joined with `/` on all platforms (`path_to_id`) so ids never leak `\` on Windows.
>
> Corpus generation embeds the **current** project straight from `CLAUDE_PROJECT_DIR` (Windows-safe, no decode) and additionally scans `~/.claude/projects/*` for other projects, deriving each one's key from its *resolved real path* (via `sessions-index.json`, falling back to greedy filesystem decode) — never the raw encoded dir name. The matcher prefixes its project candidates with `encode_project_key(--project dir)`, so the keys agree by construction.

Way trees are n-deep (progressive disclosure, ADR-105), so IDs reflect the full tree path — no fixed domain/way depth assumption.

### Inclusion Markers

A file at `<project>/.claude/.ways-embed` controls whether that project's ways are embedded:

| State | Behavior |
|-------|----------|
| No marker, valid ways found | Create marker = `include`, embed |
| No marker, no ways | Skip silently |
| Marker = `include` | Embed |
| Marker = `disinclude` | Skip, warn if valid ways exist |
| Ways fail lint | Never embed, regardless of marker |

Writing to `<project>/.claude/` is consistent with Claude Code's own behavior — it already writes memory, `settings.local.json`, and permissions there.

### Manifest-Based Staleness Detection

A manifest at `${XDG_CACHE_HOME}/claude-ways/user/embed-manifest.json` records what was embedded using content hashes — not timestamps:

```json
{
  "global_hash": "a1b2c3...",
  "global_count": 58,
  "projects": {
    "-home-aaron-myproject": {
      "path": "/home/aaron/myproject",
      "ways_hash": "d4e5f6...",
      "ways_count": 3
    }
  }
}
```

Hashes are computed from the way files themselves (e.g., `git ls-files -s .claude/ways/ | sha256sum` for tracked files, plus stat of untracked way files). This is content-addressed staleness — immune to clock skew, catches uncommitted edits, and doesn't care *when* things changed, only *whether* they changed.

### Session-Start Staleness Check

At session start (cheap, no embedding work):

1. Read the manifest
2. Compute current content hash for global ways
3. For each project in `~/.claude/projects/`, compute ways hash if `.claude/ways/` exists
4. Compare hashes against manifest entries
5. If any hash differs, or a project exists that isn't in the manifest, trigger regen

Hash computation is one `ls-files | sha256sum` per scope — cheaper than git log, and works for uncommitted changes too. At worst it runs every session start. User-scope ways are relatively static; project-scope ways evolve significantly, making this check worthwhile.

If the manifest is missing or corrupted, treat it as "everything stale" and do a full regen. The manifest is a cache of caches — losing it just costs one regen cycle.

### Staleness is Harmless

If a project is deleted or its ways removed, stale embeddings remain in the corpus but never match — no way file backs them at runtime. Next regen naturally drops them. No eager purge, no reconciliation. Corpus regen is append-from-scan, not diff-and-reconcile.

## Consequences

### Positive

- Project-local ways get embedding-quality matching (98%) instead of BM25 fallback (91%)
- Single corpus, single embedding space — no per-project model overhead
- Content-addressed staleness — immune to clock skew, no date comparison, catches uncommitted edits
- Lint-gating prevents broken or untrusted ways from entering the embedding
- Inclusion markers give projects control without global configuration
- Manifest enables incremental regen — only re-embed when something changed

### Negative

- Session start gains a filesystem scan across `~/.claude/projects/` (should be fast — directory listing + content hash per scope)
- Regen cost grows linearly with project count (each project's ways are additional embedding work, ~20ms per way)
- Manifest is another file to manage in XDG cache (but it's a cache — loss just triggers full regen)
- Inclusion markers in project `.claude/` directories are a write outside the framework's own tree

### Neutral

- BM25 and NCD fallback paths already handle project-local ways — this extends existing behavior to the embedding tier
- The matcher **does** need a change for matching: its project candidates must be looked up under the same `<project-key>/<way-tree-path>` namespace the corpus writes (see implementation note above). The original ADR assumed corpus generation alone sufficed; that was incorrect — the two sides must share one key derivation. Pattern/keyword matching was unaffected (it reads way files directly), which is why the gap surfaced only as silent semantic misses.
- Progressive disclosure (ADR-105) works the same way for project ways — parent/child relationships, sibling coverage, depth tracking all apply. These operate on the bare way id (session markers, show, parent-boost); only the embedding lookup uses the namespaced `<project-key>/...` id.
- `embed-status` CLI tool needs updating to report: manifest contents, per-project inclusion state (included/disincluded/not found), staleness per scope, and project paths.
- Claude Code's project path encoding (`/` → `-`, and `:` → `-` on Windows) is a one-way function. Paths with hyphens in directory names can't be reverse-decoded. The manifest records real paths at embed time and the namespace key is derived from the real path, not the encoded dir name — so decoding is never on the matching critical path. `~/.claude/projects/*` scanning uses `sessions-index.json` (real path) first and greedy decode only as a best-effort fallback for *other* projects; the current project always comes from `CLAUDE_PROJECT_DIR`.

### Footgun guard (added during implementation)

`ways corpus --ways-dir <dir>` historically wrote to and re-embedded the canonical user corpus, silently replacing all global + project ways with just `<dir>`'s ways. Corpus generation now accepts `--output <dir>` to direct artifacts to an isolated location, and warns when `--ways-dir` is used without `--output` against the canonical corpus.

## Alternatives Considered

### Per-project corpus files

Generate a separate corpus per project, load multiple at match time.

Rejected: multiplies file I/O, complicates the scanner, and breaks the "one embedding space" property that makes cosine similarity scores comparable across all ways.

### Embed on every session start unconditionally

Skip the manifest, just regenerate every time.

Rejected: embedding 58+ ways takes ~2 seconds. With multiple projects, this could grow to 5-10 seconds — noticeable on every session start. A content-hash check is effectively free by comparison and only triggers regen when something actually changed.

### Store corpus in project `.claude/` alongside ways

Each project manages its own embedding cache.

Rejected: violates the XDG separation principle. Runtime artifacts belong in `~/.cache/`, not in project trees. Also breaks unified matching.

### No project-scope embedding (status quo)

Keep embedding for global ways only, rely on BM25 for project ways.

Rejected: creates an inconsistent matching experience. The whole point of ADR-108 was that BM25 can't distinguish meaning — that limitation applies equally to project ways.
