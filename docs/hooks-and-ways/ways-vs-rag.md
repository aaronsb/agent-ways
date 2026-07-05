# Ways and RAG: Same Problem, Different Architecture

Ways (situated delivery of local norms — "the way we do it around here," injected at the moment of relevant action) and Retrieval-Augmented Generation (RAG) solve the same fundamental problem: getting the right knowledge into a language model's context window at the right time. The resemblance is real — both systems select relevant information and inject it before generation. But the mechanisms differ in ways that matter for how well each approach works in practice.

This document explores the relationship honestly: what's shared, what's different, and why the differences produce different outcomes.

## The Shared Problem

Language models have finite context windows. Not everything can be loaded upfront. Both RAG and Ways answer the same question:

> Given everything we *could* tell the model, what should we tell it *right now*?

RAG answers this with semantic similarity search. Ways answer it with event-driven triggers. Both are retrieval systems in the broadest sense — they retrieve relevant context and inject it. The disagreement is about *what signal to retrieve on*.

## Where They Diverge

### 1. Pull vs. Push

RAG is a **pull** architecture. The model (or an orchestrator) formulates a query, searches a corpus, and pulls back results. The retrieval is driven by what the model is currently processing — usually the user's latest message.

Ways are a **push** architecture. The environment observes events — a tool invocation, a keyword in a prompt, a state threshold — and pushes relevant context into the window. The model doesn't request the context; it arrives because something happened.

This distinction matters when the model doesn't know what it doesn't know. A developer typing "fix the auth bug" isn't asking about commit conventions, but a way triggered by the subsequent `git commit` will inject them. RAG can only retrieve what the query implies; Ways can retrieve what the *action* implies.

### 2. Stateless vs. Stateful

RAG systems are typically stateless between queries. Each retrieval is independent — the system doesn't remember what it already retrieved. This leads to a well-known problem: the same chunks get re-injected across turns, consuming context window budget with redundant information.

Ways maintain session state through marker files. Each way fires **once per session**. The state machine is simple — `not_shown → shown` — but it solves a problem that RAG handles poorly: knowing when to stop retrieving the same thing.

### 3. One Trigger Channel vs. Many

RAG retrieves on one signal: semantic similarity between the query and the corpus. This is powerful for natural language questions but blind to non-textual events.

Ways retrieve on multiple independent channels:

| Channel | Example | RAG Equivalent |
|---------|---------|----------------|
| Keyword regex | "optimize" in user prompt | Approximate (embedding similarity) |
| Tool invocation | `git commit` about to execute | None |
| File pattern | Editing `.env` | None |
| Embedding semantic | "make this query faster" | Similar (shared embedding approach) |
| State condition | Context window 75% full | None |
| Scope | Subagent spawning | None |

Three of these six channels have no RAG equivalent. You can't embed a `git commit` command into a vector space in a way that meaningfully retrieves commit conventions — the relationship is causal (this action needs this guidance), not semantic (this text is similar to that text).

### 4. Retrieval Probability Is Fixed vs. Adaptive

In a standard RAG pipeline, each chunk's retrieval probability depends only on its similarity to the current query. Prior retrievals don't change future retrieval behavior.

Ways have **progressive disclosure trees**: when a parent way fires, its children's triggering thresholds drop by 20%. The system's sensitivity adapts within a session based on what context has already been established. If the security parent way has fired, security sub-topics become easier to trigger — because the conversational domain has been established.

This is a feedback loop in the retrieval system itself. Building this on top of RAG would require a separate state-tracking orchestration layer — at which point you've left RAG's architecture and built something closer to Ways.

### 5. Documents vs. Behavioral Constraints

RAG typically retrieves *information* — facts, documentation, reference material. The retrieved content answers questions or provides context for reasoning.

Ways retrieve *behavioral constraints* — directives about how to act in a specific situation. "Use conventional commit format" isn't information to reason about; it's an instruction to follow. The content is prescriptive, not informational.

The distinction has a name in principal–agent theory: what ways resolve is **preference uncertainty**. No model ships knowing this team's norms, and an agent that lacks them has only two safe strategies — ask constantly or hedge exhaustively. RAG resolves missing facts; ways resolve missing norms.

You *could* use RAG to retrieve prescriptive content. But without trigger specificity, you'd inject commit guidance while someone is debugging CSS, because the semantic distance between "write good code" and "write good commits" is small. Ways solve this by coupling the retrieval to the specific action, not to the semantic neighborhood.

## The Taxonomy

If we step back, both RAG and Ways are instances of a broader pattern: **context-window management systems**. They sit in a design space with other approaches:

| System | Retrieval Signal | State | Trigger Channels | Content Type |
|--------|-----------------|-------|-----------------|--------------|
| RAG | Semantic similarity | Stateless | Text only | Informational |
| Ways | Events + semantics | Session-stateful | Text, tools, files, state | Prescriptive |
| Rules | File paths | Stateless | File access | Prescriptive |
| System prompt | None (always loaded) | N/A | N/A | Mixed |

Ways occupy a different point in this design space than RAG. They trade RAG's corpus scalability (RAG can search millions of documents) for trigger precision and session awareness. A RAG system with 10,000 documents would outperform Ways at knowledge retrieval. Ways with 20 behavioral constraints outperform RAG at sustained behavioral adherence.

## When RAG Would Be Better

Ways are not universally superior. RAG wins when:

- **The corpus is large.** Ways scale to dozens of way files, not thousands. If you need to search a knowledge base of API documentation, RAG is the right tool.
- **The query is open-ended.** "What does this error mean?" benefits from semantic search across a large corpus. Ways can't answer questions they don't have pre-authored content for.
- **The content is informational, not prescriptive.** If you're augmenting generation with facts rather than shaping behavior, RAG's document-retrieval model is a natural fit.

## When Ways Are Better

Ways win when:

- **Guidance must fire on actions, not just text.** Tool invocation, file editing, and state transitions have no natural representation as RAG queries.
- **Context budget is constrained.** Ways' once-per-session injection and small payload size keep the context window lean. RAG's re-retrieval pattern consumes more budget over time.
- **Behavioral adherence over long sessions matters.** The [context decay model](context-decay.md) shows that injection timing — proximity to the generation cursor — dominates content quality for sustained adherence. Ways inject at the tool-call boundary; RAG injects at the query boundary.
- **The corpus is small and curated.** Twenty well-authored behavioral constraints don't need vector search. They need precise triggering.

## The Bottom Line

RAG and Ways are both retrieval systems. They share the goal of getting relevant context into the window at the right time. The difference is architectural: RAG retrieves by similarity, Ways retrieve by event. RAG is stateless, Ways track session state. RAG searches a corpus, Ways monitor an environment.

Calling Ways "RAG with extra steps" is like calling a database "a file with extra steps" — the data ends up on disk either way, but the extra steps are what make it useful for the job. The state machine, the multi-channel triggers, the progressive disclosure, the action awareness — these aren't decorations on RAG. They're the architecture that makes behavioral adherence work in long-running agent sessions where semantic similarity alone falls short.
