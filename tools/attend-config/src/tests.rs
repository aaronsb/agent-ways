use super::*;
use agent_settings::Registry;
use std::time::Duration;

fn layer(scope: LayerScope, text: &str) -> Layer {
    let name = if scope == LayerScope::User { "user" } else { "project" };
    Layer::from_text(&SCHEMA, name, FILE, scope, Some(Path::new(&format!("/{name}.yaml"))), text)
}

fn messages(l: &Layer) -> Vec<String> {
    l.findings.iter().map(|f| f.to_string()).collect()
}

// ── parse and validate round trip ──────────────────────────────

#[test]
fn the_default_file_lints_clean_and_loads_the_defaults() {
    let l = layer(LayerScope::User, &default_yaml());
    assert!(l.findings.is_empty(), "{:?}", messages(&l));
    let (from_file, defaults) = (Config::from_layers(&[l]), Config::default());
    assert_eq!(format!("{:?}", from_file.governor), format!("{:?}", defaults.governor));
    assert_eq!(format!("{:?}", from_file.engagement), format!("{:?}", defaults.engagement));
    assert!(from_file.project_sensors.is_empty());
}

#[test]
fn the_canonical_fragment_parses_back_to_the_defaults() {
    let reg = Registry::new(vec![&SCHEMA]);
    let emitted = reg.emit("", None);
    assert_eq!(emitted.len(), 1);
    let text = serde_yaml::to_string(&emitted[0].1).unwrap();
    let l = layer(LayerScope::User, &text);
    assert!(l.findings.is_empty(), "{text}\n{:?}", messages(&l));
    assert_eq!(format!("{:?}", sorted(&Config::from_layers(&[l]))), format!("{:?}", sorted(&Config::default())));
}

fn sorted(c: &Config) -> Vec<(String, String)> {
    let mut v = vec![("projects".to_string(), format!("{:?}", c.project_sensors))];
    v.push(("governor".into(), format!("{:?} {:?} {:?}", c.governor, c.engagement, c.cleanup)));
    v
}

// ── validation: a bad section falls back, a switch fails closed ──

#[test]
fn a_bad_value_falls_back_its_section_and_names_the_line() {
    let l = layer(LayerScope::User, "governor:\n  base_cooldown: abc\n  rate_window: 60\nengagement:\n  decay_per_minute: 0.05\n");
    let c = Config::from_layers(std::slice::from_ref(&l));
    assert_eq!(c.governor.rate_window, Duration::from_secs(120), "the whole governor section falls back");
    assert_eq!(c.engagement.decay_per_minute, 0.05, "engagement loads as written");
    let f = &l.findings[0];
    assert_eq!((f.line, f.section.as_deref(), f.fallback), (Some(2), Some("attend.governor"), true));
    assert!(f.diagnostic("attend").contains("`ways settings fix attend.governor` repairs it"), "{}", f.diagnostic("attend"));
}

#[test]
fn out_of_range_values_are_findings_not_clamped() {
    for (text, section) in [
        ("engagement:\n  decay_per_minute: -5\n", "attend.engagement"),
        ("engagement:\n  step_multiplier: 1000\n", "attend.engagement"),
        ("cleanup:\n  interval: 0\n", "attend.cleanup"),
    ] {
        let l = layer(LayerScope::User, text);
        assert_eq!(l.findings.first().and_then(|f| f.section.as_deref()), Some(section), "{text}: {:?}", messages(&l));
    }
}

#[test]
fn a_bad_switch_reads_off() {
    let c = Config::from_layers(&[layer(LayerScope::Project, "cleanup:\n  enabled: yes please\n")]);
    assert!(!c.cleanup.enabled, "cleanup's switch fails closed");
}

#[test]
fn a_file_that_does_not_parse_sets_nothing_and_switches_cleanup_off() {
    let l = layer(LayerScope::User, "governor:\n  base_cooldown: 99\ncleanup: [\n");
    assert!(l.findings[0].is_parse_failure());
    let c = Config::from_layers(&[l]);
    assert_eq!(c.governor.base_cooldown, Duration::from_secs(15));
    assert!(!c.cleanup.enabled);
}

// ── the retired sensors section (ADR-403 §7) ─────────────────

