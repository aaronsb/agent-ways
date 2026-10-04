---
hide:
  - toc
---

# A turn with agent-ways

Two minutes, in fourteen beats. The animation follows one Claude Code turn as the hooks see it: core at session start, a prompt matched by the keyword and semantic lanes, the relevance judge, tool calls and a subagent dispatch, the end of the turn, and re-disclosure as the context window fills. Then it widens to two sessions and a person on the attend bus, showing how a message reaches a busy session and an idle one.

<div style="position: relative; width: 100%; height: 0; padding-top: calc(56.25% + 48px); border-radius: 8px; overflow: hidden;">
<iframe src="../animations/a-turn-with-ways/index.html?embed" title="Animated explainer: a turn with agent-ways" style="position: absolute; inset: 0; width: 100%; height: 100%; border: 0;" loading="lazy" allowfullscreen></iframe>
</div>

[Open it full size](animations/a-turn-with-ways/index.html){ .md-button }

Space plays or pauses, and the timeline jumps to any beat. Adding `?t=` and a number of seconds to the full-size page's address opens it at that moment. The prompt, the scores, and the session names are illustrative. The hook names, thresholds, commands, and way triggers are the shipped ones.

## Where each beat is documented

| Beat | Read more |
|---|---|
| Session start, the prompt lanes, the fire rule | [Matching and routing](../hooks-and-ways/matching.md), [Matching engine reference](../hooks-and-ways/engine-reference.md) |
| The relevance judge | [The relevance judge](../explanation/relevance-judge/relevance-judge-the-model.md) |
| Tool calls, subagents, the Stop hooks | [Hooks and ways system](../hooks-and-ways.md) |
| Re-disclosure | [Context decay](../hooks-and-ways/context-decay.md) |
| Sessions, the bus, the person in attend-chat | [Attend overview](../attend-and-monitor/README.md), [Delivery](../attend-and-monitor/delivery.md), [attend-chat](../attend-and-monitor/tui.md) |

## As a video

The same two minutes on YouTube, for sharing or for a reader without JavaScript.

<div style="position: relative; width: 100%; aspect-ratio: 16 / 9; border-radius: 8px; overflow: hidden;">
<iframe src="https://www.youtube-nocookie.com/embed/Ma4sDBPJVvU" title="A turn with agent-ways (video)" style="position: absolute; inset: 0; width: 100%; height: 100%; border: 0;" loading="lazy" allow="encrypted-media; picture-in-picture; fullscreen" referrerpolicy="strict-origin-when-cross-origin" allowfullscreen></iframe>
</div>

[Watch on YouTube](https://youtu.be/Ma4sDBPJVvU)

`make explainer-video` renders the animation to `build/explainers/a-turn-with-ways.mp4` (1080p, 30 fps). [Making explainers](authoring.md) covers the recorder and how to build a new one.
