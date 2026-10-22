---
contract: adr/v1
kind: decision
verb: add
capability: matching
amends:
  - ADR-196#Decision
basis:
  - operator: aaronsb
    level: directed
    said: "does claude code mods allow us to load a mod that can execute the judge from within the mod, using claude code's access to the inference api, rather than an externally held one? that would be a big deal"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "I've avoided building a judge for most of the agent-ways development maturity life because I needed to manage a seperate api. only recently did the value of the judge warrant an api key use and now here we have the ability to just use the same system that cc dispatches"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "I see the flow and it makes sense. that's an acceptable spend. the only other way I see we could do this is if claude code has a mod hook for an internal evaluator call that would use the authenticated account rather than an independent api token"
    via: "the operator in the head session's chat, 2026-10-22, answering ADR-188's cost probe after seeing its figure"
  - operator: aaronsb
    level: directed
    said: "let's make sure that 'ways settings' (option 3, gate) has a third profile, a claude code mod (we could maybe call it claude code internal?) and should activate/deactivate that as the model source (to ensure we don't double execute)"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - operator: aaronsb
    level: directed
    said: "let's make that the default"
    via: "the operator in the head session's chat, 2026-10-22, right after the third-profile direction; relayed verbatim by the coordinator. Reversed by the next entry"
  - operator: aaronsb
    level: directed
    said: "ok I am reconsidering making it default. we'll make it selectable"
    via: "the operator in the head session's chat, 2026-10-22, after the entry before; relayed verbatim by the coordinator. `claude-code` is selectable and not the default"
  - operator: aaronsb
    level: directed
    said: "it feels like theres a risk of building an unmaintainable matcher monolith here. let's be careful to keep concerns seperated"
    via: "the operator in the head session's chat, 2026-10-22, on ADR-188's Bash pipeline; relayed verbatim by the coordinator"
  - upstream: "Claude Code 2.1.304's bundled mod types (plugin-authoring `claude-code.d.ts`): `$.model.complete` runs one text completion through the session's own API client and credentials, with no tools, no history and no system prompt beyond the CLI's identity block and `request.system`; `ModelCompleteRequest` takes `model` (an alias such as `haiku`, resolved like `--model`), `prompt`, `system`, `maxTokens`, `effort` and `timeoutMs`; `ModelCompleteResult` is the reply's `text` and `usage`, or a `reason` (`api-error`, `empty-reply`, `aborted`); a `$` call's time never counts against the calling hook's budget (`HookBudget`). `$.model.fork` runs a tool-less completion over the session's own transcript from the prompt cache and bills the whole prefix again once the cache entry lapses; `$.model.classify` picks one of a set of labels for one text with a fixed classifier prompt"
  - evidence: "tools/ways-agent-core/profiles.yaml ships two engine profiles, `anthropic` and `openrouter`, both Claude Haiku 4.5 with threshold 0.3, timeout_ms 2000, turns 1, max_turn_chars 1200, concurrency 8 and max_candidates 8; tools/ways-agent-core/src/profile.rs: `SHIPPED_ORDER` is anthropic then openrouter, and `resolve` takes the user layer's `engine` when set, else the first shipped profile whose provider has a key, else no gate; tools/ways-agent-core/src/settings.rs exposes the choice as `gate.engine`"
  - evidence: "tools/ways-cli/src/cmd/scan/gate.rs: on the prompt lane any judge failure, a deadline included, fails open and the matcher's decision stands (ADR-196 §7); a config that does not parse fails closed, with the gate off and nothing sent"
  - evidence: ADR-707, the judge study, where the judge reading narration passed more off-topic candidates than the judge reading the described call alone
  - precedent: ADR-196
  - precedent: ADR-502
  - precedent: ADR-188
agent:
  name: Claude
  model: claude-opus-5-5
status: proposed
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-188
  - ADR-195
  - ADR-196
  - ADR-406
  - ADR-502
  - ADR-606
  - ADR-707
