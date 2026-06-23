---
id: 01.012.E
domain: system
mode: explanation
related:
  - "[[01.009.E]]"
  - "[[01.011.E]]"
  - "[[01.013.E]]"
  - "[[ADR-139]]"
aliases: []
---

# Scenario — steady-state authoring

**A localized install (Spanish active) evolves: a way is added or edited.** Once an
adopter is in localized mode, maintenance settles into a stable two-part shape: **the
English root + its one localization, kept together.**

## How it plays out

```mermaid
flowchart LR
    A[author / edit a way] --> E[English .md root<br/>source of truth]
    E --> L[regenerate its Spanish alias<br/>translate vs root · tune]
    L --> C[multi corpus rebuilt<br/>English anchor + Spanish layer]
    C --> G{ways tune clean?}
    G -->|no| L
    G -->|yes| D[done — both layers current]

    classDef authoring fill:#7c3aed,stroke:#4a5568,color:#ffffff
    classDef root fill:#2d8e5e,stroke:#4a5568,color:#ffffff
    classDef process fill:#2d7d9a,stroke:#4a5568,color:#ffffff
    classDef gate fill:#fbbf24,stroke:#4a5568,color:#1a1a1a
    classDef done fill:#2d8e5e,stroke:#4a5568,color:#ffffff

    class A authoring
    class E root
    class L,C process
    class G gate
    class D done
```

## What each move is doing

- **The English root is authored normally.** Writing or editing a way means writing
  English `description`+`vocabulary` — exactly as on an English install. The 17×
  all-languages stub tax is gone ([[ADR-139]], Phase A); the root is just English.
- **One localization, not eighteen.** Because `output_language` is `es`, the steady
  state is the root **plus Spanish** — the single active language — regenerated for the
  new or changed way. Not a matrix of every language the project ever shipped; only
  what this adopter actually uses.
- **Re-localization is incremental and root-anchored.** The changed way's Spanish alias
  is re-translated against the (possibly updated) English root and re-tuned. A drifting
  English root pulls its Spanish alias back into alignment — the root leads, the
  localization follows.
- **Tuning gates the change.** `ways tune` re-checks the touched alias: does it still
  align to the English root, and does it still avoid colliding with another way? Clean →
  done; flagged → re-author the stub (see [[01.013.E]]).

## The point

Localized maintenance is not a perpetual translation marathon — it is "write English,
keep one shadow in sync." The cost scales with *one* language and with *what changed*,
not with a language matrix or the whole corpus. This is the steady state the
adopter-run model converges to, and it is sustainable for a single maintainer precisely
because the English root does the anchoring, not a committee of equal translations.
