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
  - operator: aaronsb
    level: directed
    said: "this also should be something decided during install or setup of a fresh ways installation. I'm not sure if our lifecycle management for it surfaces this configuration choice now already, but it should be one of the three choices for judge (the fourth I suppose is to not configure one but that severely impacts the quality of ways)"
    via: "the operator in the head session's chat, 2026-10-22; relayed verbatim by the coordinator"
  - evidence: "scripts/install.sh runs `ways judge-setup` in its Judge section (the `judge_setup` function, its comment and body at lines 423-444 on main at 7fd6c0de, called at lines 496 and 565), and `ways manage update` runs the same check; tools/ways-cli/src/cmd/judge.rs `setup()` and `pick()` offer anthropic or openrouter, add the key through `ways-agent key add` and check it; the degraded warning (every matched way is injected) prints before the offer, and a skip prints `Skipped.`; all of these read `ways_agent_core::keys::judge_ready`, whose `Readiness` (Ready, Off, NoKey, Unverified, Config) knows only keys"
  - upstream: "Claude Code 2.1.304's bundled mod types (plugin-authoring `claude-code.d.ts`): `$.model.complete` runs one text completion through the session's own API client and credentials, with no tools, no history and no system prompt beyond the CLI's identity block and `request.system`; `ModelCompleteRequest` takes `model` (an alias such as `haiku`, resolved like `--model`), `prompt`, `system`, `maxTokens`, `effort` and `timeoutMs`; `ModelCompleteResult` is the reply's `text` and `usage`, or a `reason` (`api-error`, `empty-reply`, `aborted`); a `$` call's time never counts against the calling hook's budget (`HookBudget`). `$.model.fork` runs a tool-less completion over the session's own transcript from the prompt cache and bills the whole prefix again once the cache entry lapses; `$.model.classify` picks one of a set of labels for one text with a fixed classifier prompt"
  - evidence: "tools/ways-agent-core/profiles.yaml ships two engine profiles, `anthropic` and `openrouter`, both Claude Haiku 4.5 with threshold 0.3, timeout_ms 2000, turns 1, max_turn_chars 1200, concurrency 8 and max_candidates 8; tools/ways-agent-core/src/profile.rs: `SHIPPED_ORDER` is anthropic then openrouter, and `resolve` takes the user layer's `engine` when set, else the first shipped profile whose provider has a key, else no gate; tools/ways-agent-core/src/settings.rs exposes the choice as `gate.engine`"
  - evidence: "tools/ways-cli/src/cmd/scan/gate.rs: on the prompt lane any judge failure, a deadline included, fails open and the matcher's decision stands (ADR-196 §7); a config that does not parse fails closed, with the gate off and nothing sent"
  - evidence: ADR-707, the judge study, where the judge reading narration passed more off-topic candidates than the judge reading the described call alone
  - operator: aaronsb
    level: directed
    said: "we're using haiku 4.5 because it proved to be more effective than 5.5. let's continue with 4.5 through our improvements, then once we got everything working we'll re-evaluate the model choice and see if 5.5 is better"
    via: "chat, session 2026-10-22"
  - evidence: "ADR-709, the handoff spike: a process a `prompt.submit` handler starts beside `next(e)` answers the job a UserPromptSubmit settings hook waits on inside `next(e)`; `$.session.id()` equals the hook's `session_id`; `prompt.submit` fires under `claude -p` with origin `sdk`; and at judge sizes `claude-haiku-4-5` answered with median 1162 ms and 2 of 20 calls over 2000 ms, against median 1938 ms and 23 of 50 for the alias `haiku`, which Claude Code resolves to claude-haiku-5-5"
  - precedent: ADR-196
  - precedent: ADR-502
  - precedent: ADR-188
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "if the selected engine is unavailable, then our 'error'|fallback is to configure one of the api key paths (anthropic,openrouter)"
    via: chat in the agent-ways session, 2026-10-22, answering the unavailable probe
    covers: [unavailable]
  - operator: aaronsb
    said: "1131 reads as we discussed I am ok with it"
    via: "chat in the agent-ways session, 2026-10-22, after reading PR #1131 (the PR body listed the three agent-made calls: Bash lane judges every candidate in batches of 8; tool_event default [PreToolUse, UserPromptExpansion]; no-judge setup writes gate.mode off)"
