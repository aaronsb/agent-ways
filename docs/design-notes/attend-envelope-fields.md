# Attend envelope fields: sender kind, principal, and addressee on every signal

A reading of the attend signal wire format, taken 2026-09-19, that settles the structured envelope fields issues #532 (sender kind and on-behalf-of) and #533 (addressee) ask for. The two issues share one on-disk change and one set of readers, so this note specifies them together for a single implementation. It then states what #535 (age- and party-aware drain) reads from the fields and which parts of ADR-172 that amends. Signing and cross-machine relay stay out of scope; the last section says why these fields are still their prerequisite. #538 (attend as an MCP server for outbound) is covered where the field design touches tool parameters.

The ADR that #532's `adr:needed` label calls for can cite this note for its context and lift the field table into its decision section.

## What the wire carries today

A signal is one line in a `.signal` file, written by `cmd_send` in `tools/attend/src/cmd/send.rs` and mirrored by `write_signal` in `tools/attend-chat/src/signal.rs`:

```
from|project|cwd|message
from|project|cwd|re:<signal-id>|message
```

`from` is `claude:<session-id>` or `external:<user>@<terminal>`. The prefix names the implementation that wrote the file. The `re:` field is a tagged optional field: the parser in `tools/attend/src/cmd/inbox.rs` (`parse_signal`) consumes it only when the value matches the signal-id character class, so prose that begins with `re:` falls through as message text. Three parsers exist for this line (`attend/inbox.rs`, `sensor-peers/lib.rs`, `attend-chat/signal.rs`) and two writers; they are kept in lockstep by comments (ADR-136, Neutral consequences). The first two share the `re:` fence. attend-chat's `parse_file` has none: it splits into at most five fields and tests `re:` at a fixed index, so on a tagged record it would render the tags as body and drop the threading. It needs a rewrite to the tag-consuming parse below, and the checklist names it.

The receiver learns the sender's kind by reading the rendered name. The receiver learns the addressee by knowing which directory the file sat in, which the drain and the sensor know and the transcript does not. The 2026-09-19 evidence in #532 and #533 follows from those two gaps: an approval rule that had to string-match `bockeliea@ssh`, an `/insights` run that scored eight coordinating sessions as one operator each, and a fresh session that read ten messages of another pair's traffic to confirm none of it was for it.

## The fields

Four fields, all written by the sending attend process from state it resolves itself. The model supplies the message body and, through flags, the addressee and the optional principal. The model never supplies its own kind or id.

| Field | Type | Values | Written by | Source |
|---|---|---|---|---|
| `from_kind` | enum | `human`, `agent` | sender process | ADR-171 identity resolution (rule below) |
| `from_id` | string | canonical sender id | sender process | session id for `agent`; sanitized username (ADR-170) for `human`; key fingerprint once signing lands |
| `on_behalf_of` | string, optional | canonical id of the principal whose request the message carries | sender process from `--on-behalf-of <id>` | asserted by the sender, unverified |
| `to` | string | `*`, `#<channel>`, or a comma-separated set of canonical ids | sender process from routing flags or from the replied message | resolved at send time |

`from_id` and `to` share one id space: session ids for agents, sanitized usernames for humans. Display names and Greek ordinals never appear in these fields (ADR-171: ordinals are presentation, never keys).

Two value classes apply. `re` keeps the signal-id class `[A-Za-z0-9_-]+`: `is_valid_signal_id` and `sanitize_id_component` in `agent-identity/src/identity.rs` hold that class in lockstep, and `cmd_send --re` exits 1 on anything outside it. `from_id`, `on_behalf_of`, and `to` take the wider class `[A-Za-z0-9_@.,:/#*-]+`. Both classes exclude `|`, whitespace, and control characters, so the tagged-field parse cannot be broken by a value. Channel names need a tightening to fit: `validate_group_name` in `attend-groups/src/lib.rs` block-lists only `/`, space, `#`, `:`, and a leading `_` or `@`, so `dev|ops`, `équipe`, and names containing `,` are legal today and would split the wire or collide with the id-set separator. The checklist tightens `validate_group_name` to `[A-Za-z0-9_.-]+`. A channel created before the tightening whose name falls outside that class keeps working: the writer omits the `to` tag for it and readers derive `to` from the directory, as they do for legacy records.

