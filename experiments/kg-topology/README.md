# kg topology: way vocabulary compiled from a knowledge graph

Measured 2026-10-09. Self-evaluated.

The ways corpus was ingested into a local knowledge-graph-system instance (one ontology per top-level domain, frontmatter stripped, extraction by claude-sonnet-5-5, embeddings by ModernBERT). Concepts were read back per way, way-to-way edges were built from shared concepts and kg relationships, and distinctive concepts missing from each way's frontmatter were proposed as `vocabulary:` terms. kg was only read after ingest. Agent-ways code is unchanged.

## Files

- `concepts-per-way.json`, `concept-to-ways.json`: the readback, keyed by way path.
- `topology.json`: way-to-way edges, the comparison with authored See Also links, isolated ways, the annealing record.
- `vocabulary-applied.json`: the terms added per way, after pruning.
- `gates.json`: probe results on main and on the branch.
- `scripts/`: the readback, topology, candidate and gate scripts. They hardcode their working directory (`/tmp/kgtopo-a3601`); edit it before rerunning.

## Results

| Set | main pass / top-1 | with kg vocabulary |
|---|---|---|
| tree-sample (133) | 89 / 76 | 91 / 78 |
| tree-sample-joined (81) | 57 / 65 | 58 / 69 |
| tree-sample-pleasantry (133) | 89 / 76 | 91 / 78 |
| unrelated sets (6) | 0 fire | 0 fire |
| sealed held-out, evaluative ways (24) | 10 / 9 | 11 / 11 |

The held-out set was written blind from one-sentence situations (ADR-605, `evaluative/held-out`) and scored three times in all: 9 before the evaluative ways were tuned, 10 after, 11 with these terms. It is retired; the next comparison gets a fresh set. Direct asks went from 7 to 8 of 12; passing mentions stayed at 3 of 12.

664 terms were added across 138 ways. Additions that cost a pass were pruned: tool names on broad parent ways lowered their scores on situational prompts (depscan fell from 0.56 to 0.36 after gaining `osv rustsec cargo-audit`), and itops/policy's own terms made it lose to itops/proposals.

## Topology

- Only 63 of about 1,700 concepts appear in more than one way, and kg supports 131 of the 297 authored See Also links. Many concepts name their own way ("Security Way"), so each document forms its own cluster. Stripping the way's name from the ingested prose is the next input change.
- One missing link: `documentation/mermaid` and `softwaredev/visualization/diagrams` share three concepts (diagram-type choice, flowchart overuse, GitHub Mermaid compatibility) and had no link. Both now carry See Also lines. Their Jaccard of 0.19 is the highest in the corpus; no pair is a near-duplicate.
- Annealing folded collaboration, data, itops, research and writing into kg's default pool, and suggested, at confidence 0.40 to 0.45, splitting the larger domains.