status: accepted
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
  - ADR-709
observable:
  - "`ways settings`, gate section, lists three engine profiles: anthropic, openrouter and claude-code, shown as \"Claude Code internal\"; selecting one writes `gate.engine`"
  - "Switching `gate.engine` between claude-code and anthropic in `ways settings` moves judging between the ways mod module and the ways agent; over a session of prompts and Bash calls on each setting, every judged candidate has exactly one judge request across both paths, counted from `judge_call` lines by candidate"
  - "With `gate.engine: claude-code`, the ways agent sends no provider request for any lane: its `judge_call` lines name engine claude-code and carry no provider cost"
  - "With `gate.engine: anthropic` and the ways mod module loaded, the module claims no job and makes no `$.model.complete` call"
  - "With `gate.engine: claude-code`, no ways mod module loaded and an Anthropic key configured, the session resolves to `anthropic` at its first judge request: every judge request of that session goes to the Anthropic API and none to `$.model.complete`, the session's resolution file names `anthropic` with reason `claude-code unavailable: no Claude Code host`, and `ways agent status` shows the same"
  - "With `gate.engine: claude-code`, no ways mod module loaded and no key configured, no judge request is made; `ways agent status`, the settings gate screen and each judged lane's `gate_fallback` line (reason `engine_unavailable`) say to configure an Anthropic or OpenRouter key"
  - "A ways mod module that loads after its session resolved to a key engine claims no job for the rest of that session; the next session resolves to `claude-code`"
  - "With `gate.engine` unset and the mod loaded, the engine resolves as before (anthropic, then openrouter, by key) and never to claude-code"
  - "Over at least 50 prompts with candidates under claude-code, the prompt lane's judge latency (p50, p95, and the share past `timeout_ms`) is recorded here beside the key path's"
  - check: "On a sample of ADR-707's uniform calls and of logged prompt-lane requests, claude-code and anthropic, given the same model and judge text, pass the same candidates closely enough that ADR-195's and ADR-707's numbers carry over; the agreement rate and the disagreeing pairs are recorded here"
    status: open
    note: "The 2026-10-22 probe below is a smoke test of 20 calls and 85 pairs, Bash lane only; the carry-over measurement needs a larger sample and prompt-lane requests"
  - check: "Before a release offers the `claude-code` profile, a probe mod on the maintainer's machine completes a judge round trip through `$.model.complete`: a job written, claimed and judged, with the verdicts, the latency and the reply's usage recorded here"
    observed: "2026-10-22 about 20:14Z, Claude Code 2.1.308, on the maintainer's machine: `claude -p --plugin-dir <probe mod> \"/judge-probe 20 1\"`, a probe mod whose slash command builds ADR-707 arm E's judge prompts byte-identical to the study's and sends them, sequentially, through `$.model.complete` with model claude-haiku-4-5, a strict-JSON instruction and timeoutMs 8000. 20 of 20 calls answered, no API errors, no parse failures; every reply came wrapped whole in a ```json fence, which the probe's parser accepted. Against the key path's arm E verdicts the two agreed on 79 of 85 pairs (both pass 20, probe only 3, key only 3, both block 59), all 6 disagreements on pairs graded ADJ. Passed by prior grade: ON 12 of 12, ADJ 10 of 21, OFF 1 of 52. Latency median 1,241 ms, range 681 to 2,840 ms. Usage 9,886 input and 2,563 output tokens over the 20 calls, 296 to 792 input per call, no cache reads. A mod slash command ran under `claude -p`, so the path can be tested headless. ADR-707's addendum has the table"
    status: "round trip observed; the job-file handoff (written by the hook side, claimed by the module) was not exercised, since the probe sent the prompts itself"
    results: "scratchpad judge-probe-mod/results/2026-10-22T20-14-06-037Z.jsonl, kept outside the repository"
  - "A fresh install's Judge step offers four choices in order: Claude Code internal, an Anthropic key, an OpenRouter key, no judge"
  - "Choosing Claude Code internal at setup leaves `gate.engine: claude-code`, the ways mod installed and enabled, and no key stored; `ways agent status` reports the judge ready"
  - "Choosing no judge at setup prints the degraded warning naming the quality cost, leaves `gate.mode: off`, and `ways agent status` says the judge is off by choice"
  - "On a Claude Code version that cannot load mods, the Judge step says so and offers only the key choices and no judge"
  - "A claude-code reply that is not valid JSON in the `record_judgements` shape is logged as a judge error with reason `invalid_reply`, and is handled as that lane handles any judge failure"