### How from_kind is decided

`attend_session::identity()` (ADR-171) climbs the process ancestry to a Claude Code session record. The rule is:

- `session_resolved == true`: the process runs under a Claude session. `from_kind = agent`, `from_id = session_id`. A subagent's Bash tool resolves to the supervising session, so its sends carry the supervisor's id (ADR-171: subagents have no roster identity).
- `session_resolved == false`: no Claude session owns the process. `from_kind = human`, `from_id = sanitize_id_component($USER)`, the same member id ADR-170 uses for channel membership.

attend-chat is an exception by construction. Its `identify_sender` (`attend-chat/src/signal.rs`) hardcodes the human path and never consults the session tuple, so it writes `human` even when `attend chat` was launched from inside a session and the TUI is a child of the Claude process (`attend chat` execs the `attend-chat` binary). The exception is deliberate: the TUI is the operator's surface, and a person is at its keyboard.

The rule keys on `session_resolved` alone and ignores `origin_resolved`. A session record without a `cwd` is still a session, and the partial case must not turn an agent into a human.

The rule sees process lineage and cannot see the keyboard. A command the operator runs through the harness's `!` shell escape is a child of the Claude process and resolves as `agent`. This limit belongs to any mechanical rule, and the note records it instead of adding a heuristic. The operator's channels for `human`-kind sends are attend-chat and a plain terminal.

### on_behalf_of

An agent relaying its operator's request writes `--on-behalf-of <username>`. An agent relaying another agent's request writes that agent's session id. The field is a claim the sender makes and attend does not verify it; verification is what signing adds later. The shape follows the RFC 5322 `Sender` and `From` split and the RFC 8693 `act` claim: the actor is `from_id`, the principal is `on_behalf_of`. The bus stays flat. A receiver applying an approval rule checks `from_kind` and `on_behalf_of` mechanically and decides for itself what it accepts; the fields are evidence for the receiver to weigh and carry no rank.

### to

`to` names who the message is for. The directory the file lands in is how it gets there. The two agree for broadcast and channels and can differ for directed messages and replies:

- `attend send <msg>` (default, and `--broadcast`): `to = *`. File lands in `_broadcast/`.
- `attend send --channel <name>`: `to = #<name>`. File lands in `@<name>/`.
- `attend send --to <path>`: `to` = the session ids of every live peer at that path, comma-separated. `cmd_send` already resolves the path against the live roster and errors when nobody is there, so the set is never empty. Two sessions at one origin (alpha and beta, ADR-129) share a tray and both appear in `to`. File lands in the tray for that path.
- attend-chat's `@Nickname` directed send, and the `/invite` and `/kick` notices (`attend-chat/src/app/keys.rs`), write into the peer's tray through `cwd_dir(&peer.root)`. Their `to` is the session ids at that root, the same value `--to <path>` produces. Writing the channel or `*` there would make #535's party rule collapse a human's directed message as traffic between others.
- `attend reply <msg>`: `to` = the `from_id` of the replied message. If that message carried `on_behalf_of`, the reply still addresses the sender, since the sender is the party in the exchange. Delivery is unchanged: a reply rides whatever directory the routing flags select, which is `_broadcast/` by default. `attend reply` today records only the parent's signal id (`last_inbound`); the builder resolves the parent's `from_id` at reply time by looking the id up in the scan directories, the same lookup `cmd_inbox_read` performs. When the parent cannot be found, the reply already degrades to unthreaded, and `to` falls back to the routing default.

`cmd_send` builds one destination directory today and `--to` takes one path; attend-chat's `@a @b` fan-out (ADR-136 Decision 4) is the only multi-destination writer. The rule for fan-out, applied by attend-chat now and by the CLI when `--to` grows a repeatable form, is that each file carries the full addressee set, as RFC 5322 `To` does, so any recipient can see who else received it.

## On-disk form

