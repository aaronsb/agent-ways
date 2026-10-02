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
    let xdg = &from_file.sensors["xdg-downloads"];
    assert!(!xdg.enabled && xdg.script.as_deref().is_some_and(|s| s.ends_with("/attend/sensors/xdg-downloads.sh")));
    for b in BUILTINS {
        assert_eq!(format!("{:?}", from_file.sensors[*b]), format!("{:?}", defaults.sensors[*b]), "{b}");
    }
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
    // Each built-in's keys are listed with no file setting them.
    let names: Vec<String> = reg.concrete("attend.sensors.git", &[]).iter().map(|b| b.name()).collect();
    assert!(names.contains(&"attend.sensors.git.interval".to_string()), "{names:?}");
}

fn sorted(c: &Config) -> Vec<(String, String)> {
    let mut v: Vec<_> = c.sensors.iter().map(|(k, s)| (k.clone(), format!("{s:?}"))).collect();
    v.sort();
    v.push(("governor".into(), format!("{:?} {:?} {:?}", c.governor, c.engagement, c.cleanup)));
    v
}

#[test]
fn the_project_layers_over_the_user_file_key_by_key() {
    let user = layer(LayerScope::User, "governor:\n  base_cooldown: 30\nsensors:\n  git:\n    interval: 45\n    threshold: 3.0\n");
    let project = layer(
        LayerScope::Project,
        "sensors:\n  git:\n    interval: 90\n  processes:\n    enabled: false\n    watch: [mix, zig]\n  disk:\n    script: .claude/sensors/disk.sh\n    requires: [Read]\n",
    );
    let c = Config::from_layers(&[user, project]);
    assert_eq!(c.governor.base_cooldown, Duration::from_secs(30));
    assert_eq!((c.sensors["git"].interval, c.sensors["git"].threshold), (Duration::from_secs(90), 3.0));
    assert!(!c.sensors["processes"].enabled);
    assert_eq!(c.sensors["processes"].watch.as_deref(), Some(&["mix".to_string(), "zig".to_string()][..]));
    assert_eq!(c.sensors["git"].watch, None, "no watch list keeps the sensor's own");
    let disk = &c.sensors["disk"];
    assert_eq!((disk.script.as_deref(), disk.interval, disk.requires.clone()), (Some(".claude/sensors/disk.sh"), Duration::from_secs(60), vec!["Read".to_string()]));
    assert_eq!(c.sensors["context"].requires, vec!["Read".to_string()]);
}

#[test]
fn block_and_flow_lists_read_the_same() {
    let a = layer(LayerScope::User, "sensors:\n  processes:\n    watch:\n      - cargo\n\n      # elixir\n      - mix\n");
    let b = layer(LayerScope::User, "sensors:\n  processes:\n    watch: [cargo, mix]\n");
    assert_eq!(Config::from_layers(&[a]).sensors["processes"].watch, Config::from_layers(&[b]).sensors["processes"].watch);
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
        ("sensors:\n  git:\n    interval: 0\n", "attend.sensors"),
        ("cleanup:\n  interval: 0\n", "attend.cleanup"),
    ] {
        let l = layer(LayerScope::User, text);
        assert_eq!(l.findings.first().and_then(|f| f.section.as_deref()), Some(section), "{text}: {:?}", messages(&l));
    }
    assert_eq!(Config::from_layers(&[layer(LayerScope::User, "sensors:\n  git:\n    interval: 0\n")]).sensors["git"].interval, Duration::from_secs(30));
}

#[test]
fn a_bad_switch_reads_off() {
    let c = Config::from_layers(&[layer(LayerScope::Project, "sensors:\n  git:\n    enabled: maybe\n    interval: 45\ncleanup:\n  enabled: yes please\n")]);
    assert!(!c.sensors["git"].enabled, "a sensor whose switch is bad is off");
    assert_eq!(c.sensors["git"].interval, Duration::from_secs(30), "its other keys fall through");
    assert!(!c.cleanup.enabled, "cleanup's switch fails closed");
}

#[test]
fn a_file_that_does_not_parse_sets_nothing_and_switches_cleanup_off() {
    let l = layer(LayerScope::User, "governor:\n  base_cooldown: 99\nsensors: [\n");
    assert!(l.findings[0].is_parse_failure());
    let c = Config::from_layers(&[l]);
    assert_eq!(c.governor.base_cooldown, Duration::from_secs(15));
    assert!(!c.cleanup.enabled);
}

// ── lint: the forms the hand parser took and the schema does not ──

#[test]
fn old_sensor_prefixes_and_retired_keys_are_findings() {
    let l = layer(
        LayerScope::Project,
        "signals:\n  half_life_seconds: 3600\nengagement:\n  burst_window: 900\n  burst_threshold: 4\ncleanup:\n  retention: 10\nsensors:\n  -processes:\n  +disk:\n    script: ./d.sh\n  git:\n    x-note: mine\n",
    );
    let m = messages(&l);
    let has = |s: &str| m.iter().any(|x| x.contains(s));
    assert!(has("/project.yaml:1: signals: `signals:` was retired"), "{m:?}");
    assert!(has("/project.yaml:4: [attend.engagement] engagement.burst_window: unknown key"), "{m:?}");
    assert!(has("[attend.cleanup] cleanup.retention: unknown key"), "{m:?}");
    assert!(has("/project.yaml:9: [attend.sensors] sensors.-processes: '-processes' is not a sensor name; switch one off with `processes: {enabled: false}`"), "{m:?}");
    assert!(has("'+disk' is not a sensor name; a sensor of your own is `disk:` with a script"), "{m:?}");
    assert!(has("[attend.sensors] sensors.git.x-note: unknown key"), "{m:?}");
    let c = Config::from_layers(&[l]);
    assert_eq!(c.engagement.burst_threshold, 3, "engagement falls back whole");
    assert!(!c.sensors.contains_key("+disk") && !c.sensors.contains_key("disk"), "a script under an old name never runs");
}

#[test]
fn an_old_minus_entry_closes_the_files_sensors_and_reads_nothing_else_there() {
    // `-processes:` was the way to switch a sensor off in a project. The
    // schema cannot name it, so the project's sensors section fails closed:
    // every built-in and every sensor the file names reads off.
    let user = layer(LayerScope::User, "sensors:\n  mine:\n    script: ./m.sh\n");
    let project = layer(LayerScope::Project, "sensors:\n  -processes:\n  git:\n    interval: 90\n  disk:\n    script: ./d.sh\n    enabled: true\n");
    assert!(project.findings.iter().all(|f| f.closed), "{:?}", messages(&project));
    let c = Config::from_layers(&[user, project]);
    for b in BUILTINS {
        assert!(!c.sensors[*b].enabled, "{b} is off");
    }
    assert!(!c.sensors["disk"].enabled, "a sensor the file names is off too");
    assert_eq!(c.sensors["git"].interval, Duration::from_secs(30), "nothing else in the section is read");
    assert!(c.sensors["mine"].enabled, "a sensor of the user file alone is not named here, and runs");
}

#[test]
fn a_file_that_does_not_parse_switches_every_built_in_sensor_off() {
    let l = layer(LayerScope::Project, "sensors:\n  processes:\n    enabled: false\ngovernor: [\n");
    assert!(l.findings[0].is_parse_failure());
    let c = Config::from_layers(&[l]);
    for b in BUILTINS {
        assert!(!c.sensors[*b].enabled, "{b} is off");
    }
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
