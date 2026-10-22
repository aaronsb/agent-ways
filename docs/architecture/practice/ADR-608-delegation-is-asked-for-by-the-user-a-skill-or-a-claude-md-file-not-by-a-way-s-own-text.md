---
contract: adr/v1
kind: decision
verb: constrain
capability:
  - method
  - loop
amends:
  - ADR-175#Decision
  - ADR-175#Alternatives Considered
basis:
  - evidence: 'Claude Code 2.1.306 binary (build 2026-10-21, git 01ae8327), read with strings on 2026-10-22: the main prompt carries "Do not use the [Agent] tool, workflows, or deep-research unless the user, a CLAUDE.md file, or a skill asks for it" under <additional_guidance> (opus5_reduced_delegation). It is emitted when the model has opus_5_prompt_bundle, flag tengu_slate_bittern is on (default true), and server-served heron_brook text does not already carry it. The 2.1.219 sentence "Do not call the AgentTool unless the user requested it" is no longer in the binary; only its first words remain, as a prefix that suppresses the new sentence when served text contains it.'
  - evidence: 'Claude Code 2.1.306 model capability table, same read: claude-opus-5 has opus_5_prompt_bundle; claude-opus-5-5 has opus_5_5_prompt_bundle and not opus_5_prompt_bundle; claude-fable-5-1 has fable_5_1_prompt_bundle and fable_5_mitigations. By default the sentence reaches claude-opus-5 only; another model receives it only when the server serves the capability.'
  - evidence: 'Claude Code 2.1.306, same read: the cost-side delegation texts are gated apart from the bundle. "Subagents multiply cost and time ... Delegate only when the payoff clearly exceeds that overhead" needs steer counter_steer; "A fresh agent costs more than it looks ... When in doubt, don''t spawn" needs flag tengu_lucky_cerf. Either can reach a model that lacks the permission sentence.'
  - evidence: 'Corpus survey, 2026-10-22: meta/subagents quotes the 2.1.219 sentence and states that invoking "a skill or way" whose steps call for delegation is the user''s request; research says "asking for the research is asking for the sweep"; research/sweep, documentation/validate, evaluative/held-out, evaluative/ablation/rounds, meta/introspection and environment/bounded-execution/watching direct a spawn with no clause naming who asked for it.'
  - precedent: ADR-175
  - precedent: ADR-128
  - precedent: ADR-142
agent:
  name: Claude
  model: claude-opus-5-5
considered:
  - operator: aaronsb
    said: "(a) Delegation ways, (b) Fresh reader. I assume with delegation ways, you mean: delegate to subagents without asking. My hope is that I do not have to remind you on each session to do so. As far as I know this is contrarian to the claude code system prompt and irritating to me - not delegating results in session churn."
    via: AskUserQuestion answer, session 2026-10-22
status: accepted
date: 2026-10-22
deciders:
  - aaronsb
related:
  - ADR-175
  - ADR-128
  - ADR-142
  - ADR-609
observable:
  - "run: strings on the installed Claude Code binary, grep for 'unless the user, a CLAUDE.md file, or a skill asks for it'; meta/subagents quotes that sentence verbatim and names claude-opus-5 as the model it reaches by default"
  - "run: rg -n \"skill or way whose steps|is asking for the sweep\" hooks/ways returns nothing; no way presents its own text as the user's request"
  - "see: on claude-opus-5 with no CLAUDE.md line, a plain research question that fires the research way's fan-out step gets one line naming the fan-out and its width, and no Agent call until the user answers"
  - "see: on claude-opus-5, /merge dispatches code-reviewer without asking, and the session names the payoff in a clause"
  - "see: asked to check a README with a cold read, the session spawns a fresh reader without asking and says in a clause that the reader must not have seen the author's reasoning"
---

# ADR-608: Delegation is asked for by the user, a skill, or a CLAUDE.md file, not by a way's own text

## Summary

