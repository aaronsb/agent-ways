//! The ways settings schema (ADR-503 §1): every key of `config.yaml`, of a
//! target's `config.yaml` and of a project's `.claude/ways.yaml`, with its
//! type, range, default, scope and help. [`crate::config::Config`] loads
//! through it, and `ways settings` composes it with the other components'.

use agent_settings::schema::{DefaultValue, FileSpec, Kind, KeySpec, LayerScope, Schema, Scope, SectionSpec};
use agent_settings::Layer;
use serde_yaml::Value;
use std::path::{Path, PathBuf};

/// The file kind: user, target and project files all take these keys.
pub const FILE: &str = "config";

/// The sections the hook commands read (ADR-503 §5). `config::global()`
/// serves the hooks and `reconcile` alike, so every ways section but
/// `theme` is read on the hook path; no hook reads a theme (ADR-504 §11).
/// The list is explicit so a hook names each section it loads, through
/// `load-sections`, never a whole-file `load-all`.
pub const HOOK_SECTIONS: &[&str] =
    &["ways", "ways.switch", "ways.subagents", "ways.domains", "matching", "install.targets", "install.secret_path_deny", "ways.project", "ways.log"];

/// The fallback unit is the section (ADR-503 §4), so each switch that turns
/// something off is a section of its own: a bad value elsewhere can never
/// switch it back on. Per-way toggles fall back one entry at a time.
const SECTIONS: &[SectionSpec] = &[
    SectionSpec {
        name: "ways",
        file: FILE,
        top: &["language", "default_scope"],
        per_entry: false, entry: None, repair: None,
        columns: None,
        doc: "The language ways are written in and the default scope of a way.",
    },
    SectionSpec {
        name: "ways.switch",
        file: FILE,
        top: &["enabled"],
        per_entry: false, entry: None, repair: None,
        columns: None,
        doc: "Whether ways run at all; false in a project's .claude/ways.yaml switches them off there.",
    },
    SectionSpec {
        name: "ways.subagents",
        file: FILE,
        top: &["subagents"],
        per_entry: false, entry: None, repair: None,
        columns: None,
        doc: "Whether subagents and teammates get ways; the main agent's ways are unaffected.",
    },
    SectionSpec {
        name: "ways.domains",
        file: FILE,
        top: &["disabled_domains"],
        per_entry: false, entry: None, repair: None,
        columns: None,
        doc: "Domains switched off everywhere.",
    },
    SectionSpec {
        name: "matching",
        file: FILE,
        top: &[
            "semantic_fire_probability",
            "keyword_floor_probability",
            "parent_threshold_multiplier",
            "parent_boost_floor",
            "near_miss_margin",
            "admission",
            "body_rank",
            "refire_presets",
        ],
        per_entry: false, entry: None, repair: None,
        columns: None,
        doc: "When a way fires: the calibrated probabilities, the parent boost, and how often a way may fire again.",
    },
    SectionSpec {
        name: "ways.log",
        file: FILE,
        top: &["event_retention_days", "decision_retention_turns"],
        per_entry: false, entry: None, repair: None,
        columns: None,
        doc: "How long the event and decision logs keep history.",
    },
    SectionSpec {
        name: "install.targets",
        file: FILE,
        top: &["targets"],
        per_entry: true, entry: None,
        repair: Some("`ways manage target add|enable|disable|remove <dir> --commit`"),
        columns: Some(("target", "state")),
        doc: "Where agent-ways is active. Changed by `ways manage target`, which reconciles.",
    },
    SectionSpec {
        name: "install.secret_path_deny",
        file: FILE,
        top: &["secret_path_deny"],
        per_entry: false, entry: None, repair: None,
        columns: None,
        doc: "Whether the secret-path permissions.deny baseline is merged into settings.json.",
    },
    SectionSpec {
        name: "install.manage_mode",
        file: FILE,
        top: &["manage_mode"],
        per_entry: false, entry: None, repair: None,
        columns: None,
        doc: "Whether `ways manage update`, `repair` and `target` apply without --commit.",
    },
    SectionSpec {
        name: "ways.project",
        file: FILE,
        top: &["ways"],
        per_entry: true, entry: None, repair: None,
        columns: Some(("way", "enabled")),
        doc: "Per-way switches for one project (ADR-131), in its .claude/ways.yaml. Each entry falls back alone.",
    },
    // Not in HOOK_SECTIONS: the attend mod reads its entry through `ways
    // settings list mods.attend`, and reconcile reads each mod's `install`
    // per target. Each entry, one per mod, falls back alone (ADR-512 item 1).
    SectionSpec {
        name: "mods",
        file: FILE,
        top: &["mods"],
        per_entry: true, entry: None, repair: None,
        columns: None,
        doc: "The Claude Code mods agent-ways ships, one entry per mod (ADR-512): whether repair installs each one, and its other settings.",
    },
    // Not in HOOK_SECTIONS: no hook path reads a theme (ADR-504 §11). Declared
    // in agent-theme, so attend reads the same keys (one source).
    agent_theme::settings::SECTION,
];

const RETIRED: &[(&str, &str)] = &[
    ("attend_mod", ATTEND_MOD_RETIRED),
    ("default_embed_threshold", "`default_embed_threshold` was retired by ADR-156 (calibrated scoring) and is ignored; use `semantic_fire_probability` (a probability in [0,1])"),
    ("default_multi_embed_threshold", "`default_multi_embed_threshold` was retired by ADR-156 (calibrated scoring) and is ignored; use `semantic_fire_probability` (a probability in [0,1])"),
    ("keyword_gate_fraction", "`keyword_gate_fraction` was retired by ADR-156 (calibrated scoring) and is ignored; use `keyword_floor_probability` (a probability in [0,1])"),
];

/// What a file that still holds `attend_mod` is told: nothing reads it, and
/// the command that moves its values (ADR-512 item 2).
pub const ATTEND_MOD_RETIRED: &str = "`attend_mod` was retired by ADR-512 and is ignored; its keys are now under `mods.attend`, and `ways settings fix mods.attend` moves them";

