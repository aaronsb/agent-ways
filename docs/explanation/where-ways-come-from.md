---
id: 01.026.E
domain: ways
mode: explanation
related:
  - "[[01.024.R]]"
  - "[[01.025.H]]"
aliases: []
---

# Where ways come from


Most of the shared corpus came from outside it. Someone read or heard about a practice, judged that it would help the agent, and wrote it as a way. Contributions from other people are the same kind of source. This page explains why the project welcomes them, how the maintainer weighs one against the corpus, and what can happen to it. The kinds of contribution and the evidence that helps review them are in [Contribution kinds and the evidence that helps review](../reference/contribution-kinds-and-review-evidence.md).

## Variety from outside the corpus

A person who judges that a practice would help is predicting where the agent's default falls short. A model run with only "you are a helpful assistant" has the variety its training put there and no more. Each way adds a situation the agent now handles deliberately.

The maintainer's own work meets a limited set of situations. A contributor's practice carries situations from other work, which the maintainer may never meet. That variety is the reason to welcome contributions.

Measurement has limits here. A benchmark of typical scenarios under-samples the atypical situations a way exists for, so probe results and ablations inform the maintainer's judgment and do not replace it ([why evidence helps, and its limits](../reference/contribution-kinds-and-review-evidence.md#why-evidence-helps-and-its-limits)).

## Keeping the corpus coherent

The maintainer's judgment also works in the other direction: it keeps the corpus coherent. Every new way can conflict with every way already in the corpus and with the agent's own system prompt, so the number of possible conflicts grows faster than the corpus does. When two ways disagree, the agent's attention splits between them, or one of them quietly loses.

A contribution is therefore read against what is already there. `ways author siblings` and the golden probe show overlap and prompts taken from existing ways. Conflicting rules are found by reading the neighbouring ways and the system prompt the way sits under. When a contribution contradicts an existing way, the maintainer chooses one rule and changes the other. The corpus never keeps both.

## What happens to a contribution

The maintainer may:

- keep it as it is
- tune its triggers
- rewrite it in the corpus register
- merge it into an existing way
- take the idea into a skill or into a tool's own instructions
- set it aside

Each outcome is credited to the pull request the contribution came from. As the corpus grows, the coherence check above leads to more rewrites, merges and set-asides. These outcomes say nothing about the quality of the contribution or of the contributor's practice.

A way that is set aside keeps its value where it was earned, in the contributor's user scope (`$XDG_CONFIG_HOME/agent-ways/ways/`). It can come back to the shared corpus when someone sees the gap it fills.