- **Decided:** ADR-175 is re-based on the Claude Code 2.1.306 sentence "Do not use the Agent tool, workflows, or deep-research unless the user, a CLAUDE.md file, or a skill asks for it". By default that sentence reaches claude-opus-5 only; Opus 5.5 and Fable 5.1 do not receive it. A spawn is asked for when one of the three sources the sentence names asks for it: the user in the conversation, a skill in use, or a CLAUDE.md file. A way's own text no longer counts as the user's request. The operator may place one line in their CLAUDE.md that asks for the delegation the corpus's skills and ways direct. agent-ways documents that line and never writes it. A spawn whose payoff is independence from the author, such as a cold read or a held-out case writer, counts as asked for when the user asks for that check, because the check has no inline substitute.
- **Trades away:** On a model that receives the sentence and with no CLAUDE.md line, a way that fires on its own proposes its spawn in one line and waits, where it now spawns. That costs one exchange each time. ADR-175's reading that a way's step is the user's request is withdrawn.
- **One-way?** No. Reversible: the change is to way text and to a line in a file the operator owns, and both revert in one session.
- **Probes:** *Confident (standing):* When a session runs on Opus 5, should the delegation the ways call for keep happening without asking you? For that it needs one line in your CLAUDE.md. Yes or no. *Not confident (cold-read):* When you ask me to check my own work, for example a cold read of docs, held-out tests or a refutation pass, should I spawn the fresh reader without asking first? Yes or no.
- **Inversion:** At one end, ways grant delegation by their own text. At the other, delegation happens only when the user names it each time. This decision takes neither end. The grant comes from the operator's own words, said once in CLAUDE.md or each time in the conversation, and from skills, which the harness names. Ways carry the step and its payoff.

## Context

ADR-175 answered the 2.1.219 system-prompt sentence "Do not call the AgentTool unless the user requested it". It treated that sentence as a permission gate and satisfied the gate in two tiers. Tier 1 was doctrine in `meta/subagents`, stating that "invoking a skill or way whose steps call for delegation is the user's request". Tier 2 was a clause at each delegation site. ADR-175 rejected putting the authorisation in the user's CLAUDE.md. Its reasons were that CLAUDE.md has the same authorship as way text, so it would win only on recency, and that it applies to every project rather than to the procedures that specify delegation.

Claude Code 2.1.306 replaced that sentence. The new text names three sources that can ask for a spawn: the user, a CLAUDE.md file, and a skill. Hook output, which is how a way reaches the model, is not among them. The binary also gates the sentence more narrowly. It is emitted for models carrying `opus_5_prompt_bundle`, which by default is claude-opus-5 alone. The old sentence survives only as a detection prefix for text the server may still serve. Cost-side texts that ask whether a spawn's payoff exceeds its overhead are gated separately and can reach any model.

Two of ADR-175's premises have changed. A way that calls its own text the user's request now claims a source the harness does not list, which is the override pattern ADR-175 declined. CLAUDE.md is now a named source, so the recency objection to the rejected alternative no longer applies.

A third question was open under the old text and is sharper under the new one. Some spawns exist for efficiency: reading many sources in parallel, or keeping log output out of the main context. Others exist for independence: a reader who has not seen the author's reasoning, a case writer who never saw the code, a refuter who did not find the claim, a blind judge. The second kind cannot be done inline. The author cannot stop knowing what it knows.

## Decision