observable:
  - "`ways settings`, gate section, lists three engine profiles: anthropic, openrouter and claude-code, shown as \"Claude Code internal\"; selecting one writes `gate.engine`"
  - "Switching `gate.engine` between claude-code and anthropic in `ways settings` moves judging between the ways mod module and the ways agent; over a session of prompts and Bash calls on each setting, every judged candidate has exactly one judge request across both paths, counted from `judge_call` lines by candidate"
  - "With `gate.engine: claude-code`, the ways agent sends no provider request for any lane: its `judge_call` lines name engine claude-code and carry no provider cost"
  - "With `gate.engine: anthropic` and the ways mod module loaded, the module claims no job and makes no `$.model.complete` call"
  - "With `gate.engine: claude-code` and no ways mod module loaded, no judge request is made, `ways agent status` and the settings gate screen say the engine has no Claude Code host, and each judged lane logs `gate_fallback` with reason `engine_unavailable`"
  - "With `gate.engine` unset and the mod loaded, the engine resolves as before (anthropic, then openrouter, by key) and never to claude-code"
  - "Over at least 50 prompts with candidates under claude-code, the prompt lane's judge latency (p50, p95, and the share past `timeout_ms`) is recorded here beside the key path's"
  - "On a sample of ADR-707's uniform calls and of logged prompt-lane requests, claude-code and anthropic, given the same model and judge text, pass the same candidates closely enough that ADR-195's and ADR-707's numbers carry over; the agreement rate and the disagreeing pairs are recorded here"
  - "Before a release offers the `claude-code` profile, a probe mod on the maintainer's machine completes a judge round trip through `$.model.complete`: a job written, claimed and judged, with the verdicts, the latency and the reply's usage recorded here"
  - "A claude-code reply that is not valid JSON in the `record_judgements` shape is logged as a judge error with reason `invalid_reply`, and is handled as that lane handles any judge failure"
---

# ADR-708: A Claude Code internal engine for the relevance gate, selectable as the only model source

## Summary

- **Decided:** a third shipped engine profile, `claude-code`, shown as "Claude Code internal", sits beside `anthropic` and `openrouter`. It has no key and names the model as an alias Claude Code resolves (`haiku`). Its threshold and limits are the others' until measured. Its judge runs inside Claude Code, in a ways module loaded as a mod, through `$.model.complete` on the session's own signed-in account. `gate.engine` is the single model source for every lane the gate covers, the prompt lane and ADR-188's Bash lane:
  - With `claude-code` selected, the mod module is the only judge and the ways agent sends no provider calls.
  - With `anthropic` or `openrouter` selected, the module stands down even when it is loaded.
  - Nothing falls back from one engine to another, so no candidate is judged twice.
  - When the selected engine is unavailable, each lane does what it does on any judge failure today, and `ways agent status` and the settings gate screen say why.
  - An unset engine keeps today's rule, the first shipped profile whose provider has a key, so loading the mod never switches the engine.
  - This amends ADR-196 §5, which defines the engine profiles, and §6, which ties the gate's approval to a working key.
- **Trades away:** an automatic choice. A user with the mod and a key gets the key engine until they pick Claude Code internal. With `claude-code` selected and no mod loaded (a `claude -p` run, a host without mods), the gate has no judge at all, rather than borrowing a key that exists. Judging spends the account's usage, on a subscription the plan's limits, alongside the session's own work. The prompt lane's judge waits on a mod module answering a job file, a path whose latency is unmeasured. The engine depends on a preview API that can change between Claude Code builds.
- **One-way?** No. The profile is one entry, the module is optional, and selecting another engine turns it off.
- **Probes:** *Confident (switch):* the engine you pick in `ways settings` is the only thing that judges; picking Claude Code internal stops all key-based judge calls, and picking a key engine leaves the mod idle, so nothing is ever judged twice. *Not confident (unavailable):* with Claude Code internal picked and the mod not loaded, for example in a `claude -p` run, prompt-time ways go through unjudged as they do today with no key, and Bash meaning-based ways are not delivered at all. Is that the behaviour you want, rather than falling back to a key you have?
- **Inversion:** at one end, every available judge runs and the first answer wins, which double-spends and makes the source ambiguous. At the other, the judge is tied to whichever host happens to be present. This decision makes the source one explicit setting and gives the absence of that source no substitute.

## Context

The relevance gate (ADR-196) judges candidate ways with Claude Haiku through a provider key the ways agent holds (ADR-502). Two shipped engine profiles name the providers, `gate.engine` selects one, and an unset engine takes the first shipped profile whose provider has a key. ADR-188 extends the gate to the Bash lane, where the judge runs detached and its verdicts arrive through ADR-606's deferred pipeline.

