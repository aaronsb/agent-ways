//! The agent daemon's settings schema (ADR-503 §1): `agent.yaml`, the user
//! layer over the shipped engine profiles, and the provider keys, which the
//! schema shows as present or absent and never reads (§12).

use agent_settings::schema::{DefaultValue, FileSpec, Kind, KeySpec, LayerScope, Schema, Scope, SectionSpec};
use agent_settings::Layer;
use serde_yaml::Value;

use crate::profile::{self, Provider};

/// The file kind of `agent.yaml`.
pub const FILE: &str = "agent";

/// The sections the relevance gate reads on the hook path (ADR-503 §5).
pub const GATE_SECTIONS: &[&str] = &["gate", "gate.mode", "gate.profiles"];

/// `mode` is a section of its own, so a bad `engine` never turns a gate the
/// operator switched off back on; profiles fall back one profile at a time,
/// so a typo in one never drops another's tuning.
const SECTIONS: &[SectionSpec] = &[
    SectionSpec { name: "gate", file: FILE, top: &["engine"], per_entry: false, entry: None, repair: None, doc: "The engine profile the relevance gate uses." },
    SectionSpec { name: "gate.mode", file: FILE, top: &["mode"], per_entry: false, entry: None, repair: None, doc: "What the gate does with a verdict: enforce, shadow or off." },
    SectionSpec { name: "gate.profiles", file: FILE, top: &["profiles"], per_entry: true, entry: None, repair: None, doc: "Changes to the shipped engine profiles, and profiles of your own. Each profile falls back alone." },
    SectionSpec { name: "gate.keys", file: "keys", top: &[], per_entry: false, entry: None, repair: None, doc: "Provider API keys, shown present or absent. `ways agent key` adds, rotates, checks and removes them." },
];

const BASE: KeySpec = KeySpec {
    name: "",
    section: "gate.profiles",
    file: FILE,
    path: &[],
    kind: Kind::Text,
    default: DefaultValue::None,
    instances: &["anthropic", "openrouter"],
    scope: Scope::User,
    doc: "",
    long: "",
    check: None,
    computed: None,
    fail_closed: None,
};

const POSITIVE: Kind = Kind::Int { min: 1, max: i64::MAX };
const PRICE: Kind = Kind::Float { min: 0.0, max: 1000.0 };