The three positional fields stay. Tagged fields follow them in a fixed order, and `re:` keeps its place as the last tag so the existing tail parse is undisturbed:

```
from|project|cwd|from_kind:agent|from_id:<sid>|to:*|message
from|project|cwd|from_kind:human|from_id:aaron|to:<sid>|re:<signal-id>|message
from|project|cwd|from_kind:agent|from_id:<sid>|on_behalf_of:aaron|to:<sid1>,<sid2>|message
```

A reader consumes `key:value|` segments after `cwd` while the key is one of the five known tags (`from_kind`, `from_id`, `on_behalf_of`, `to`, `re`) and the value matches that tag's class (the signal-id class for `re`, the wider class for the other three). The first segment that fails either test begins the message. Readers accept the known tags in any order; writers emit the fixed order above. The fence is the one `re:` already uses, extended to four more keys, and it keeps the message able to begin with any prose other than an exact known tag.

The `from` field is kept unchanged. `identity_view.rs` renders the sender from its `claude:` and `external:` prefix, the instance registry lookup keys on the session id inside it, and the own-message skip compares against it. `from_kind` and `from_id` are derived from the same identity resolution in the same process, so the two cannot disagree except by hand edit. Readers prefer the tagged fields when present.

### Reading a signal without the fields

Every existing signal on disk lacks the tags, and the ledger is never reaped by age (ADR-136 Decision 3), so readers carry the derivation indefinitely:

- `from_kind`: `claude:` prefix means `agent`; `external:` prefix means `human`; any other prefix means unknown, rendered as the raw `from` as today.
- `from_id`: the suffix after the prefix; for `external:`, the part before `@`, sanitized, which matches the ADR-170 member id.
- `on_behalf_of`: absent.
- `to`: from the directory. `_broadcast/` is `*`; `@<name>/` is `#<name>`; a tray directory is "this tray", which a session at that origin treats as addressed to itself. That is today's behavior for legacy directed messages and it stays.

### Schema version stamp

Not warranted for this change. The presence of the tags is the version, unknown tags are rejected into message text by construction, and all readers and writers on one machine ship from one workspace build with self-reload (`maybe_self_reload` in `cmd/run/tick.rs`) closing the gap for long-running sensors. The condition that reverses this is two writers of different versions sharing one bus, which is the cross-machine relay case. When that arrives, `v:` joins the known tag set with the same fence and readers that see a version above their own treat the rest of the tag run as opaque. Adding it now would be a field nothing reads.

### One parser

This change touches all three parsers and both writers. The recommendation is to move the line format into one shared crate (the natural home is `agent-identity`, which already owns `signal_filename` and `sanitize_id_component`, or a small `attend-wire` sibling) and have `attend`, `sensor-peers`, and `attend-chat` call it. #538 asks for CLI and MCP as two frontends over one library; the wire format is the first piece of that library. The alternative, extending the lockstep comments to five sites, is what the codebase does today and it has held, so a builder under time pressure can keep it. The cost is that the next tag repeats this five-site edit.

## Rendering on both conduits

Attend's output format is bracketed key-value, chosen because Monitor entity-escapes angle brackets (design note: cognitive loop and awareness layer). Both conduits carry one machine-readable header per message in that format, with the same keys, so a transcript consumer parses one shape wherever it finds it:

```
[attend message from_kind=agent from=<from_id> to=* id=<signal-id>]
[attend message from_kind=human from=aaron to=<sid> re=<signal-id> id=<signal-id>]
[attend message from_kind=agent from=<from_id> on_behalf_of=aaron to=<sid1>,<sid2> id=<signal-id>]
```

`on_behalf_of` and `re` appear only when set. Ids in `to` are rendered as ids; the human-readable line beside the header carries display names.

