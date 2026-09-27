---
status: Accepted
date: 2026-03-17
deciders:
  - aaronsb
  - claude
related:
  - ADR-014
  - ADR-103
  - ADR-104
---

# ADR-105: Progressive Disclosure for Way Trees

## Context

As the ways corpus grows (48+ ways at time of writing), flat ways create two problems:

1. **Token bloat**: A 173-line docs way dumps everything on a single trigger, regardless of whether the user needs README guidance, docstring conventions, or Mermaid styling.
2. **Vocabulary crowding**: As more ways share overlapping vocabulary, BM25 discrimination degrades. A broad way with 20 vocabulary terms competes with everything.

Comparison with [obra/superpowers](https://github.com/obra/superpowers) revealed that their skill system uses lazy-loading via the Skill tool, while our hook-based system loads eagerly on match. We need an equivalent of lazy-loading within the hooks architecture.

The supply chain tree (`softwaredev/code/supplychain/`) already demonstrated the pattern organically — 11 files across 3 depth levels with threshold progression from 1.8 to 2.5 and zero vocabulary overlap between siblings.

## Decision

Adopt **tree-structured ways with threshold progression** for complex domains. Specifically:

**Threshold convention**: Root ways use threshold 1.8 (broad catch), mid-tier 2.0 (focused), leaves 2.5 (narrow specialist). This ensures the root fires on general mentions and children only fire on specific sub-topics.

**Vocabulary isolation**: Sibling ways must have Jaccard similarity < 0.15. Each child owns its own keyword space. The `way-tree-analyze.sh` tool validates this at authoring time.

**Parent-aware threshold lowering**: When a parent way's marker exists, children's BM25 thresholds are reduced by 20%. This is evidence-based: if the parent domain is active, children should fire more easily.

**Tree disclosure tracking**: `show-way.sh` records disclosure events to a JSONL metrics file per session, capturing: parent way, depth, epoch distance from parent, and sibling coverage. This data feeds back into authoring decisions (never-fire children need vocabulary broadening).

**Anti-rationalization patterns**: High-stakes leaf ways include "Common Rationalizations" tables that counter the agent's tendency to skip steps. Placed in leaves (not roots) so they only appear when the agent is actively doing the thing it might skip.

**Token budget targets**: Realistic single path ~1200 tokens, worst-case full tree ~4000 tokens. Validated by `/ways-tests budget`.

**When NOT to tree**: Ways under 80 lines with a single cohesive concern stay flat (errors, performance, debugging, config, commits).

## Consequences

### Positive

- Token efficiency: typical session injects ~1200 tokens of relevant guidance instead of ~600 tokens of everything
- Matching precision: narrow child vocabularies reduce false positives and vocabulary crowding
- Anti-rationalization: high-stakes ways actively counter shortcuts the agent might take
- Observability: disclosure metrics reveal which children never fire (vocabulary gaps) and which cascade instantly (vocabulary overlap)

### Negative

- More files to maintain (docs: 1→4, testing: 1→3, security: 1→4)
- Authors must understand threshold progression and vocabulary isolation conventions
- Parent-aware threshold lowering adds a marker check per way in the scan loop (~0.1ms each)

### Neutral

- Think strategies (stateful cognitive scaffolding) follow the same pattern but with explicit stage advancement rather than independent child activation
- The `way-tree-analyze.sh` tool and `/ways-tests tree|budget|crowding|metrics` commands make tree health observable
- Integration test accuracy improved from 87% to 94% after refactoring

## Alternatives Considered

- **Skill-based lazy loading** (obra/superpowers approach): Skills load via the Skill tool on-demand. We considered this but it requires the agent to self-select, which is unreliable. Hook-based triggering is more deterministic.
- **Flat ways with longer vocabulary**: Keep single files but expand vocabulary to cover sub-topics. Rejected because it worsens vocabulary crowding — more terms per way means more overlap between ways.
- **CLAUDE.md includes**: Load sub-content via CLAUDE.md references. Rejected because it's spatial coupling (position in file) rather than temporal coupling (when the agent needs it).

## Reference Implementation

`hooks/ways/softwaredev/code/supplychain/` — 11 files, 3 depth levels, threshold 1.8→2.0→2.5, all sibling Jaccard < 0.06, worst-case ~3840 tokens, average path ~940 tokens.