/// The mods agent-ways ships to Claude Code, by the name of their entry
/// under `mods`, with the doc of that entry. The plugins
/// `plugins/.claude-plugin/marketplace.json` lists, in its order.
pub const MODS: &[(&str, &str)] = &[
    ("attend", "The attend mod in Claude Code (ADR-406, ADR-414): whether a session starts listening, keepwarm, keepalive, the install and poll timing."),
    ("ways-judge", "The ways-judge mod, the relevance gate's Claude Code internal engine (ADR-708): whether repair installs it."),
    ("ways-theme", "The ways-theme mod, which has Claude Code follow theme.active (ADR-513): whether repair installs it."),
];

const BASE: KeySpec = KeySpec {
    name: "",
    section: "ways",
    file: FILE,
    path: &[],
    kind: Kind::Text,
    default: DefaultValue::None,
    instances: &[],
    scope: Scope::Both,
    doc: "",
    long: "",
    check: None,
    computed: None,
    fail_closed: None,
};

const PROB: Kind = Kind::Float { min: 0.0, max: 1.0 };

const KEYS: &[KeySpec] = &[
    KeySpec {
        name: "ways.enabled",
        section: "ways.switch",
        fail_closed: Some(closed_off),
        path: &["enabled"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        doc: "Whether hooks inject ways at all.",
        long: "Set false in a project's .claude/ways.yaml to switch ways off for that project: hooks inject nothing there (ADR-184).",
        ..BASE
    },
    KeySpec {
        name: "ways.subagents",
        section: "ways.subagents",
        fail_closed: Some(closed_off),
        path: &["subagents"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        doc: "Whether subagents and teammates get ways.",
        long: "Set false in a project's .claude/ways.yaml to inject nothing into the agents that project's sessions dispatch; the main agent keeps its ways. `ways session subagents off` does the same for one session (#768).",
        ..BASE
    },
    KeySpec {
        name: "ways.language",
        path: &["language"],
        kind: Kind::ChoiceOf { options: languages, multi: false },
        default: DefaultValue::Yaml("auto"),
        doc: "Output language: en, auto, or a language code.",
        long: "en and auto keep the English corpus. A specific code, set by the ways-localize skill, switches the build, matcher and tuner to that language (ADR-139).",
        ..BASE
    },
    KeySpec {
        name: "ways.default_scope",
        path: &["default_scope"],
        default: DefaultValue::Yaml("agent"),
        doc: "The scope of a way that names none.",
        long: "Ways without a scope: field apply at this scope.",
        ..BASE
    },
    KeySpec {
        name: "ways.disabled_domains",
        section: "ways.domains",
        fail_closed: Some(closed_domains),
        path: &["disabled_domains"],
        kind: Kind::ChoiceOf { options: domains, multi: true },
        default: DefaultValue::Yaml("[]"),
        doc: "Domains switched off everywhere, such as [ea, itops].",
        long: "Every way under a listed domain stays silent. Set as a list, `[ea, itops]` or `ea,itops`.",
        ..BASE
    },
    KeySpec {
        name: "ways.project.*",
        fail_closed: Some(closed_toggle),
        section: "ways.project",
        path: &["ways", "*"],
        kind: Kind::Toggle,
        default: DefaultValue::Yaml("true"),
        scope: Scope::Project,
        doc: "One way on or off in this project.",
        long: "Project scope only (ADR-131). `false` silences the way in this project; absent means on. `ways settings set ways.project.<id> false` turns a way off; `ways settings unset ways.project.<id>` turns it back on. A key ending in `/*`, such as `softwaredev/code/supplychain/*`, covers that directory's way and every way under it; a toggle on a way itself overrides the prefix (ADR-701).",
        ..BASE
    },
    KeySpec {
        name: "matching.semantic_fire_probability",
        section: "matching",
        path: &["semantic_fire_probability"],
        kind: PROB,
        default: DefaultValue::Yaml("0.5"),
        doc: "Calibrated probability at which a way fires on relatedness.",
        long: "A way fires when the calibrated probability of its best model lane reaches this value (ADR-156). Lower fires more often.",
        ..BASE
    },
    KeySpec {
        name: "matching.keyword_floor_probability",
        section: "matching",
        path: &["keyword_floor_probability"],
        kind: PROB,
        default: DefaultValue::Yaml("0.15"),
        doc: "Calibrated probability a keyword hit must also reach.",
        long: "A pattern: hit fires only when the calibrated probability reaches this floor (ADR-156). Ways opt out with pattern_strict: true.",
        ..BASE
    },
    KeySpec {
        name: "matching.parent_threshold_multiplier",
        section: "matching",
        path: &["parent_threshold_multiplier"],
        kind: Kind::Float { min: 0.0, max: 2.0 },
        default: DefaultValue::Yaml("0.8"),
        doc: "Multiplier on a child way's fire probability once its parent fired.",
        long: "Below 1.0 children fire more easily once their parent domain is active (progressive disclosure). 1.0 turns the boost off.",
        ..BASE
    },
    KeySpec {
        name: "matching.parent_boost_floor",
        section: "matching",
        path: &["parent_boost_floor"],
        kind: PROB,
        default: DefaultValue::Yaml("0.3"),
        doc: "Lowest fire probability the parent boost may reach.",
        long: "Keeps cascading boosts out of the noise band where generic words collide.",
        ..BASE
    },
    KeySpec {
        name: "matching.near_miss_margin",
        section: "matching",
        path: &["near_miss_margin"],
        kind: PROB,
        default: DefaultValue::Yaml("0.05"),
        doc: "How far below its threshold a score is logged as a near miss.",
        long: "Telemetry only (ADR-134): it never changes firing. The tuning passes read the near-miss events.",
        ..BASE
    },
    KeySpec {
        name: "matching.admission",
        section: "matching",
        path: &["admission"],
        kind: Kind::Choice(&crate::config::Admission::NAMES),
        default: DefaultValue::Yaml("share"),
        doc: "How late interaction admits a way into body confirmation: share or chunk_top.",
        long: "share admits a way whose summed softmax share over the surface's chunks reaches 0.15. chunk_top admits the top-ranked way of every chunk instead. Both also admit a way whose peak chunk cosine reaches 0.50, keep at most 6 by peak, and body-confirm them (ADR-700 §12). chunk_top is under evaluation (ADR-701 increment 6).",
        ..BASE
    },
    KeySpec {
        name: "matching.body_rank",
        section: "matching",
        path: &["body_rank"],
        kind: Kind::Choice(&crate::config::BodyRank::NAMES),
        default: DefaultValue::Yaml("scaled-single"),
        doc: "Whether a way's best body section joins its ranking score: off, on, scaled or scaled-single.",
        long: "on ranks a way by its alias cosine plus 0.25 times its best body section's cosine. scaled divides that by 1.25, keeping the alias scale the calibration was fitted on. scaled-single, the default, applies scaled to one-sentence prompts only and leaves late interaction on alias scores. on and scaled also reach late interaction, where they confirm the body against a section other than the one that contributed (a way with one section or none confirms against its alias). All of it applies only while the body sidecar is complete. off ranks on alias cosines alone (ADR-701 §6).",
        ..BASE
    },
    KeySpec {
        name: "matching.refire_presets.*",
        section: "matching",
        path: &["refire_presets", "*"],
        // frontmatter's REFIRE_NUMERIC_MAX: above 1.0 is valid but rare.
        kind: Kind::Float { min: 0.0, max: 10.0 },
        default: DefaultValue::Fn(refire_default),
        instances: &["once", "rare", "normal", "frequent"],
        doc: "A refire preset: the fraction of the context window before a way may fire again.",
        long: "A way's `refire: <name>` looks the preset up here and multiplies by the session's context window (ADR-126). New names may be added.",
        ..BASE
    },
    KeySpec {
        name: "ways.event_retention_days",
        section: "ways.log",
        path: &["event_retention_days"],
        kind: Kind::Int { min: 1, max: 3650 },
        scope: Scope::User,
        default: DefaultValue::Yaml("365"),
        doc: "Days an archive file is kept after it is written.",
        long: "Machine-wide, so user scope only: a project file cannot shorten it. The live events.jsonl is bounded by size and by age, and the lines it sheds are written to events-YYYY-MM-DD.jsonl.gz beside it, named for the day of the removal. Archives older than this many days are deleted, at most once a day (ADR-701 §2). Introspection, `ways tune stats` and the tuning passes read the archives as well as the live file. events.jsonl itself is never deleted by this setting. judge_call lines stay in the live file, since `ways agent cost` sums them.",
        ..BASE
    },
    KeySpec {
        name: "ways.decision_retention_turns",
        section: "ways.log",
        path: &["decision_retention_turns"],
        kind: Kind::Int { min: 1, max: 10_000_000 },
        scope: Scope::User,
        default: DefaultValue::Yaml("50000"),
        doc: "Turns of decision records the live decisions.jsonl holds.",
        long: "Machine-wide, so user scope only (ADR-701 §2). A turn is a decision record with turn_start true, together with the records after it up to the next one. Once a day, when the live file holds 10% more turns than this, its oldest turns move whole to decisions-YYYY-MM-DD.jsonl.gz beside it and the newest this many stay. Those archives expire under ways.event_retention_days.",
        ..BASE
    },
    KeySpec {
        name: "install.targets",
        section: "install.targets",
        fail_closed: Some(closed_targets),
        path: &["targets"],
        kind: Kind::ReadOnly,
        scope: Scope::User,
        check: Some(check_targets),
        doc: "The Claude Code config directories agent-ways is active in.",
        long: "Changed by actions, since each change reconciles: `ways manage target add|enable|disable|remove <dir> --commit` (ADR-184). Absent means the default ~/.claude, enabled.",
        ..BASE
    },
    agent_theme::settings::ACTIVE,
    agent_theme::settings::SHAPE,
    agent_theme::settings::WAYPOINTS,
    KeySpec {
        name: "mods.attend.listening",
        section: "mods",
        fail_closed: Some(closed_dormant),
        path: &["mods", "attend", "listening"],
        kind: Kind::Choice(&["active", "dormant"]),
        default: DefaultValue::Yaml("dormant"),
        doc: "How a Claude Code session starts: active (listening over attend) or dormant.",
        long: "dormant is plain Claude Code: the attend mod lists only attend_listen, polls nothing and draws nothing, and a session listens only once the user asks for attend. active listens from session start until Claude Code exits. Set it in a project's .claude/ways.yaml for the projects where several agents work together; the project wins over a target's and the user's file (ADR-406).",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepwarm",
        section: "mods",
        fail_closed: Some(closed_keepwarm),
        path: &["mods", "attend", "keepwarm"],
        kind: Kind::Choice(&["on", "off"]),
        default: DefaultValue::Yaml("on"),
        doc: "Whether the attend mod drives cache wakes: on or off.",
        long: "on keeps ADR-182's wakes in sessions that are listening: one wake after 50 idle minutes while a window is armed, and the three-hour window a paid cold write arms. It also lets the keepalive policy (mods.attend.keepalive.*) run in every interactive session with the mod, dormant ones included, while autocompact is off (ADR-414). off means no cache wake the mod drives in that project's sessions, the policy included, and arms nothing.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.install",
        section: "mods",
        fail_closed: Some(closed_uninstalled),
        path: &["mods", "attend", "install"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        scope: Scope::Machine,
        doc: "Whether `ways manage repair` installs the attend mod in Claude Code.",
        long: "Read per target, from the user file and the target's own file; a project cannot set it. false and a repair uninstall the mod and remove the agent-ways marketplace from every target it applies to, and the mod lists nothing where it is still loaded; /attend then falls back to Monitor (ADR-406 item 11).",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.poll_seconds",
        section: "mods",
        path: &["mods", "attend", "poll_seconds"],
        kind: Kind::Int { min: 1, max: 3600 },
        default: DefaultValue::Yaml("5"),
        doc: "Seconds between polls of attend while an active session is awake.",
        long: "Each poll is one attend process: a drain while no turn runs, and the unread count for the status row.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.slow_poll_seconds",
        section: "mods",
        path: &["mods", "attend", "slow_poll_seconds"],
        kind: Kind::Int { min: 1, max: 3600 },
        default: DefaultValue::Yaml("60"),
        doc: "Seconds between polls once an active session has been quiet.",
        long: "After the quiet period the poll slows to this interval and returns to poll_seconds on the next delivery, send or turn. A store on a slow mounted directory can be given a longer one (ADR-406 item 12).",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.quiet_minutes",
        section: "mods",
        path: &["mods", "attend", "quiet_minutes"],
        kind: Kind::Int { min: 1, max: 1440 },
        default: DefaultValue::Yaml("20"),
        doc: "Minutes with no delivery, send or turn before the poll slows.",
        long: "The session keeps listening; only the interval changes.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.preamble_every_turns",
        section: "mods",
        path: &["mods", "attend", "preamble_every_turns"],
        kind: Kind::Int { min: 1, max: 10_000 },
        default: DefaultValue::Yaml("20"),
        doc: "Turns before a delivery carries the preamble line again.",
        long: "The preamble is one line of tool usage, added below the messages. It goes with the first delivery of a session and the first after a compaction or a /clear, then with the first delivery once this many turns have passed. Every other delivery carries the frame alone (ADR-406 item 4).",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.headless_poll",
        section: "mods",
        path: &["mods", "attend", "headless_poll"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("false"),
        doc: "Whether a claude -p run or an SDK session may poll attend.",
        long: "Such a run exits after one turn, so by default it lists attend_listen and never polls, whatever the project's start state.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.enabled",
        section: "mods",
        fail_closed: Some(closed_keepalive),
        path: &["mods", "attend", "keepalive", "enabled"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        doc: "Whether the attend mod keeps an idle session warm for a bounded stretch, then asks for a wrap.",
        long: "The keepalive policy (ADR-414) runs in an interactive session with the mod while autocompact is off: one keepwarm: turn the margin before the prompt cache lapses, counted from the operator's last prompt, then one wrap: turn, after which the cache lapses. With autocompact on it does not run, and ADR-182's armed windows apply as before. false returns the project to ADR-182 alone. Only an explicit true runs it: a bad value, or settings that cannot be read, leave it off.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.max_keepalives",
        section: "mods",
        path: &["mods", "attend", "keepalive", "max_keepalives"],
        kind: Kind::Int { min: 0, max: 48 },
        default: DefaultValue::Yaml("8"),
        doc: "Keepalives allowed since the operator's last prompt before the wrap.",
        long: "Each keepalive is one short turn that reads the cached context. With the defaults the keepalives land 50, 100 and on to 400 minutes after the last turn, and the wrap at 450 minutes. 0 wraps at the first due point.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.margin_minutes",
        section: "mods",
        path: &["mods", "attend", "keepalive", "margin_minutes"],
        kind: Kind::Int { min: 3, max: 30 },
        default: DefaultValue::Yaml("10"),
        doc: "Minutes before the prompt cache lapses that a keepalive is due.",
        long: "Due at the last request's time plus its cache tier's TTL, less this margin. 10 keeps ADR-182's 50-minute point on the hour tier. The five-minute tier gets no keepalive.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.windows",
        section: "mods",
        path: &["mods", "attend", "keepalive", "windows"],
        kind: Kind::List,
        default: DefaultValue::Yaml("[\"daily 00:00-24:00\"]"),
        doc: "When keepalives may run: entries of `<days> <HH:MM>-<HH:MM>`.",
        long: "Days are daily, one day (Sat) or a range (Mon-Fri; Fri-Mon wraps the week). 24:00 closes a day; an end before its start crosses midnight and belongs to the start's day. Outside every window the next due point sends the wrap instead. An entry that does not parse is dropped and `attend keepwarm status` names it; an empty list is never inside. Set several with commas: `ways settings set mods.attend.keepalive.windows \"Mon-Fri 08:00-19:00, Sat 10:00-14:00\"`.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.timezone",
        section: "mods",
        path: &["mods", "attend", "keepalive", "timezone"],
        kind: Kind::Text,
        default: DefaultValue::Yaml("\"\""),
        doc: "The IANA zone the windows are read in; empty for the host's zone.",
        long: "Such as Europe/Berlin or America/Chicago. A name the host's time zone database does not know is reported by `attend keepwarm status`, and the host's zone is used.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.wrap",
        section: "mods",
        path: &["mods", "attend", "keepalive", "wrap"],
        kind: Kind::Choice(&["transcript", "file", "send", "off"]),
        default: DefaultValue::Yaml("transcript"),
        doc: "Where the wrap's final reply goes: transcript, file, send or off.",
        long: "transcript leaves it in the conversation. file has the mod write it to wrap_file. send has the mod send it over attend to wrap_to, opening with a `handoff:` line; a refused send is written to wrap_file instead. off sends no wrap, and the policy stops at the bound. The transcript keeps the reply under every destination.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.wrap_file",
        section: "mods",
        path: &["mods", "attend", "keepalive", "wrap_file"],
        kind: Kind::Text,
        default: DefaultValue::Yaml("\"{state}/wrap/{session}.md\""),
        doc: "The file the mod writes the wrap's reply to, for file and a refused send.",
        long: "{state} is the ways state root ($XDG_STATE_HOME/agent-ways), {project} the project directory's name, {session} the session id, and {date} the day in the windows' zone. The path must name {session} and resolve under {state}/wrap/ through no symlink; anything else falls back to {state}/wrap/<session>.md, and `attend keepwarm status` names the refusal.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.wrap_to",
        section: "mods",
        path: &["mods", "attend", "keepalive", "wrap_to"],
        kind: Kind::Text,
        default: DefaultValue::Yaml("\"\""),
        doc: "Who receives the wrap with send: empty for the operator.",
        long: "As `attend message send --to` reads it: a person's member id, a session's name or a project path; #name posts in that channel. Empty sends it to the operator's own member id, so it reaches attend-chat as a direct message. #open is refused, since it reaches every listening session; the reply goes to the operator.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.after_wrap",
        section: "mods",
        path: &["mods", "attend", "keepalive", "after_wrap"],
        kind: Kind::Choice(&["lapse", "compact"]),
        default: DefaultValue::Yaml("lapse"),
        doc: "What follows the wrap: lapse, or compact the conversation.",
        long: "lapse lets the prompt cache lapse an hour after the wrap. compact has the mod compact the conversation with the focus of the wrap's closing /compact line, leaving a small context for the operator's return; it is one more warm request. Either applies once the wrap has answered, or after after_wrap_keepalives more keepalives when that is set.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.after_wrap_keepalives",
        section: "mods",
        path: &["mods", "attend", "keepalive", "after_wrap_keepalives"],
        kind: Kind::Int { min: 0, max: 48 },
        default: DefaultValue::Yaml("0"),
        doc: "Keepalives after the wrap before after_wrap applies.",
        long: "The session stays warm with its whole context for this many more keepalives after the wrap, so an operator back within that stretch finds the conversation as it was and the handoff already written. Then after_wrap applies: the cache lapses, or the mod compacts the conversation with the wrap's focus. Outside the windows it applies at the next due point. 0 applies it once the wrap has answered.",
        ..BASE
    },
    KeySpec {
        name: "mods.attend.keepalive.wrap_skill",
        section: "mods",
        path: &["mods", "attend", "keepalive", "wrap_skill"],
        kind: Kind::Text,
        default: DefaultValue::Yaml("\"wrap stay\""),
        doc: "The skill the wrap turn asks for, with its arguments.",
        long: "Written `<skill> [arguments]`, as `wrap stay` (the shipped wrap skill) or `handoff`; a leading / is allowed. The skill should square the task list, write a continuation prompt and end with a /compact <focus> line, which after_wrap: compact uses. The wrap and the compaction run only when the session lists this skill: empty, a skill the session does not have, or a value refused leaves them off, and the keepalives stop at max_keepalives. The text goes into a turn nobody watches, so the name is letters, digits and : _ - . and the arguments plain words; `attend keepwarm status` names a refusal.",
        ..BASE
    },
    KeySpec {
        name: "mods.ways-judge.install",
        section: "mods",
        fail_closed: Some(closed_uninstalled),
        path: &["mods", "ways-judge", "install"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("false"),
        scope: Scope::Machine,
        doc: "Whether `ways manage repair` installs the ways-judge mod in Claude Code.",
        long: "The wanted state of the relevance gate's Claude Code internal engine (ADR-512 item 1). Read per target, from the user file and the target's own file, the target winning; a project cannot set it.",
        ..BASE
    },
    KeySpec {
        name: "mods.ways-theme.install",
        section: "mods",
        fail_closed: Some(closed_uninstalled),
        path: &["mods", "ways-theme", "install"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("false"),
        scope: Scope::Machine,
        doc: "Whether `ways manage repair` installs the ways-theme mod in Claude Code.",
        long: "Installed, Claude Code follows theme.active: the settings merge selects the custom theme custom:ways in the target's settings.json, unless the person chose another, and the mod rewrites themes/ways.json from theme.active at each session start, Claude Code's own dark for terminal. It adds no setting of its own, and when ways fails it changes nothing (ADR-513). Read per target, from the user file and the target's own file, the target winning; a project cannot set it.",
        ..BASE
    },
    KeySpec {
        name: "install.secret_path_deny",
        section: "install.secret_path_deny",
        fail_closed: Some(closed_deny),
        path: &["secret_path_deny"],
        kind: Kind::Bool,
        default: DefaultValue::Yaml("true"),
        doc: "Merge the secret-path permissions.deny baseline into settings.json.",
        long: "On by default (ADR-152). false suppresses the baseline entirely; a Claude Code deny cannot be reopened by an allow, so this is the one opt-out.",
        ..BASE
    },
    KeySpec {
        name: "install.manage_mode",
        section: "install.manage_mode",
        path: &["manage_mode"],
        kind: Kind::Choice(&["whatif", "commit"]),
        default: DefaultValue::Yaml("whatif"),
        scope: Scope::User,
        doc: "What a `ways manage` verb does without a flag: whatif (report only) or commit (apply).",
        long: "whatif, the default, has `ways manage update`, `repair` and `target add|enable|disable|remove` print what they would change and change nothing until --commit is given. commit makes them apply without the flag, as they did before the lifecycle convention. --commit and --what-if on the command line win over this setting. `uninstall` and `reinstall` remove the install and always need --commit. Machine-wide, so user scope only.",
        ..BASE
    },
];

fn refire_default(bound: &[String]) -> Option<Value> {
    let f = match bound.first().map(String::as_str) {
        Some("once") => 1.0,
        Some("rare") => 0.4,
        Some("normal") => 0.15,
        Some("frequent") => 0.05,
        _ => return None,
    };
    Some(Value::Number(f.into()))
}

// ── fail-closed readings (ADR-503 addendum) ─────────────────────
//
// A switch that turns something off keeps it off when its own value is bad
// or its file does not parse. `None` is "no opinion": fall through.

/// `enabled`: anything but `true` reads as off.
fn closed_off(v: &Value) -> Option<Value> {
    (v != &Value::Bool(true)).then_some(Value::Bool(false))
}

/// `disabled_domains`: whatever names can be read stay disabled, from a
/// list or from text such as `ea,itops`.
fn closed_domains(v: &Value) -> Option<Value> {
    let clean = |s: &str| s.trim().to_string();
    let mut names: Vec<Value> = Vec::new();
    let mut add = |s: &str| {
        for part in s.split(',') {
            let n = clean(part);
            if !n.is_empty() && !names.contains(&Value::String(n.clone())) {
                names.push(Value::String(n));
            }
        }
    };
    match v {
        Value::String(s) => add(s),
        Value::Sequence(items) => items.iter().filter_map(|i| i.as_str()).for_each(&mut add),
        _ => {}
    }
    (!names.is_empty()).then_some(Value::Sequence(names))
}

/// The values `ways.language` may take: `en` and `auto`, then the active
/// languages of the registry, the codes a locale file may carry. Read from
/// the registry compiled in, so it never depends on the directory.
fn languages(_layers: &[Layer], _: &[String]) -> Result<Vec<String>, String> {
    let mut out = vec!["en".to_string(), "auto".to_string()];
    out.extend(crate::agents::get_active_languages().into_iter().filter(|l| l != "en" && l != "auto"));
    Ok(out)
}

/// The domains `ways.disabled_domains` may name: the top-level directories
/// of the user, shipped and projected ways roots that hold a way, as the
/// engine counts one (a frontmatter fence; a file- or state-triggered way has
/// no `description:`), and those of
/// each project the layers carry (`<project>/.claude/ways.yaml` names its
/// `.claude/ways/`), since the engine honours a project's own domain there.
/// So the rule is the layers': a write to a project's file, and a write
/// from inside that project, see its domains; from another directory they
/// do not, and a stored item always stays settable.
fn domains(layers: &[Layer], _: &[String]) -> Result<Vec<String>, String> {
    let mut roots = crate::paths::ways_roots(None);
    for l in layers.iter().filter(|l| l.file == FILE && l.scope == LayerScope::Project) {
        if let Some(dir) = l.path.as_deref().and_then(Path::parent) {
            roots.push(dir.join("ways"));
        }
    }
    let mut found = std::collections::BTreeSet::new();
    for root in roots.iter().filter(|r| r.is_dir()) {
        for way in crate::scanner::scan_declared_ways(root) {
            if !way.domain.starts_with('.') {
                found.insert(way.domain);
            }
        }
    }
    Ok(found.into_iter().collect())
}

/// A per-way toggle: anything but an explicit on reads as disabled.
fn closed_toggle(v: &Value) -> Option<Value> {
    let on = match v {
        Value::Bool(b) => *b,
        Value::Mapping(m) => m.get("enabled").is_none_or(|e| e == &Value::Bool(true)),
        _ => false,
    };
    (!on).then_some(Value::Bool(false))
}

/// `mods.attend.listening`: anything but `active` reads as dormant.
fn closed_dormant(v: &Value) -> Option<Value> {
    (v.as_str() != Some("active")).then(|| Value::String("dormant".into()))
}

/// `mods.attend.keepwarm`: anything but `on` reads as off.
fn closed_keepwarm(v: &Value) -> Option<Value> {
    (v.as_str() != Some("on")).then(|| Value::String("off".into()))
}

/// `mods.attend.keepalive.enabled`: anything but `true` reads as off, the
/// side that starts no turn the operator did not type.
fn closed_keepalive(v: &Value) -> Option<Value> {
    (v != &Value::Bool(true)).then_some(Value::Bool(false))
}

/// `mods.<name>.install`: anything but `true` reads as not installed.
fn closed_uninstalled(v: &Value) -> Option<Value> {
    (v != &Value::Bool(true)).then_some(Value::Bool(false))
}

/// `secret_path_deny`: the closed side is the deny baseline merged, so
/// only a valid `false` opts out.
fn closed_deny(v: &Value) -> Option<Value> {
    (v != &Value::Bool(false)).then_some(Value::Bool(true))
}

/// One `targets` item (as a one-item list): a valid item stays as written;
/// an invalid one with a readable path is kept disabled, so it is withdrawn,
/// never projected into. With no value at all, the list is empty.
fn closed_targets(v: &Value) -> Option<Value> {
    // No value (a file that does not parse): no target, so nothing is
    // projected and the implicit default never applies.
    let Some(items) = v.as_sequence() else { return Some(Value::Sequence(Vec::new())) };
    let item = items.first()?;
    if check_targets(v).is_ok() {
        return Some(v.clone());
    }
    let path = item.get("path")?.as_str()?;
    let mut m = serde_yaml::Mapping::new();
    m.insert("path".into(), path.into());
    m.insert("enabled".into(), Value::Bool(false));
    Some(Value::Sequence(vec![Value::Mapping(m)]))
}

/// A theme name as a theme file names itself: `[a-z0-9-]+`.
fn check_targets(v: &Value) -> Result<(), String> {
    serde_yaml::from_value::<Vec<crate::config::Target>>(v.clone()).map(|_| ()).map_err(|e| e.to_string())
}

pub static SCHEMA: Schema = Schema {
    component: "ways",
    files: &[FileSpec { id: FILE, retired: RETIRED }],
    sections: SECTIONS,
    keys: KEYS,
};

/// Keys of both scopes that the settings screens write to the project's
/// `.claude/ways.yaml`, not the user file: how a project's sessions start
/// is decided per project (ADR-406 item 2), and so is the keepalive policy
/// (ADR-414 item 8). `ways settings set` still writes
/// the user file unless `--project` is given.
pub const SCREENS_WRITE_PROJECT: &[&str] = &[
    "mods.attend.listening",
    "mods.attend.keepwarm",
    "mods.attend.keepalive.enabled",
    "mods.attend.keepalive.max_keepalives",
    "mods.attend.keepalive.margin_minutes",
    "mods.attend.keepalive.windows",
    "mods.attend.keepalive.timezone",
    "mods.attend.keepalive.wrap",
    "mods.attend.keepalive.wrap_file",
    "mods.attend.keepalive.wrap_to",
    "mods.attend.keepalive.after_wrap",
    "mods.attend.keepalive.after_wrap_keepalives",
    "mods.attend.keepalive.wrap_skill",
];

/// Whether the mod `name` (an entry of [`MODS`]) is wanted in a target
/// (ADR-512 item 1): `mods.<name>.install` from the user file, then the
/// target's own file. A project cannot set it.
pub fn mod_install(name: &str, target_config: &Path) -> bool {
    let layers = [
        Layer::read(&SCHEMA, "user", FILE, LayerScope::User, &crate::paths::user_config()),
        Layer::read(&SCHEMA, "target", FILE, LayerScope::Target, target_config),
    ];
    mod_install_in(name, &layers)
}

/// What one file sets for `mods.<name>.install`, if anything: the target's
/// config, read alone, to tell a per-target override from the user's
/// setting.
pub fn mod_install_in_file(name: &str, path: &Path) -> Option<bool> {
    let layer = Layer::read(&SCHEMA, "file", FILE, LayerScope::Target, path);
    layer.get(&["mods".to_string(), name.to_string(), "install".to_string()]).and_then(Value::as_bool)
}

/// What the user config sets for `mods.<name>.install`, if anything.
pub fn mod_install_in_user(name: &str) -> Option<bool> {
    let layer = Layer::read(&SCHEMA, "user", FILE, LayerScope::User, &crate::paths::user_config());
    layer.get(&["mods".to_string(), name.to_string(), "install".to_string()]).and_then(Value::as_bool)
}

fn mod_install_in(name: &str, layers: &[Layer]) -> bool {
    let key = format!("mods.{name}.install");
    let Some(spec) = SCHEMA.keys.iter().find(|k| k.name == key) else { return false };
    let r = agent_settings::load::resolve(spec, &[], layers);
    r.value.or(r.default).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Whether the user config sets `install.manage_mode: commit`, so a
/// `ways manage` verb that honours it applies without `--commit`. A bad or
/// absent value is whatif.
pub fn manage_mode_commit() -> bool {
    let layers = [Layer::read(&SCHEMA, "user", FILE, LayerScope::User, &crate::paths::user_config())];
    manage_mode_commit_in(&layers)
}

fn manage_mode_commit_in(layers: &[Layer]) -> bool {
    let Some(spec) = SCHEMA.keys.iter().find(|k| k.name == "install.manage_mode") else { return false };
    agent_settings::load::resolve(spec, &[], layers).value.and_then(|v| v.as_str().map(|s| s == "commit")).unwrap_or(false)
}

/// The project overlay file of a project directory.
pub fn project_file(project_dir: &Path) -> PathBuf {
    project_dir.join(".claude").join("ways.yaml")
}

/// The text a new project overlay starts with.
pub const PROJECT_HEADER: &str = "# Project-scope ways overlay — see ADR-115, ADR-131\n";

/// The project root settings resolve `start` to: the nearest ancestor that
/// is a project root with the overlays as markers too
/// ([`crate::util::is_project_root`]), and only when none is, the nearest
/// git checkout, so a fresh repository takes its first project write. An
/// outer project wins over a nested checkout. The home directory is never a
/// project: its `.claude/` is the user's.
pub fn project_root(start: &Path) -> Option<PathBuf> {
    let home = claude_sessions::home_dir();
    let candidates = || start.ancestors().filter(|d| !is_home(d, &home));
    candidates()
        .find(|d| crate::util::is_project_root(d, crate::util::SETTINGS_OVERLAYS))
        .or_else(|| candidates().find(|d| d.join(".git").exists()))
        .map(Path::to_path_buf)
}

/// Whether `dir` is the home directory `home`, which is never a project.
pub fn is_home(dir: &Path, home: &Path) -> bool {
    let canon = |q: &Path| std::fs::canonicalize(q).unwrap_or_else(|_| q.to_path_buf());
    canon(dir) == canon(home)
}

/// A `theme` setting (`active`, `shape`) as the layers resolve it for
/// `project_dir`, the highest layer that sets it winning.
pub fn theme_setting(project_dir: &Path, key: &str) -> Option<String> {
    let path = vec!["theme".to_string(), key.to_string()];
    layers(project_dir).iter().rev().find_map(|l| l.get(&path).and_then(|v| v.as_str().map(str::to_string)))
}

/// Every layer a ways key resolves through, lowest first: the user file,
/// the current target's file, and the project overlay. ADR-506 retired the
/// pre-1.0 `ways.json` and `$XDG_CONFIG_HOME/ways/config.yaml` layers.
pub fn layers(project_dir: &Path) -> Vec<Layer> {
    let mut out = Vec::new();
    let user = crate::paths::user_config();
    let user_layer = Layer::read(&SCHEMA, "user", FILE, LayerScope::User, &user);
    let targets: Option<Vec<crate::config::Target>> =
        user_layer.get(&["targets".into()]).and_then(|v| serde_yaml::from_value(v.clone()).ok());
    out.push(user_layer);
    let current = crate::paths::current_config_dir();
    let list = targets.unwrap_or_else(|| crate::config::Config::default().targets());
    if let Some(t) = list.iter().find(|t| t.matches_dir(&current)) {
        let path = t.config_path();
        if path.is_file() {
            out.push(Layer::read(&SCHEMA, "target", FILE, LayerScope::Target, &path));
        }
    }
    out.push(Layer::read(&SCHEMA, "project", FILE, LayerScope::Project, &project_file(project_dir)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(name: &str, scope: LayerScope, text: &str) -> Layer {
        Layer::from_text(&SCHEMA, name, FILE, scope, None, text)
    }

    fn resolved(name: &str, layers: &[Layer]) -> Option<Value> {
        let spec = SCHEMA.keys.iter().find(|k| k.name == name).unwrap();
        agent_settings::load::resolve(spec, &[], layers).value
    }

    #[test]
    fn manage_mode_defaults_to_whatif_and_reads_commit_from_the_user_file() {
        assert!(!manage_mode_commit_in(&[layer("user", LayerScope::User, "")]));
        assert!(manage_mode_commit_in(&[layer("user", LayerScope::User, "manage_mode: commit\n")]));
        assert!(!manage_mode_commit_in(&[layer("user", LayerScope::User, "manage_mode: whatif\n")]));
        assert!(!manage_mode_commit_in(&[layer("user", LayerScope::User, "manage_mode: yes\n")]), "a bad value is whatif");
        assert!(!manage_mode_commit_in(&[layer("project", LayerScope::Project, "manage_mode: commit\n")]), "a project cannot set it");
    }

    #[test]
    fn mods_attend_defaults_dormant_keepwarm_on_installed() {
        assert_eq!(resolved("mods.attend.listening", &[]), Some(Value::String("dormant".into())));
        assert_eq!(resolved("mods.attend.keepwarm", &[]), Some(Value::String("on".into())));
        assert_eq!(resolved("mods.attend.install", &[]), Some(Value::Bool(true)));
        assert_eq!(resolved("mods.attend.quiet_minutes", &[]), Some(Value::Number(20.into())));
        assert_eq!(resolved("mods.attend.preamble_every_turns", &[]), Some(Value::Number(20.into())));
        assert_eq!(resolved("mods.attend.headless_poll", &[]), Some(Value::Bool(false)));
        assert!(mod_install_in("attend", &[]));
    }

    /// ADR-414 item 8: the keepalive policy's defaults, and its switch
    /// falling to off when its section does not load.
    #[test]
    fn mods_attend_keepalive_defaults_and_its_switch_fails_closed() {
        assert_eq!(resolved("mods.attend.keepalive.enabled", &[]), Some(Value::Bool(true)));
        assert_eq!(resolved("mods.attend.keepalive.max_keepalives", &[]), Some(Value::Number(8.into())));
        assert_eq!(resolved("mods.attend.keepalive.margin_minutes", &[]), Some(Value::Number(10.into())));
        assert_eq!(
            resolved("mods.attend.keepalive.windows", &[]),
            Some(Value::Sequence(vec![Value::String("daily 00:00-24:00".into())]))
        );
        assert_eq!(resolved("mods.attend.keepalive.timezone", &[]), Some(Value::String(String::new())));
        assert_eq!(resolved("mods.attend.keepalive.wrap", &[]), Some(Value::String("transcript".into())));
        assert_eq!(resolved("mods.attend.keepalive.wrap_file", &[]), Some(Value::String("{state}/wrap/{session}.md".into())));
        assert_eq!(resolved("mods.attend.keepalive.wrap_to", &[]), Some(Value::String(String::new())));
        assert_eq!(resolved("mods.attend.keepalive.after_wrap", &[]), Some(Value::String("lapse".into())));
        assert_eq!(resolved("mods.attend.keepalive.after_wrap_keepalives", &[]), Some(Value::Number(0.into())));
        assert_eq!(resolved("mods.attend.keepalive.wrap_skill", &[]), Some(Value::String("wrap stay".into())));
        assert_eq!(closed_keepalive(&Value::Null), Some(Value::Bool(false)));
        assert_eq!(closed_keepalive(&Value::Bool(true)), None);
    }

    #[test]
    fn the_project_wins_for_listening_and_keepwarm() {
        let layers = [
            layer("user", LayerScope::User, "mods:\n  attend:\n    listening: dormant\n    keepwarm: on\n"),
            layer("target", LayerScope::Target, "mods:\n  attend:\n    keepwarm: off\n"),
            layer("project", LayerScope::Project, "mods: { attend: { listening: active } }\n"),
        ];
        assert_eq!(resolved("mods.attend.listening", &layers), Some(Value::String("active".into())));
        assert_eq!(resolved("mods.attend.keepwarm", &layers), Some(Value::String("off".into())));
    }

    #[test]
    fn install_is_read_from_the_user_and_target_files_never_a_project() {
        let user_off = layer("user", LayerScope::User, "mods:\n  attend:\n    install: false\n");
        assert!(!mod_install_in("attend", std::slice::from_ref(&user_off)));
        let target_on = layer("target", LayerScope::Target, "mods:\n  attend:\n    install: true\n");
        assert!(mod_install_in("attend", &[user_off.clone(), target_on]));
        let project_off = layer("project", LayerScope::Project, "mods:\n  attend:\n    install: false\n");
        assert!(!project_off.findings.is_empty(), "a project file may not set install");
        assert!(mod_install_in("attend", &[project_off]));
    }

    #[test]
    fn a_bad_value_keeps_an_off_switch_off() {
        // A bad poll interval fails the attend entry; the switches it held keep
        // their closed reading, so keepwarm stays off and install false.
        let l = layer("user", LayerScope::User, "mods:\n  attend:\n    keepwarm: off\n    install: false\n    poll_seconds: soon\n");
        assert_eq!(resolved("mods.attend.keepwarm", std::slice::from_ref(&l)), Some(Value::String("off".into())));
        assert!(!mod_install_in("attend", std::slice::from_ref(&l)));
        let bad = layer("project", LayerScope::Project, "mods:\n  attend:\n    listening: loud\n");
        assert_eq!(resolved("mods.attend.listening", &[bad]), Some(Value::String("dormant".into())));
    }

    /// ADR-512 item 1: one entry per mod under `mods`, the ways-judge
    /// mod's wanted state false unless a file says otherwise, and each entry
    /// falling back alone.
    #[test]
    fn each_mod_has_an_entry_and_one_falls_back_without_the_other() {
        assert_eq!(resolved("mods.ways-judge.install", &[]), Some(Value::Bool(false)));
        assert!(!mod_install_in("ways-judge", &[]));
        let on = layer("target", LayerScope::Target, "mods:\n  ways-judge:\n    install: true\n");
        assert!(mod_install_in("ways-judge", std::slice::from_ref(&on)));
        let l = layer("user", LayerScope::User, "mods:\n  attend:\n    listening: active\n  ways-judge:\n    install: maybe\n");
        assert_eq!(resolved("mods.attend.listening", std::slice::from_ref(&l)), Some(Value::String("active".into())));
        assert!(!mod_install_in("ways-judge", std::slice::from_ref(&l)));
        for (name, _) in MODS {
            assert!(SCHEMA.keys.iter().any(|k| k.name == format!("mods.{name}.install")), "{name} has no install key");
        }
    }

    /// ADR-512 item 2: `attend_mod` is retired, so a file holding it
    /// reports it and configures nothing from it.
    #[test]
    fn attend_mod_is_retired_and_read_by_nothing() {
        let l = layer("user", LayerScope::User, "attend_mod:\n  listening: active\n  install: false\n");
        assert_eq!(resolved("mods.attend.listening", std::slice::from_ref(&l)), Some(Value::String("dormant".into())));
        assert!(mod_install_in("attend", std::slice::from_ref(&l)));
        assert!(l.findings.iter().any(|f| f.message == ATTEND_MOD_RETIRED), "{:?}", l.findings);
    }
}