#[test]
fn each_old_sensor_key_names_the_file_and_field_to_write() {
    let l = layer(
        LayerScope::User,
        "signals:\n  half_life_seconds: 3600\nsensors:\n  git:\n    interval: 45\n    enabled: false\n  disk:\n    script: ~/d.sh\n    interval: 120\n",
    );
    let m = messages(&l);
    let want = [
        "/user.yaml:1: signals: `signals:` was retired",
        "/user.yaml:5: sensors.git.interval: retired by ADR-403; write `interval: 45` in $XDG_CONFIG_HOME/attend/sensors/git.yaml",
        "/user.yaml:6: sensors.git.enabled: retired by ADR-403; write `enabled: false` in $XDG_CONFIG_HOME/attend/sensors/git.yaml",
        "/user.yaml:8: sensors.disk.script: the inline script form was retired by ADR-403; write $XDG_CONFIG_HOME/attend/sensors/disk.yaml with `kind: command`, `argv: [bash, ~/d.sh]` and `output: lines`",
        "/user.yaml:9: sensors.disk.interval: retired by ADR-403; write `interval: 120` in $XDG_CONFIG_HOME/attend/sensors/disk.yaml, the command sensor that replaces the script",
    ];
    for w in want {
        assert!(m.iter().any(|x| x.starts_with(w)), "{w}\n{m:?}");
    }
    assert!(l.findings.iter().all(|f| !f.fallback), "a retired key only reports");
}

#[test]
fn a_project_file_s_old_keys_name_the_project_s_sensor_file() {
    let m = messages(&layer(LayerScope::Project, "sensors:\n  -processes:\n  git:\n    threshold: 3.0\n"));
    assert!(m.iter().any(|x| x.contains("sensors.-processes: retired by ADR-403; write `enabled: false` in .claude/attend/sensors/processes.yaml")), "{m:?}");
    assert!(m.iter().any(|x| x.contains("sensors.git.threshold: retired by ADR-403; write `threshold: 3.0` in .claude/attend/sensors/git.yaml")), "{m:?}");
}

#[test]
fn project_sensors_is_the_user_file_s_alone() {
    let c = Config::from_layers(&[layer(LayerScope::User, "project_sensors: [~/p, /q]\n")]);
    assert_eq!(c.project_sensors, vec!["~/p".to_string(), "/q".to_string()]);
    let l = layer(LayerScope::Project, "project_sensors: [/q]\n");
    assert!(messages(&l).iter().any(|x| x.contains("user file only")), "{:?}", messages(&l));
    assert!(Config::from_layers(&[l]).project_sensors.is_empty());
}

#[test]
fn max_per_window_zero_is_a_mute() {
    let l = layer(LayerScope::User, "governor:\n  max_per_window: 0\n");
    assert!(l.findings.is_empty(), "{:?}", messages(&l));
    assert_eq!(Config::from_layers(&[l]).governor.max_per_window, 0);
}

#[test]
fn attend_keeps_no_theme_key() {
    assert!(SCHEMA.keys.iter().all(|k| !k.name.contains("theme")), "the one theme is ways' theme.active");
    assert!(SCHEMA.keys.iter().all(|k| k.name.starts_with("attend.")) && SCHEMA.sections.iter().all(|s| s.name.starts_with("attend.")));
}

// ── the writer ─────────────────────────────────────────────────

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("attend-config-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn write_changes_only_its_keys_and_refuses_a_bad_value() {
    let d = tmp("write");
    let p = d.join("config.yaml");
    let src = "# mine\nengagement:\n  burst_threshold: 5   # by hand\n  decay_per_minute: 0.1 # tuned\n# end\n";
    std::fs::write(&p, src).unwrap();
    assert!(write(&p, &[("attend.engagement.decay_per_minute", Value::from(0.0256))]).unwrap());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), src.replace("0.1 # tuned", "0.0256 # tuned"));
    let e = write(&p, &[("attend.engagement.decay_per_minute", Value::from(7.0))]).unwrap_err();
    assert!(e.contains("outside") && e.ends_with("nothing written"), "{e}");
    assert!(write(&p, &[("attend.nope", Value::from(1))]).is_err());
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn init_never_overwrites() {
    let d = tmp("init");
    let p = d.join("attend/config.yaml");
    assert!(init(&p).unwrap());
    std::fs::write(&p, "# mine\n").unwrap();
    assert!(!init(&p).unwrap());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "# mine\n");
    std::fs::remove_dir_all(&d).ok();
}