---

# ADR-708: A Claude Code internal engine for the relevance gate, selectable, with one judge engine per session

## Summary

- **Decided:** a third shipped engine profile, `claude-code`, shown as "Claude Code internal", sits beside `anthropic` and `openrouter`. It has no key and pins the model to `claude-haiku-4-5`, as the key profiles do. Its threshold and limits are the others' until measured. Its judge runs inside Claude Code, in a ways module loaded as a mod, through `$.model.complete` on the session's own signed-in account. Each session has one judge engine, resolved once from `gate.engine` and held for the whole session, for every lane the gate covers, the prompt lane and ADR-188's Bash lane:
  - With `claude-code` selected and the mod loaded, the mod module is the only judge and the ways agent sends no provider calls.
  - With `claude-code` selected and no mod loaded, the session resolves to a configured key engine, anthropic then openrouter, as the operator directed; with no key either, `ways agent status`, the settings gate screen and the log say to configure one.
  - With `anthropic` or `openrouter` selected, the module stands down even when it is loaded.
  - No job falls back from one engine to another, so no candidate is judged twice.
  - An unset engine keeps today's rule, the first shipped profile whose provider has a key, so loading the mod never switches the engine.
  - This amends ADR-196 §5, which defines the engine profiles, and §6, which ties the gate's approval to a working key.
- **Trades away:** an automatic choice. A user with the mod and a key gets the key engine until they pick Claude Code internal. With `claude-code` selected and no mod loaded (a `claude -p` run, a host without mods), the session judges on a key and pays the provider; with no key it has no judge. Judging spends the account's usage, on a subscription the plan's limits, alongside the session's own work. The prompt lane's judge waits on a mod module answering a job file, a path whose latency is unmeasured. The engine depends on a preview API that can change between Claude Code builds.
- **One-way?** No. The profile is one entry, the module is optional, and selecting another engine turns it off.
- **Probes:** *Confident (switch):* the engine you pick in `ways settings` is the only thing that judges; picking Claude Code internal stops all key-based judge calls, and picking a key engine leaves the mod idle, so nothing is ever judged twice. *Not confident (unavailable):* answered; with Claude Code internal picked and the mod not loaded, for example in a `claude -p` run, the session judges on one of your key engines for the whole session; with no key either, nothing is judged and status tells you to configure one. The question as first asked offered no fallback; the operator's answer, recorded under `considered`, chose the key paths.
- **Inversion:** at one end, every available judge runs and the first answer wins, which double-spends and makes the source ambiguous. At the other, the judge is tied to whichever host happens to be present. This decision makes the source one explicit setting, resolved once per session, with the key engines as the only substitute when the mod is absent.

## Context

The relevance gate (ADR-196) judges candidate ways with Claude Haiku through a provider key the ways agent holds (ADR-502). Two shipped engine profiles name the providers, `gate.engine` selects one, and an unset engine takes the first shipped profile whose provider has a key. ADR-188 extends the gate to the Bash lane, where the judge runs detached and its verdicts arrive through ADR-606's deferred pipeline.

The judge came late to this project because it needed a separate API key to manage; only once its value justified that key was the gate built. ADR-196's Context rested on Claude Code offering no hook-side model call; that premise no longer holds for a mod, as the probe recorded under `observable` shows, and a dated note on ADR-196 says so. Claude Code's mod API offers `$.model.complete`: one text completion through the session's own client and credentials, with no tools and no history. A judge running there needs no key of its own and bills nothing separately. The operator asked whether a mod could run the judge on Claude Code's own access "rather than an externally held one", and then asked for it to be a third profile in the gate settings that activates or deactivates the mod as the model source, "to ensure we don't double execute". A draft of ADR-188 had the mod judge whenever it was loaded and let an unclaimed job fall to the key; that would let one candidate reach two judges, and would let loading a mod change who pays for judging without the operator choosing it.

## Decision