const KEYS: &[KeySpec] = &[
    KeySpec {
        name: "gate.engine",
        section: "gate",
        path: &["engine"],
        instances: &[],
        doc: "The profile the gate uses.",
        long: "Unset: the first shipped profile whose provider has a key (anthropic, then openrouter). `ways agent key check` checks the key against the profile's model.",
        ..BASE
    },
    KeySpec {
        name: "gate.mode",
        section: "gate.mode",
        // Fails closed: a bad mode turns the gate off, which sends nothing
        // anywhere (ADR-503 addendum).
        fail_closed: Some(closed_mode),
        path: &["mode"],
        kind: Kind::Choice(&["enforce", "shadow", "off"]),
        default: DefaultValue::Yaml("enforce"),
        instances: &[],
        doc: "What the gate does with a verdict: enforce, shadow or off.",
        long: "enforce blocks candidates judged irrelevant; shadow judges and logs every candidate while the matcher decides; off judges nothing (ADR-196 §6).",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.provider",
        path: &["profiles", "*", "provider"],
        kind: Kind::Choice(&["anthropic", "openrouter"]),
        default: DefaultValue::Fn(|b| shipped_field(b, "provider")),
        doc: "The provider a profile calls.",
        long: "A profile that changes provider must name its model too, since a model id belongs to its provider.",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.model",
        path: &["profiles", "*", "model"],
        default: DefaultValue::Fn(|b| shipped_field(b, "model")),
        check: Some(check_model),
        doc: "The model a profile calls.",
        long: "The shipped profiles are tuned for Claude Haiku 4.5; another model scores on its own scale. `ways agent models` lists what a provider serves.",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.threshold",
        path: &["profiles", "*", "threshold"],
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: DefaultValue::Fn(|b| shipped_field(b, "threshold")),
        doc: "P(yes) below which enforce mode blocks a candidate.",
        long: "Per engine, because each model's confidence sits on its own scale (ADR-195).",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.timeout_ms",
        path: &["profiles", "*", "timeout_ms"],
        kind: Kind::Int { min: 1, max: 60_000 },
        default: DefaultValue::Fn(|b| shipped_field(b, "timeout_ms")),
        doc: "The judge call's deadline in milliseconds; past it the gate fails open.",
        long: "A call takes about 0.6 s plus 0.1 s per candidate (ADR-195).",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.turns",
        path: &["profiles", "*", "turns"],
        kind: POSITIVE,
        default: DefaultValue::Fn(|b| shipped_field(b, "turns")),
        doc: "Conversation turns sent as context, counted back from the last.",
        long: "",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.max_turn_chars",
        path: &["profiles", "*", "max_turn_chars"],
        kind: POSITIVE,
        default: DefaultValue::Fn(|b| shipped_field(b, "max_turn_chars")),
        doc: "Each turn is cut to its last this-many characters.",
        long: "",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.concurrency",
        path: &["profiles", "*", "concurrency"],
        kind: POSITIVE,
        default: DefaultValue::Fn(|b| shipped_field(b, "concurrency")),
        doc: "Provider calls the agent runs at once, across all sessions.",
        long: "",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.max_candidates",
        path: &["profiles", "*", "max_candidates"],
        kind: POSITIVE,
        default: DefaultValue::Fn(|b| shipped_field(b, "max_candidates")),
        doc: "Candidates judged per request; the rest pass unjudged.",
        long: "Taken in the matcher's order. Judge latency grows with each candidate.",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.price_in_per_mtok",
        path: &["profiles", "*", "price_in_per_mtok"],
        kind: PRICE,
        default: DefaultValue::Fn(|b| shipped_field(b, "price_in_per_mtok")),
        doc: "USD per million input tokens, for pricing judge calls.",
        long: "Used when the provider does not report a call's cost, as Anthropic does not. Shipped for Claude Haiku 4.5; a profile that changes model and sets no prices records its calls as unknown cost. `ways agent cost` reports the spend.",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.price_out_per_mtok",
        path: &["profiles", "*", "price_out_per_mtok"],
        kind: PRICE,
        default: DefaultValue::Fn(|b| shipped_field(b, "price_out_per_mtok")),
        doc: "USD per million output tokens, for pricing judge calls.",
        long: "",
        ..BASE
    },
    KeySpec {
        name: "gate.keys.*",
        section: "gate.keys",
        file: "keys",
        path: &["*"],
        kind: Kind::Secret,
        computed: Some(key_presence),
        doc: "Whether a provider key is present.",
        long: "Never shown and never passed as an argument (ADR-503 §12). Add or rotate one with `ways agent key add --provider <p>`, which reads it from stdin or a hidden prompt.",
        ..BASE
    },
];

fn closed_mode(v: &Value) -> Option<Value> {
    match v.as_str() {
        Some("enforce" | "shadow") => None,
        _ => Some(Value::String("off".into())),
    }
}

fn shipped_field(bound: &[String], field: &str) -> Option<Value> {
    let p = profile::shipped().get(bound.first()?)?.clone();
    serde_yaml::to_value(p).ok()?.get(field).cloned()
}

fn check_model(v: &Value) -> Result<(), String> {
    match v.as_str() {
        Some(m) if profile::valid_model_id(m) => Ok(()),
        _ => Err("not a model id (letters, digits and . _ : / - only)".into()),
    }
}

fn key_presence(bound: &[String]) -> Value {
    let present = bound
        .first()
        .and_then(|p| Provider::parse(p).ok())
        .is_some_and(|p| crate::keys::locate(p).is_some());
    Value::String(if present { "present" } else { "absent" }.into())
}

pub static SCHEMA: Schema = Schema {
    component: "ways-agent",
    files: &[FileSpec { id: FILE, retired: &[] }, FileSpec { id: "keys", retired: &[] }],
    sections: SECTIONS,
    keys: KEYS,
};

/// The layers the agent's keys resolve through: the user layer.
pub fn layers() -> Vec<Layer> {
    vec![Layer::read(&SCHEMA, "user", FILE, LayerScope::User, &profile::user_layer_path())]
}