1. **The gate's text and reach.** The permission text this corpus answers is the 2.1.306 sentence above. By default it reaches claude-opus-5 only. A server can serve it to other models, and a server can still serve the older sentence, which names only the user. Ways do not assume which model they run on. A clause about who asked for a spawn applies where the model's own instructions restrict the Agent tool to what someone asked for. Where no such text is present, the payoff question from ADR-175 is the check that remains.
2. **Three sources ask for a spawn.** These are the three the sentence names.
   - The user, in the conversation: for the spawn itself, or for a check whose method is the spawn (item 4).
   - A skill in use: one the user invoked, or one loaded to serve what the user asked. A skill whose step directs delegation asks for it at that step. Skills that carry ADR-175's tier-2 clause (`/start`, `/develop`, `/merge`, and any later one) meet the text by name. Tier 2 now rests on skills.
   - A CLAUDE.md file. The operator may place a standing line in their own CLAUDE.md or in a project's. `meta/subagents` documents a suggested line. The install never writes it, because `~/.claude` stays the user's own directory (ADR-142). The suggested line is: *"Delegation that an agent-ways skill or way directs at its own step counts as asked for. Spawn what the step names, at the width it states, and ask past about six agents."* That line keeps ADR-175's width bound, and it leaves workflows and deep-research out.
3. **A way's text is not a request.** A way's delegation step names the spawn and its payoff. When none of the three sources covers the step and the model's instructions carry the restriction, the way proposes the spawn in one line with its width. It then waits for an answer, or does the work inline where that is possible. With the CLAUDE.md line in place, the step is covered and runs as written.
4. **Independence spawns.** When the user asks for a check whose payoff is independence from the author, that request asks for the independent reader. Examples are a cold read of documentation, held-out cases, refutation of key claims, a blind judge, and a fresh-context review of the author's own change. The spawn proceeds, and the session names the payoff in a clause. When the way that describes such a check discloses during other work, for example on an edit to a README, the user has not asked for the check, so the way proposes it. Where an instrument that spawns nothing can do the job, such as a stop-hook script, the way leads with that instrument. Efficiency spawns have an inline substitute. Fan-out reads, heavy log reading, parallel edits and an artifact review all count, and each needs one of the three sources.
5. **Workflows and deep-research.** The new sentence admits the same three sources for these. ADR-175's scope rule stands as the operator's preference: they are proposed and discussed, never invoked unprompted. The suggested CLAUDE.md line does not cover them. Only a line that names them does.
6. **What ADR-175 keeps.** It keeps the two-tier structure and the payoff clause. It keeps the width bound and the conditions under which to still ask: not part of an invoked procedure, significant tokens outside the task, or the operator has said to work solo. It also keeps the corollary that two ways must not instruct opposite verbs at one moment. This record amends ADR-175's Decision where it reads "a skill or way", and its Alternatives Considered entry on CLAUDE.md, which this record adopts in a scoped form placed by the operator.

### Way edits this implies

The ways are not edited by this record. These are the edits it implies:

- `meta/subagents/subagents.md` (Invocation Is Authorization, line 100 onward): quote the 2.1.306 sentence, state that by default it reaches claude-opus-5, and note that served text may still carry the older sentence. List the three sources and replace "a skill or way" with "a skill". Add the suggested CLAUDE.md line and the independence rule. Say that where no restriction text is present, the payoff section is the check.
- `research/research.md:19`: replace "asking for the research is asking for the sweep" with this rule. Fan out when the user asked for breadth, a skill in use directs it, or a CLAUDE.md line covers it. Otherwise, where the restriction applies, name the fan-out and its width in one line and wait.
- `research/sweep/sweep.md:35`: the refuter is an independence spawn. It is asked for when the user asked for the sweep's claims to be checked; otherwise the way proposes it. The sweep's scouts follow `research`.
- `documentation/validate/validate.md:15`: add the clause. When the user asked for the docs to be validated, the fresh reader is asked for, and the payoff at line 11 is the clause to name. When the way fired on an edit, propose the cold read.
- `evaluative/held-out/held-out.md:20`: the same clause for the case writer.
- `evaluative/ablation/rounds/rounds.md:37`: lead with the stop-hook script, which spawns nothing. A fresh-context reviewer is an independence spawn, asked for when the user asked for the loop.
- `meta/introspection/introspection.md:44`: Part 2's spawn is an efficiency spawn. It runs when the user asked for the review or a CLAUDE.md line covers it. Otherwise do Part 2 inline or propose it, and name the payoff in a clause.
- `softwaredev/environment/bounded-execution/watching/watching.md:12`: grep the log inline. Hand heavy log reading to a subagent when one of the three sources asks for it, and otherwise propose it.