1. **The profile.** `tools/ways-agent-core/profiles.yaml` gains `claude-code`, display name "Claude Code internal", beside `anthropic` and `openrouter`:
   - provider `claude-code`, a new value of `Provider` with no key file and no key environment variable;
   - model `claude-haiku-4-5`, the model every engine uses. The operator chose Haiku 4.5 over Haiku 5.5 as the more effective judge for this build, and the choice is re-evaluated once the engine and the Bash lane work end to end. The alias `haiku` is not used: under Claude Code it resolves to claude-haiku-5-5, which thinks, and at judge sizes it ran slower and passed the 2000 ms deadline more often (ADR-709 finding 5);
   - threshold 0.3, timeout_ms 2000, turns 1, max_turn_chars 1200, concurrency 8, max_candidates 8, as the shipped profiles, until the agreement and latency observables say otherwise.

   The user layer patches it like any profile. `SHIPPED_ORDER` stays anthropic then openrouter: an unset engine picks the first profile whose provider has a key, and `claude-code` has none, so it is chosen only by name. The profile is selectable, not a default. A release offers it only once the probe round trip in `observable` is recorded.

2. **One engine per session.** Each session resolves its judge engine once and holds it for the session, for every lane the gate covers.
   - **When.** At the session's first judge request, by the hook side, which writes the result to a resolution file in the session's directory: the engine and the reason (`explicit`, `key found`, or `claude-code unavailable: no Claude Code host`). Every later request in the session, and the mod module, read that file. Resolving at the first request rather than at SessionStart leaves the mod time to register itself as the session's judge host.
   - **`gate.engine: claude-code`.** With a judge host registered, the session resolves to `claude-code`: the ways mod module is the only judge, and the ways agent sends no provider request for any lane. With none, the session resolves to the first shipped key engine whose provider has a key, anthropic then openrouter, as the operator directed for an unavailable engine. With no key either, the session has no engine (item 7).
   - **`gate.engine: anthropic` or `openrouter`.** The session resolves to that engine. The mod module reads the resolution and claims no job, even when loaded.
   - **Unset.** Today's rule: the first shipped profile whose provider has a key. Loading or unloading the mod never changes it.
   - **No per-job fallback.** A job is judged by its session's engine or not at all. A mod that registers after its session resolved to a key engine stays idle until the next session. One candidate never reaches two judges, and the resolution file says who paid.

   The fallback reuses the key rule an unset engine already follows, so it adds no setting. A user who wants a particular key engine as the fallback names it by leaving only that provider's key configured; a `gate.fallback_engine` setting is left out until someone needs it.

   Selecting `claude-code` is the operator's approval to gate, as a working key is for the key engines (amending ADR-196 §6); `mode` keeps its meaning (enforce, shadow, off).

3. **The host.** The judge runs in a ways component loaded into Claude Code: a small ways mod, or a ways module of the plugin agent-ways ships. It is not part of the attend mod, which carries messaging and session awareness. It registers itself as the session's judge host from `prompt.submit`, before each session id's first prompt reaches `next(e)`, so the hook side and `ways agent status` can tell whether one is present. It does not register at `session.start`, because `/clear` and an in-session `/resume` change the session id with no `session.start`. It reads its session's resolution through `ways judge` and claims jobs only when the resolution names `claude-code`.

4. **The request.** The judge text is the gate's own (ADR-196 §2; ADR-188 item 3c's ordered block on the Bash lane). `$.model.complete` takes no tools, so the instruction asks for strict JSON in the shape of the `record_judgements` schema: one entry per candidate with its P(yes). The module only transports: it sends the job's request through `$.model.complete` and hands the reply's text and `usage`, or its failure `reason`, back to the hook side. The hook side parses the reply and validates it against the schema in `judge::parse_reply`, which accepts a reply that is one fenced JSON block. A reply that does not parse or validate is a judge error with reason `invalid_reply`. The Bash lane's judge uses the same parser. The call sets `timeoutMs` to the profile's `timeout_ms`, since a `$` call's time does not count against a hook's budget. No sampling parameter is sent; `complete` takes none, so #1130 does not reach this engine.

   **Correction, 2026-10-22:** the call's `timeoutMs` is the job's `timeout_ms`, what remains of the gate's `timeout_ms` budget when the hook writes the job, not the profile's whole `timeout_ms`; with under 50 ms remaining the hook writes no job.

