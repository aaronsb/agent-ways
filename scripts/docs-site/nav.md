<!-- Site navigation, read by mkdocs-literate-nav. A link to a directory
     (ending in /) lists that directory's pages automatically; ways/ carries
     its own generated SUMMARY.md.
     Tabs follow the system: the two applications (ways, attend) with their
     sub-apps, labelled "Description (binary)"; the corpus of ways they serve;
     and the project itself.
     A README.md becomes the landing page of the section it is listed in, so
     a README that is not a section's landing gets a section of its own. -->
* Home
    * [What is agent-ways](index.md)
    * Get started
        * Prerequisites
            * [macOS](prerequisites-macos.md)
            * [Arch](prerequisites-arch.md)
            * [Debian](prerequisites-debian.md)
            * [Fedora](prerequisites-fedora.md)
        * [Install](install-guide.md)
        * [Finish the install](finish-install.md)
    * [The cognitive loop](cognitive-loop.md)
    * [Vocabulary](vocabulary.md)
    * [Status line](reference/statusline.md)
* Ways
    * [Start here](hooks-and-ways/README.md)
    * How ways works
        * [The model](explanation/how-ways-works/how-ways-works-the-model.md)
        * [A long session, read through ways](explanation/how-ways-works/scenario-a-long-session.md)
        * [Reading the session data yourself](explanation/how-ways-works/reading-the-session-data.md)
    * Hook engine
        * [Hooks and ways system](hooks-and-ways.md)
        * [Matching and routing](hooks-and-ways/matching.md)
        * [Matching engine reference](hooks-and-ways/engine-reference.md)
        * [Architecture diagrams](architecture.md)
        * [Subagents and teams](hooks-and-ways/teams.md)
    * Command line (ways)
        * [ways reference](reference/ways-cli.md)
        * [Stats and observability](hooks-and-ways/stats.md)
        * [The event log](reference/events.md)
        * [Renamed commands](reference/ways-cli-renames.md)
    * Embeddings (way-embed)
        * [Fix a SIGKILL on macOS](how-to/fix-way-embed-sigkill-macos.md)
    * Relevance judge (ways-agent)
        * [The model](explanation/relevance-judge/relevance-judge-the-model.md)
        * [What it sends and costs](explanation/relevance-judge/what-the-judge-sends-and-costs.md)
        * [Watching and tuning it](explanation/relevance-judge/watching-and-tuning-the-judge.md)
        * Yes/no gate research
            * [Yes/no gate research](research/yesno-relevance-gate/README.md)
    * Governance (ways-audit)
        * [Governance and traceability](governance.md)
        * [Adding a compliance claim](hooks-and-ways/provenance.md)
    * [MCP server (ways-mcp)](architecture/platform/ADR-501-the-agent-ways-mcp-server-one-server-for-attend-keepalive-and-later-modules-inbound-through-channels.md)
    * Localization
        * [The model](explanation/localization/adopter-localization-the-model.md)
        * [Scenario: the English-native install](explanation/localization/scenario-the-english-native-install.md)
        * [Scenario: the language switch](explanation/localization/scenario-the-language-switch.md)
        * [Scenario: steady-state authoring](explanation/localization/scenario-steady-state-authoring.md)
        * [The mode gate](explanation/localization/the-mode-gate-mechanism-under-the-scenarios.md)
        * [Multi-language reference](hooks-and-ways/languages.md)
    * Design and evidence
        * [Design rationale](hooks-and-ways/rationale.md)
        * [Ways and RAG](hooks-and-ways/ways-vs-rag.md)
        * [Context decay](hooks-and-ways/context-decay.md)
        * [Context decay: formal foundations](hooks-and-ways/context-decay-formal-foundations.md)
        * [Observed behavior](hooks-and-ways/observed-behavior.md)
        * Model context decay
            * [Model context decay](reference/model-context-decay/README.md)
* Attend
    * [Overview](attend-and-monitor/README.md)
    * The loop
        * [The attend loop](attend-and-monitor/loop.md)
        * [Engagement](attend-and-monitor/engagement.md)
    * Sensors
        * [Built-in sensors](attend-and-monitor/sensors.md)
        * [Your first sensor](attend-and-monitor/first-sensor.md)
        * [Authoring sensors](attend-and-monitor/authoring-sensors.md)
        * [Keepwarm](attend-and-monitor/keepwarm.md)
    * Messaging
        * [Messaging explained](explanation/attend-messaging/)
        * [Delivery](attend-and-monitor/delivery.md)
        * [Channels](attend-and-monitor/channels.md)
    * [Chat (attend-chat)](attend-and-monitor/tui.md)
    * Reference
        * [Signals](attend-and-monitor/signals.md)
        * [Configuration](attend-and-monitor/configuration.md)
        * [Command line (attend)](cli/attend.md)
* Corpus
    * [Browse the ways](ways/)
    * Authoring ways
        * [Writing a way](hooks-and-ways/extending.md)
        * [Macros](hooks-and-ways/macros.md)
        * [Scoring and testing](hooks-and-ways/scoring-and-testing.md)
    * Domain overviews
        * [Meta](hooks-and-ways/meta.md)
        * [IT operations](hooks-and-ways/itops.md)
* Project
    * [Development](development.md)
    * [Writing about the matching engine](hooks-and-ways/authoring-docs-style.md)
    * Decisions
        * [Index](architecture/INDEX.md)
        * [Ways](architecture/ways/)
        * [Attend](architecture/attend/)
        * [Platform](architecture/platform/)
        * [Practice](architecture/practice/)
        * [Governance](architecture/governance/)
        * [Documentation](architecture/documentation/)
        * [Archive](architecture/archive/)