The judge came late to this project because it needed a separate API key to manage; only once its value justified that key was the gate built. Claude Code's mod API offers `$.model.complete`: one text completion through the session's own client and credentials, with no tools and no history. A judge running there needs no key of its own and bills nothing separately. The operator asked whether a mod could run the judge on Claude Code's own access "rather than an externally held one", and then asked for it to be a third profile in the gate settings that activates or deactivates the mod as the model source, "to ensure we don't double execute". A draft of ADR-188 had the mod judge whenever it was loaded and let an unclaimed job fall to the key; that would let one candidate reach two judges, and would let loading a mod change who pays for judging without the operator choosing it.

## Decision

1. **The profile.** `tools/ways-agent-core/profiles.yaml` gains `claude-code`, display name "Claude Code internal", beside `anthropic` and `openrouter`:
   - provider `claude-code`, a new value of `Provider` with no key file and no key environment variable;
   - model `haiku`, an alias Claude Code resolves and allowlist-checks like a `--model` value, so the profile follows Claude Code's own Haiku;
   - threshold 0.3, timeout_ms 2000, turns 1, max_turn_chars 1200, concurrency 8, max_candidates 8, as the shipped profiles, until the agreement and latency observables say otherwise.

   The user layer patches it like any profile. `SHIPPED_ORDER` stays anthropic then openrouter: an unset engine picks the first profile whose provider has a key, and `claude-code` has none, so it is chosen only by name. The profile is selectable, not a default. A release offers it only once the probe round trip in `observable` is recorded.

2. **One model source.** `gate.engine` decides which judge runs, for every lane the gate covers.
   - `claude-code`: the ways mod module is the only judge. The ways agent sends no provider request for any lane.
   - `anthropic` or `openrouter`: the ways agent judges with its key, as today. The mod module reads the same setting and claims no job, even when loaded.
   - Nothing falls back across engines. A job is judged by the selected engine or not at all, so no candidate is judged twice and one setting says who paid.
   - Unset: today's rule. Loading or unloading the mod never changes the resolved engine.

   Selecting `claude-code` is the operator's approval to gate, as a working key is for the key engines (amending ADR-196 §6); `mode` keeps its meaning (enforce, shadow, off).

3. **The host.** The judge runs in a ways component loaded into Claude Code: a small ways mod, or a ways module of the plugin agent-ways ships. It is not part of the attend mod, which carries messaging and session awareness. The module reads `gate.engine` and claims judge jobs only while it names `claude-code`. It registers itself as the session's judge host, so the hook side and `ways agent status` can tell whether one is present.

4. **The request.** The judge text is the gate's own (ADR-196 §2; ADR-188 item 3c's ordered block on the Bash lane). `$.model.complete` takes no tools, so the instruction asks for strict JSON in the shape of the `record_judgements` schema: one entry per candidate with its P(yes). The module parses the reply and validates it against the schema. A reply that does not parse or validate is a judge error with reason `invalid_reply`. The call sets `timeoutMs` to the profile's `timeout_ms`, since a `$` call's time does not count against a hook's budget, and records the reply's `usage`. No sampling parameter is sent; `complete` takes none, so #1130 does not reach this engine.

5. **The prompt lane.** The prompt lane's gate runs synchronously at UserPromptSubmit (ADR-196 §1). Under `claude-code` the hook writes the judge request as a job in the session's directory and waits for the verdict up to `timeout_ms` plus the read grace it allows the ways agent today. The module, in the same Claude Code process, claims the job, judges it through `$.model.complete` and writes the verdict back. How the module learns of a job promptly, by watching the job directory or from an in-process event of its own, is the build's choice, measured by the latency observable. A job not answered within the deadline is a deadline failure, handled as the prompt lane handles one today: it fails open, the matcher's decision stands, and `gate_fallback` is logged with reason `deadline` (ADR-196 §7). The module abandons the call at the same deadline.

6. **The Bash lane.** ADR-188 item 3c's jobs go to the deferred store as before. Under `claude-code` the module claims and judges them, and under a key engine the adapter starts the ways agent detached. Either way the verdicts land in ADR-606's store and one collector delivers them. A judge failure on this lane delivers nothing, as ADR-188 item 3c says.