5. **The prompt lane.** The prompt lane's gate runs synchronously at UserPromptSubmit (ADR-196 §1). When the session's engine is `claude-code`, the hook writes the judge request as a job in the session's directory and waits for the verdict up to `timeout_ms` plus the read grace it allows the ways agent today. The module, in the same Claude Code process, claims the job, judges it through `$.model.complete` and writes the verdict back. How the module learns of a job promptly, by watching the job directory or from an in-process event of its own, is the build's choice, measured by the latency observable. A job not answered within the deadline is a deadline failure, handled as the prompt lane handles one today: it fails open, the matcher's decision stands, and `gate_fallback` is logged with reason `deadline` (ADR-196 §7). The module abandons the call at the same deadline.

   **Note, 2026-10-22: the transport.** The build carries the handoff in a `prompt.submit` bracket, which ADR-709 tested with stand-ins. The mod's `prompt.submit` handler starts `next(e)`, which runs the UserPromptSubmit settings hooks and with them the ways scan, and does not await it. Alongside it, the handler spawns `ways judge take --session <id> --wait` with `$.process.spawn`. `take` returns when the scan writes a job or when the session's resolution names a key engine. When `next(e)` settles first, the handler stops `take` with the stream's `return()`, since `$.process.run` has no abort. On a job the handler calls `$.model.complete` with the job's request and pipes the reply to `ways judge answer`. Neither `next(e)` nor a `$` call is charged to the handler's budget. The mod's origin filter includes `sdk`, the origin `prompt.submit` reports under `claude -p`. This settles the choice the item above left to the build and changes no rule.

6. **The Bash lane.** ADR-188 item 3c's jobs go to the deferred store as before. When the session's engine is `claude-code` the module claims and judges them, and on a key engine the adapter starts the ways agent detached. Either way the verdicts land in ADR-606's store and one collector delivers them. A judge failure on this lane delivers nothing, as ADR-188 item 3c says.

7. **No engine for the session.** `claude-code` with no judge host and no key configured, or a key engine named explicitly with no working key, leaves the session without an engine. Every job is then a judge failure, handled by each lane's own rule: the prompt lane fails open and logs `gate_fallback` with reason `engine_unavailable` (ADR-196 §7); the Bash lane delivers nothing and logs the same. The remedy is to configure one of the key paths, anthropic or openrouter: `ways agent status`, the gate screen of `ways settings` and the `gate_fallback` line all say so.

8. **Settings.** The gate section of `ways settings` lists the three profiles with their display names; choosing one writes `gate.engine` in the user layer. That choice is how the operator turns the mod on or off as the model source. The screen and `ways agent status` show each live session's resolved engine and its reason (explicit, key found, or `claude-code` unavailable), and which shipped profile an unset engine resolves to.

9. **Setup offers the engine.** `ways judge-setup`, which the installer and `ways manage update` run, offers four choices in this order:
   1. Claude Code internal: install and enable the ways mod and set `gate.engine: claude-code`; no key.
   2. An Anthropic key, as today.
   3. An OpenRouter key, as today.
   4. No judge: accepted with the degraded warning, which names the cost (every matched way is injected and the quality of ways drops), and recorded as `gate.mode: off` so the choice is visible.

   The step checks first whether the installed Claude Code can load mods. Where it cannot, it says so and offers choices 2 to 4 only, rather than offering one that would fail.

   `keys::judge_ready` gains the engine. `claude-code` with the mod installed and mods enabled is Ready; whether the mod actually loaded is a per-session fact, left to the session resolution of item 2. `claude-code` with neither the mod nor a key is degraded, and its fix line names enabling the mod or adding a key. `gate.mode: off` reads as off by choice. The installer, `ways manage update`, `ways agent status`, the help footer and the settings gate screen keep reading this one function, so they never disagree.

10. **Logging and cost.** Every judge call is logged as `judge_call` with the engine. Under `claude-code` it carries the reply's token usage and no provider price, since the spend is the account's usage. `way_judged`, `gate_fallback` and the decision record are written by the hook side as today, whichever engine judged.

## Consequences

### Positive

- A user signed in to Claude Code can run the gate with no API key, no separate bill, and no conversation text sent to a provider other than the one the session already uses.
- One setting names the judge and each session resolves it once, so a candidate is never judged twice and the session's resolution file names the spend's source.
- A session where the mod does not load, such as a `claude -p` run, still gets a judge when a key is configured.
- The prompt lane and the Bash lane share the switch, so the operator turns the mod on or off as the model source once.
- The engine sends no sampling parameter, so the API change behind #1130 does not touch it.

