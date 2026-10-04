<!-- Site navigation, read by mkdocs-literate-nav. A link to a directory
     (ending in /) lists that directory's pages automatically.
     Tabs follow the system: the two applications (ways, attend) with their
     sub-apps, the corpus of ways they serve, and the project itself. -->
* Home
    * [Overview](README.md)
    * [The cognitive loop](cognitive-loop.md)
    * [Install](install-guide.md)
    * Prerequisites
        * [macOS](prerequisites-macos.md)
        * [Arch](prerequisites-arch.md)
        * [Debian](prerequisites-debian.md)
        * [Fedora](prerequisites-fedora.md)
    * [Vocabulary](vocabulary.md)
    * [Status line](reference/statusline.md)
* Ways
    * [Start here](hooks-and-ways/README.md)
    * [How ways works](explanation/how-ways-works/)
    * Hook engine
        * [Hooks and ways system](hooks-and-ways.md)
        * [Architecture diagrams](architecture.md)
        * [Matching and routing](hooks-and-ways/matching.md)
        * [Matching engine reference](hooks-and-ways/engine-reference.md)
        * [Context decay](hooks-and-ways/context-decay.md)
        * [Context decay: formal foundations](hooks-and-ways/context-decay-formal-foundations.md)
        * [Model context decay](reference/model-context-decay/README.md)
        * [Design rationale](hooks-and-ways/rationale.md)
        * [Ways and RAG](hooks-and-ways/ways-vs-rag.md)
        * [Observed behavior](hooks-and-ways/observed-behavior.md)
    * ways CLI
        * [Reference](reference/ways-cli.md)
        * [Renamed commands](reference/ways-cli-renames.md)
        * [The event log](reference/events.md)
        * [Stats and observability](hooks-and-ways/stats.md)
    * way-embed
        * [Finish the install](finish-install.md)
        * [Fix a SIGKILL on macOS](how-to/fix-way-embed-sigkill-macos.md)
    * ways-agent (relevance judge)
        * [The relevance judge](explanation/relevance-judge/)
        * [Yes/no gate research](research/yesno-relevance-gate/README.md)
    * ways-audit (governance)
        * [Governance and traceability](governance.md)
        * [Adding a compliance claim](hooks-and-ways/provenance.md)
    * [ways-mcp](architecture/platform/ADR-501-the-agent-ways-mcp-server-one-server-for-attend-keepalive-and-later-modules-inbound-through-channels.md)
    * Localization
        * [Multi-language support](hooks-and-ways/languages.md)
        * [Adopter localization](explanation/localization/)
    * [Teams](hooks-and-ways/teams.md)
* Attend
    * [Overview](attend-and-monitor/README.md)
    * The loop
        * [The attend loop](attend-and-monitor/loop.md)
        * [Engagement](attend-and-monitor/engagement.md)
        * [Signals](attend-and-monitor/signals.md)
        * [Configuration](attend-and-monitor/configuration.md)
    * Sensors
        * [Built-in sensors](attend-and-monitor/sensors.md)
        * [Your first sensor](attend-and-monitor/first-sensor.md)
        * [Authoring sensors](attend-and-monitor/authoring-sensors.md)
        * [Keepwarm](attend-and-monitor/keepwarm.md)
    * Messaging
        * [Delivery](attend-and-monitor/delivery.md)
        * [Channels](attend-and-monitor/channels.md)
        * [Messaging explained](explanation/attend-messaging/)
    * [attend-chat](attend-and-monitor/tui.md)
    * [CLI reference](cli/attend.md)
* Corpus
    * [Browse the ways](ways/)
    * Authoring ways
        * [Extending the system](hooks-and-ways/extending.md)
        * [Macros](hooks-and-ways/macros.md)
        * [Scoring and testing](hooks-and-ways/scoring-and-testing.md)
        * [Docs about the matching engine](hooks-and-ways/authoring-docs-style.md)
    * Domain guides
        * [Meta ways](hooks-and-ways/meta.md)
        * [IT operations ways](hooks-and-ways/itops.md)
* Project
    * [Development](development.md)
    * [Decisions](architecture/)