Monitor line (`sensor-peers`, `read_signals`). Today: `message from <sender>: <body>`. After: `message from <sender> (<from_kind>, to <addressee>): <body>`, where `<addressee>` is `you` when this session's id is in `to`, `#open` for `*`, `#<name>` for a channel, and the resolved display names otherwise. The bracketed header rides as its own observation line ahead of the message so the ~400-character line ceiling and the chunking logic are untouched. The digest (`build_digest`) already partitions "to you" and "on #open" by directory; with `to` it partitions by addressee, which fixes the shared-tray case where alpha counts beta's directed mail as its own. The per-message header does not survive the digest path, since `build_digest` emits one line for the whole poll. In its place the digest emits one header with counts, `[attend digest count=12 to_you=3 open=9 other=0 human=2 agent=10]`, and the per-message headers are available from `attend inbox`. A transcript consumer sees either N message headers or one digest header per Monitor wake.

Drain (`cmd_inbox_drain`, `render_drain_reason`). Today each message renders as `[when] sender (scope, id X):` and a body. After, the header line precedes that line for each message. The block header `[attend] N peer message(s) delivered at the turn boundary` stays, and `ways scan` already classifies text beginning `[attend` as a harness envelope (`is_system_envelope`), so the change does not cause the drain's text to be matched as operator intent.

`attend inbox` and `attend inbox --read` gain `Kind`, `To`, and `For` (on-behalf-of) in the table and in the piped block form, rendered from the fields with the legacy derivation for old signals.

### The transcript origin object

The harness stamps `origin.kind` on rows it creates: `human` for the operator's typed prompt, `task-notification` for a Monitor wake, `peer` for a teammate message. The Stop hook returns `{"decision": "block", "reason": ...}` and the harness records the reason as a user-role row with no `origin`. Attend cannot stamp `origin` on that row today.

The mapping this note fixes, for a transcript consumer that lifts the bracketed header into `origin` and for the harness if it ever accepts origin metadata from a hook:

| Attend-delivered row | `origin.kind` | Carried beneath |
|---|---|---|
| Drain injection, any `from_kind` | `peer` | `origin.attend = {from_kind, from_id, on_behalf_of, to, id, re}` |
| Monitor wake (harness already stamps) | `task-notification` | same object, parsed from the header in the body |

`human` is reserved for the keyboard. A human on the bus is a participant reaching this session over a conduit, which is what `peer` means to the harness; the human-versus-agent distinction the approval rule needs rides in `origin.attend.from_kind`. Mapping bus humans to `origin.kind = human` was considered and set aside: `/insights` reads `human` as this session's operator directing it, and for a second operator, or the same operator addressing a different session, that reading is the error #532 documents.

## What #535 consumes

#535 makes the drain age- and party-aware and amends ADR-172. It needs one definition and two inputs from this note.