## Consequences

### Positive

- The corpus meets the harness text on its own terms again, with no claim that hook output is something the harness does not list.
- One line, written once by the operator, restores ADR-175's behaviour on claude-opus-5 in full. Without it the failure is closed: a way proposes, which is the behaviour from before ADR-175.
- Skills, which the harness names, now carry the authorisation load. The merge, develop and start loops are unaffected.
- The independence rule settles the cold-read and held-out cases by their payoff rather than by which way names them, and the payoff is stated in each of those ways already.

### Negative

- On claude-opus-5 without the CLAUDE.md line, research fan-outs, log reading and the introspection review each cost an extra exchange.
- Ways now carry a conditional clause, "where your instructions restrict the Agent tool". It is longer than the old clause, and a model has to read its own prompt to apply it.
- The reading in item 4, that asking for a check is asking for its only method, is the agent's interpretation of "asks for it". The harness text does not state it.
- Nothing enforces that a new delegating way carries the clause. The failure stays closed, as under ADR-175.

### Neutral

- On claude-opus-5-5 and claude-fable-5-1 the permission sentence is absent by default. There, the payoff clause and any cost-side text are the operative checks, and this record changes nothing about when they spawn.
- If a later release names hook output, or drops the sentence, item 3 can be relaxed in one edit to `meta/subagents` and the eight sites.

## Alternatives Considered

- **Keep claiming that a way's text is the user's request.** Rejected. ADR-175 argued that user-authored way text and user-authored CLAUDE.md carry the same authority. The 2.1.306 text enumerates its sources, and hook output is not one of them. Claiming otherwise answers the gate by asserting a source it does not list, which is the override ADR-175 rejected. The authorship argument is also weaker than it was, because ways are corpus text shipped to every install, not text the user wrote.
- **Keep CLAUDE.md rejected, as in ADR-175.** Rejected. Both of ADR-175's grounds have moved. The recency ground is gone, because CLAUDE.md is now a named source that meets the gate on its terms. The every-project ground is answered by scope: the line covers only steps that agent-ways skills and ways direct, and those ways reach every project anyway because the install projects them globally. A project that wants no delegation can say "work solo" in its own CLAUDE.md, which ADR-175 already honours.
- **Have the install write the CLAUDE.md line.** Rejected. A line the installer writes is the corpus authorising itself, the same objection as the first alternative. It would also write into a file under `~/.claude` that the install leaves to the user (ADR-142).
- **Convert the delegating ways into skills.** Not adopted wholesale. A skill is a named source, so a procedure that runs as a skill needs no further clause. But a way fires on its trigger, while a skill needs invoking. The research and validation guidance is behaviour the corpus wants disclosed during other work, not only when someone runs a procedure. A site whose content is a runnable procedure, such as the research sweep, may still move to a skill in its own change.
- **Treat every spawn the same, with no independence rule.** Rejected. A cold read or a held-out writer has no inline substitute. Making them wait on a separate request for delegation after the user asked for the check would mean asking twice for one thing.
- **Do nothing until the operator's model changes.** Rejected. The operator's main model does not receive the sentence by default, but claude-opus-5 sessions do. `meta/subagents` quotes a sentence that is no longer in the binary, and a quote that is wrong in a way misleads every model that reads it.

## Correction note (2026-10-22)

The "Way edits this implies" texts for `watching.md`, `introspection.md` and `bounded-execution` left out the condition from item 1: an efficiency spawn needs one of the three sources only where the model's instructions restrict the Agent tool. The way edits apply that condition. The decided text above is unchanged.