### Negative

- Judging spends the account's usage: in ADR-707 about 0.57 judge requests per Bash call, each about 1,160 input and 117 output tokens, plus the prompt lane's requests. On a subscription that draws on the plan's limits that the session's own work also uses.
- With `claude-code` selected and no mod loaded, the session judges on a key and spends API money the operator chose the internal engine to avoid. With no key either, the prompt lane falls back to the matcher's decision and the Bash lane delivers no meaning-based guidance until a key is configured.
- A mod that loads late in a session leaves that session on the key engine it resolved to.
- The prompt lane's judge now crosses a job file to a module in another part of the process; its latency is unmeasured, and a slow pickup fails open more often than the key path.
- The judge's instruction differs between engines, a forced tool call on the key path and strict JSON on this one, so ADR-195's and ADR-707's numbers hold for `claude-code` only once the agreement observable is recorded.
- Asked for strict JSON, Haiku through `$.model.complete` wrapped every reply of the probe in a ```json fence, so `judge::parse_reply` accepts that shape (item 4); a model that answers in another shape fails as `invalid_reply`.
- The engine depends on a preview API, and the module is one more component to ship and keep working across Claude Code builds. Mods are a preview feature, so the first setup choice works only on Claude Code versions that enable them; the setup step has to detect that rather than offer a choice that fails.
- `judge_ready` grows from a key check into an engine check, and every surface that reads it changes with it.

### Neutral

- Mods are off by default, so most installs keep the key engines; the per-session rule changes nothing for them.
- The shipped profiles are compiled into the agent (`include_str!`), so adding `claude-code` ships with a `ways-agent` release.
- `profile::valid_model_id` already admits `claude-haiku-4-5`; the `Provider` enum, its key lookup and the settings choice list gain one value each.

## Alternatives Considered

- **The mod judges whenever it is loaded, with the key as a per-job fallback for unclaimed jobs.** ADR-188's previous draft. Rejected: a slow claim and a fallback can judge one candidate twice and spend twice, and loading a mod would switch who pays without the operator choosing it. The operator asked for one switch; the key fallback this record keeps is resolved once per session, never per job.
- **`claude-code` as the default when the mod is loaded.** An unset engine would resolve once per session, to `claude-code` when the ways mod is loaded and otherwise by today's key rule, and hold for the session. Declined by the operator, who first asked for it as the default and then chose to make it selectable only. Loading a mod would otherwise change the engine and who pays without a choice in the settings.
- **No fallback: `claude-code` without a host leaves the session unjudged even where a key exists.** This record's earlier text. Replaced on the operator's answer that the fallback for an unavailable engine is one of the key paths; resolving it once per session keeps one judge per session.
- **A named `gate.fallback_engine` setting.** Lets the operator pick the fallback explicitly. Not added: the shipped key order already decides it, and a setting can be added when a user needs a different order.
- **Host the judge in the attend mod.** It is the mod agent-ways already ships. Rejected: attend carries messaging and session awareness, and the judge is the ways engine's concern; the operator asked that concerns stay separate.
- **Name the model as the alias `haiku`.** The profile would follow whichever Haiku Claude Code resolves. Not adopted: under Claude Code 2.1.308 the alias resolves to claude-haiku-5-5, which the operator judged less effective than Haiku 4.5 for this gate, and which at judge sizes ran slower, with 23 of 50 calls past 2000 ms (ADR-709).
- **`$.model.fork`.** The judge would see the session's whole transcript from the prompt cache. Rejected: in ADR-707 narration in the judge's input brought off-topic passes back, and a fork pays for the whole prefix again whenever the cache entry has lapsed.
- **`$.model.classify`.** One label per text with a fixed prompt. Rejected: the gate judges up to eight candidates in one request against an instruction of its own and needs P(yes) per candidate, which a single label does not give.
- **Make the prompt lane fail closed when the selected engine is unavailable.** Fewer unjudged injections in a run without the mod. Not adopted here: ADR-196 §7's fail-open rule covers every judge failure on the prompt lane, and changing it is a decision about the gate as a whole, not about this engine.