A session S is party to a message M when any of the following holds: S's session id is in `M.to`; `M.from_id` is S's session id (own messages already mark without delivering); M carries `re` and the message it replies to is one S is party to, followed through the `re` chain to the root. The chain usually resolves, since replies land in the same scan directories as their parents and nothing is reaped by age. A parent can still be missing: it may sit in a directory S does not scan (`cmd_inbox_read` covers S's tray, `_broadcast/`, and joined channels only), or its sender's project may have been reaped by liveness (`run_cleanup`). An unresolvable `re` makes M party-unknown, and the drain renders party-unknown messages in full. Collapsing is reserved for threads whose every link resolved to a non-party message. A channel message (`to = #<name>`) reaches S only if S joined the channel, so the drain treats channel traffic as party by membership. Broadcast traffic (`to = *`) is party only through the sender or the reply chain, and that was the case the 2026-09-19 session hit: ten broadcast messages between two other agents.

Age is the file mtime the drain already reads into `when`. Nothing new is needed for it.

With those, #535 groups delivered messages by thread root, shows each thread's age, renders threads S is party to in full, collapses each non-party thread to one line (`[3 messages, Hana-beta and Tam, #open, 2d to 4h ago]`), and optionally marks non-party messages older than a window as consumed without rendering, leaving them readable in `attend inbox`. Marking without rendering has precedent in the drain's cold-start baseline (`scan_pending`, `baselining`).

Parts of ADR-172 this amends:

- Decision 1, "pulls pending authored messages for this session and, if any, injects them": the drain injects party messages in full and non-party threads as summaries, and may withhold aged non-party messages from injection while still marking them consumed.
- The render cap (`DRAIN_RENDER_MAX`, a constant the ADR's implementation PR set): the cap applies after party ordering, so party messages are never the ones pushed into the "+N more" remainder.
- Nothing in Decisions 2 through 6 changes: the verb, the shared seen-set and its merge semantics, the tuple key and resolved gate, the no-reap rule, and the re-entry ceiling all stand. Retention is ADR-136's and #535 does not touch it.

The sensor digest (`build_digest`) can adopt the same party partition without amending ADR-136, since the digest's contract (count-led, nothing dropped, detail in `attend inbox`) is unchanged.

## #538 and typed tool parameters

Under the MCP server, `send` takes `message: string`, `to?: string[]`, `channel?: string`, `on_behalf_of?: string`. `reply` takes `message: string` and `on_behalf_of?: string`. `from_kind` and `from_id` are never parameters: the server derives them per connection through the same ADR-171 resolution, which is the per-connection identity #538 already requires. `to` accepts display names or canonical ids at the tool boundary and the server resolves names to ids before writing, so the on-disk field carries ids only. The CLI's `--to <path>` is the same resolution with a path as input. Both frontends write the same envelope through the one library the previous section recommends.

## Out of scope, and why the fields come first

Signing and cross-machine relay are separate work with their own ADR. This note leaves them out because each needs a decision this note does not make: key custody and fingerprint issuance for signing, transport and trust for relay. The fields are still the prerequisite for both. A signature covers named fields, and today the sender exists only in prose and the addressee only in a directory name; there is nothing to bind. `from_id` is the slot a key fingerprint fills, and the reader rule "prefer the tagged field" is what lets that substitution land without a second migration. `on_behalf_of` is the claim signing turns from asserted into verified. Relay needs `to` to survive transport, since the destination directory does not.

## Builder checklist

One change, in this order:

1. Shared parse and format for the line (or lockstep edits at `attend/src/cmd/inbox.rs`, `sensor-peers/src/lib.rs`, `attend-chat/src/signal.rs`), with round-trip tests for legacy and tagged records, the two value classes, the prose fence, and the legacy derivation. attend-chat's `parse_file` is a rewrite to the tag-consuming parse, since it has no fence today.
2. `validate_group_name` in `attend-groups`: tighten to `[A-Za-z0-9_.-]+`, with a test that `dev|ops`, `équipe`, and a name containing `,` are rejected.
3. `cmd_send`: resolve `from_kind` and `from_id` from `attend_session::identity()`, build `to` from the routing branch, accept `--on-behalf-of <id>` on `send` and `reply` in `cli.rs`, and validate its value against the wider class, exiting 1 on failure as `--re` does. `attend-chat` `write_signal`: `from_kind = human`, `from_id = human_member_id()`, `to` from the destination: `*` for broadcast, `#<name>` for a channel, the session ids at the root for a directed `@Nickname` send and for `/invite` and `/kick` notices.
4. `cmd_reply`: look up the parent by id to set `to`.
5. Readers: `Drained` and `render_drain_reason`, the sensor's Monitor line and digest header, `cmd_inbox` and `cmd_inbox_read`.
6. The three synchronized messaging docs (`skills/attend/SKILL.md`, `tools/sensor-disclosure/src/disclosures/messaging.md`, `hooks/ways/softwaredev/environment/attend/attend.md`) gain one sentence each on `--on-behalf-of` and on what the header line means, per the ADR-136 lockstep rule.

## See also

- ADR-136, message lane split from the sensor bus; the lockstep rule and the no-reap decision this note relies on.
- ADR-170, human identity is the sanitized username; the `from_id` rule for humans.
- ADR-171, the identity tuple; the `from_kind` rule and the id space.
- ADR-172, the turn-boundary drain; the conduit #535 amends.
- ADR-173, chat idioms; `--channel` and `--to` as the routing flags these fields record.
- Design note: cognitive loop and awareness layer; the bracketed key-value output format.
- RFC 5322 (`From`, `Sender`, `To`) and RFC 8693 (`sub`, `act`) for the shape of the actor and principal split.