7. **The selected engine unavailable.** `claude-code` with no judge host registered, or a key engine with no working key, is a judge failure for every job, handled by each lane's own rule: the prompt lane fails open and logs `gate_fallback` with reason `engine_unavailable`; the Bash lane delivers nothing and logs the same. No other engine is tried. `ways agent status` names the engine and why it cannot judge, and the gate screen of `ways settings` shows the same line beside the engine.

8. **Settings.** The gate section of `ways settings` lists the three profiles with their display names; choosing one writes `gate.engine` in the user layer. That choice is how the operator turns the mod on or off as the model source. The screen shows whether a judge host is registered for live sessions when `claude-code` is chosen, and which shipped profile an unset engine resolves to.

9. **Logging and cost.** Every judge call is logged as `judge_call` with the engine. Under `claude-code` it carries the reply's token usage and no provider price, since the spend is the account's usage. `way_judged`, `gate_fallback` and the decision record are written by the hook side as today, whichever engine judged.

## Consequences

### Positive

- A user signed in to Claude Code can run the gate with no API key, no separate bill, and no conversation text sent to a provider other than the one the session already uses.
- One setting names the judge, so a candidate is never judged twice and the spend has one known source.
- The prompt lane and the Bash lane share the switch, so the operator turns the mod on or off as the model source once.
- The engine sends no sampling parameter, so the API change behind #1130 does not touch it.

### Negative

- Judging spends the account's usage: in ADR-707 about 0.57 judge requests per Bash call, each about 1,160 input and 117 output tokens, plus the prompt lane's requests. On a subscription that draws on the plan's limits that the session's own work also uses.
- With `claude-code` selected and no mod loaded, nothing is judged. The prompt lane falls back to the matcher's decision, and the Bash lane delivers no meaning-based guidance, even where a key exists.
- The prompt lane's judge now crosses a job file to a module in another part of the process; its latency is unmeasured, and a slow pickup fails open more often than the key path.
- The judge's instruction differs between engines, a forced tool call on the key path and strict JSON on this one, so ADR-195's and ADR-707's numbers hold for `claude-code` only once the agreement observable is recorded.
- The engine depends on a preview API, and the module is one more component to ship and keep working across Claude Code builds.

### Neutral

- Mods are off by default, so most installs keep the key engines; the fallback-free rule changes nothing for them.
- The shipped profiles are compiled into the agent (`include_str!`), so adding `claude-code` ships with a `ways-agent` release.
- `profile::valid_model_id` already admits `haiku`; the `Provider` enum, its key lookup and the settings choice list gain one value each.

## Alternatives Considered

- **The mod judges whenever it is loaded, with the key as a fallback for unclaimed jobs.** ADR-188's previous draft. Rejected: a slow claim and a fallback can judge one candidate twice and spend twice, and loading a mod would switch who pays without the operator choosing it. The operator asked for one switch.
- **`claude-code` as the default when the mod is loaded.** An unset engine would resolve once per session, to `claude-code` when the ways mod is loaded and otherwise by today's key rule, and hold for the session. Declined by the operator, who first asked for it as the default and then chose to make it selectable only. Loading a mod would otherwise change the engine and who pays without a choice in the settings.
- **Fall back to a key engine when `claude-code` has no host.** Rejected: it reintroduces two judges behind one setting, and the operator's choice of the account over a key would quietly lapse in every `claude -p` run.
- **Host the judge in the attend mod.** It is the mod agent-ways already ships. Rejected: attend carries messaging and session awareness, and the judge is the ways engine's concern; the operator asked that concerns stay separate.
- **`$.model.fork`.** The judge would see the session's whole transcript from the prompt cache. Rejected: in ADR-707 narration in the judge's input brought off-topic passes back, and a fork pays for the whole prefix again whenever the cache entry has lapsed.
- **`$.model.classify`.** One label per text with a fixed prompt. Rejected: the gate judges up to eight candidates in one request against an instruction of its own and needs P(yes) per candidate, which a single label does not give.
- **Make the prompt lane fail closed when the selected engine is unavailable.** Fewer unjudged injections in a run without the mod. Not adopted here: ADR-196 §7's fail-open rule covers every judge failure on the prompt lane, and changing it is a decision about the gate as a whole, not about this engine.
