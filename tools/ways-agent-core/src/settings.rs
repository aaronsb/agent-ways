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
    SectionSpec { name: "gate", file: FILE, top: &["engine"], per_entry: false, entry: None, repair: None, columns: None, doc: "The engine profile the relevance gate uses." },
    SectionSpec { name: "gate.mode", file: FILE, top: &["mode"], per_entry: false, entry: None, repair: None, columns: None, doc: "What the gate does with a verdict: enforce, shadow or off." },
    SectionSpec { name: "gate.profiles", file: FILE, top: &["profiles"], per_entry: true, entry: None, repair: None, columns: None, doc: "Changes to the shipped engine profiles, and profiles of your own. Each profile falls back alone." },
    SectionSpec { name: "gate.keys", file: "keys", top: &[], per_entry: false, entry: None, repair: None, columns: Some(("provider", "key")), doc: "Provider API keys, shown present or absent. `ways agent key` adds, rotates, checks and removes them." },
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
        kind: Kind::ChoiceOf { options: profile_names, multi: false },
        instances: &[],
        doc: "The profile the gate uses.",
        long: "A shipped profile or one of your own under gate.profiles. Unset: the first shipped profile whose provider has a key (anthropic, then openrouter); adding a key never switches it, and `ways agent status` says which applies. `ways agent key check` checks the key against the profile's model.",
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
        kind: Kind::ChoiceOf { options: model_ids, multi: false },
        default: DefaultValue::Fn(|b| shipped_field(b, "model")),
        check: Some(check_model),
        doc: "The model a profile calls.",
        long: "The shipped profiles are tuned for Claude Haiku 4.5 (Haiku 5.5 ranked worse); another model scores on its own scale. `ways agent models` lists what a provider serves and keeps the list this key offers; until it has run for the profile's provider the key takes any model id, and a list that has aged is still offered.",
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
        long: "The hook offers at most two turns: Claude's last reply, then the prompt. A value above 2 sends those two.",
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
        doc: "USD per million input tokens, for pricing judge calls.",
        long: "For a provider that does not report a call's cost, as Anthropic does not. Unset: the list price of Claude Haiku 5.5 or 4.5 for that model and its dated ids, else the call's cost is unknown. Applies only with price_out_per_mtok set too. `ways agent cost` reports the spend.",
        ..BASE
    },
    KeySpec {
        name: "gate.profiles.*.price_out_per_mtok",
        path: &["profiles", "*", "price_out_per_mtok"],
        kind: PRICE,
        doc: "USD per million output tokens, for pricing judge calls.",
        long: "Set with price_in_per_mtok; see it for the default.",
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
        long: "Never shown and never passed as an argument (ADR-503 §12). Add or rotate one with `ways agent key add --provider <p>`, which reads it from stdin or a prompt that shows a dot per character.",
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

/// The profiles `gate.engine` may name: the shipped ones, then each the
/// user layer adds under `profiles:`, in the file's order. A profile of your
/// own whose patch does not build, such as one with no model, names nothing
/// the gate could run, so it is left out. A shipped one stays: its bad patch
/// is dropped and it runs as shipped ([`profile::profiles`]).
fn profile_names(layers: &[Layer], _: &[String]) -> Result<Vec<String>, String> {
    let shipped = profile::shipped();
    let mut out: Vec<String> = shipped.keys().cloned().collect();
    for l in layers.iter().filter(|l| l.file == FILE) {
        let Some(Value::Mapping(m)) = l.accepted.get("profiles") else { continue };
        for (k, v) in m {
            let Some(name) = k.as_str() else { continue };
            let builds = serde_yaml::from_value::<profile::ProfilePatch>(v.clone())
                .ok()
                .is_some_and(|p| profile::patched(name, &p, &shipped).is_ok());
            if builds && !out.iter().any(|o| o == name) {
                out.push(name.to_string());
            }
        }
    }
    Ok(out)
}

/// The models a profile may name: the cached list of the profile's
/// provider, which `ways agent models` writes. No network I/O: a settings
/// load must not wait on a provider. A profile whose provider is not known
/// yet names no list, and the key takes text.
fn model_ids(layers: &[Layer], bound: &[String]) -> Result<Vec<String>, String> {
    model_ids_in(&crate::models::cache_dir(), layers, bound)
}

fn model_ids_in(dir: &std::path::Path, layers: &[Layer], bound: &[String]) -> Result<Vec<String>, String> {
    if bound.is_empty() {
        return Err("the list depends on the profile's provider".into());
    }
    let provider = profile_provider(layers, bound).ok_or("no provider known for this profile")?;
    crate::models::options_in(dir, provider)
}

/// The provider a profile calls: the user layer's, else the shipped profile's.
fn profile_provider(layers: &[Layer], bound: &[String]) -> Option<Provider> {
    let name = bound.first()?;
    let stored = layers
        .iter()
        .filter(|l| l.file == FILE)
        .filter_map(|l| l.accepted.get("profiles")?.get(name.as_str())?.get("provider")?.as_str().map(str::to_string))
        .next_back();
    let provider = stored.or_else(|| shipped_field(bound, "provider")?.as_str().map(str::to_string))?;
    Provider::parse(&provider).ok()
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
        // The key file, which hooks read; a key only in the variable is absent to them.
        .is_some_and(|p| crate::keys::locate_file(p).is_some());
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_settings::Choices;

    fn user(text: &str) -> Vec<Layer> {
        vec![Layer::from_text(&SCHEMA, "user", FILE, LayerScope::User, None, text)]
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ways-settings-models-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_profile_s_models_are_its_providers_cached_list() {
        use crate::models::{write_in, ModelInfo};
        let m = |id: &str| ModelInfo { id: id.into(), name: id.into(), input_per_mtok: None, output_per_mtok: None };
        let dir = scratch("opts");
        let b = |n: &str| vec![n.to_string()];
        let layers = user("profiles:\n  mine:\n    provider: openrouter\n    model: x/y\n");
        // Absent: the reason names the provider's fetch command.
        assert_eq!(
            model_ids_in(&dir, &layers, &b("anthropic")).unwrap_err(),
            "model list not fetched; run `ways agent models --provider anthropic`"
        );
        write_in(&dir, Provider::Anthropic, &[m("claude-sonnet-5-5"), m("claude-haiku-4-5")], 1).unwrap();
        write_in(&dir, Provider::Openrouter, &[m("x/y")], 1).unwrap();
        // The shipped profile's provider, and a user profile's own.
        assert_eq!(model_ids_in(&dir, &layers, &b("anthropic")).unwrap(), ["claude-haiku-4-5", "claude-sonnet-5-5"]);
        assert_eq!(model_ids_in(&dir, &layers, &b("mine")).unwrap(), ["x/y"]);
        // A patch that moves a shipped profile to another provider follows it.
        let moved = user("profiles:\n  anthropic:\n    provider: openrouter\n    model: x/y\n");
        assert_eq!(model_ids_in(&dir, &moved, &b("anthropic")).unwrap(), ["x/y"]);
        // No profile named, or one with no provider: text.
        assert!(model_ids_in(&dir, &layers, &[]).is_err());
        assert!(model_ids_in(&dir, &layers, &b("nosuch")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_model_key_checks_the_shape_when_no_list_is_read() {
        let k = KEYS.iter().find(|k| k.name == "gate.profiles.*.model").unwrap();
        let b = vec!["anthropic".to_string()];
        assert!(k.check_value_in(&Value::from("claude-x-1"), None, &b).is_ok());
        assert!(k.check_value_in(&Value::from("bad id!"), None, &b).is_err());
        assert!(k.check_value_in(&Value::from(3), None, &[]).is_err());
    }

    #[test]
    fn the_engine_is_a_profile_the_shipped_file_or_the_user_layer_names() {
        let engine = KEYS.iter().find(|k| k.name == "gate.engine").unwrap();
        assert_eq!(
            engine.kind.choices(Some(&user("")), &[]),
            Choices::Of { items: vec!["anthropic".into(), "openrouter".into()], multi: false }
        );
        let layers = user("profiles:\n  mine:\n    provider: anthropic\n    model: claude-sonnet-5-5\n  anthropic:\n    threshold: 0.4\n");
        assert_eq!(engine.kind.describe(&layers, &[]), "one of anthropic, openrouter, mine");
        assert!(engine.parse_cli("mine", &layers, &[]).is_ok());
        assert!(engine.parse_cli("openrouter", &layers, &[]).is_ok());
        let e = engine.parse_cli("open-router", &layers, &[]).unwrap_err();
        assert_eq!(e, "expected one of anthropic, openrouter, mine, found 'open-router'");
        // A profile of your own the gate could not build is no choice. A
        // shipped one switched to another provider without a model stays:
        // the bad patch is dropped and it runs as shipped.
        let layers = user("profiles:\n  half:\n    provider: openrouter\n  anthropic:\n    provider: openrouter\n  ok:\n    provider: openrouter\n    model: x/y\n");
        assert_eq!(engine.kind.describe(&layers, &[]), "one of anthropic, openrouter, ok");
        // The hook path loads one file on its own and keeps the value.
        assert!(user("engine: nope\n")[0].findings.is_empty());
    }
}
