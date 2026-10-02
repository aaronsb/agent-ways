//! Scene management for attend (ADR-118).
//!
//! A scene is a named preset that configures channel membership.
//! Scenes live in `~/.config/attend/scenes.yaml`.
//!
//! Built-in defaults:
//!   private — leave all channels (project scope only)
//!
//! `open` used to be a scene preset that joined an `@open/` group;
//! ADR-124 folded it into the `#open` base channel (which everyone
//! is always in implicitly), so the preset was removed.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::groups::Groups;

/// A scene definition.
#[derive(Debug, Clone)]
pub struct Scene {
    pub channels: Vec<String>,
}

/// Load scenes from config file. Returns built-in defaults merged with user config.
pub fn load_scenes() -> HashMap<String, Scene> {
    let mut scenes = HashMap::new();

    // Built-in defaults. `open` used to live here; it's now the
    // `#open` base channel (ADR-124) — implicit for every peer.
    scenes.insert("private".to_string(), Scene { channels: Vec::new() });

    // User config overlay
    let path = scenes_config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        for (name, scene) in parse_scenes_yaml(&content) {
            scenes.insert(name, scene);
        }
    }

    scenes
}

/// Activate a scene — reconfigure channel membership to match the preset.
pub fn activate(scene_name: &str, groups: &Groups) -> Result<String, String> {
    let scenes = load_scenes();
    let scene = scenes
        .get(scene_name)
        .ok_or_else(|| format!("unknown scene '{scene_name}' — try: {}",
            scenes.keys().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")))?;

    // Leave all current named channels
    for (name, _) in groups.my_groups() {
        groups.leave(&name).ok();
    }

    // Join the scene's channels
    for channel_name in &scene.channels {
        groups.join(channel_name, false)?;
    }

    if scene.channels.is_empty() {
        Ok("project scope only".to_string())
    } else {
        Ok(format!("joined: {}", scene.channels.join(", ")))
    }
}

fn scenes_config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("attend").join("scenes.yaml")
}

/// Parse scenes.yaml. Format:
/// ```yaml
/// private:
///   channels: []
/// workroom:
///   channels: [deploy, infra]
/// ```
fn parse_scenes_yaml(content: &str) -> HashMap<String, Scene> {
    let mut scenes = HashMap::new();
    let mut current_name: Option<String> = None;
    let mut current_channels: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Top-level: scene name
        if indent == 0 && trimmed.ends_with(':') {
            if let Some(ref name) = current_name {
                scenes.insert(name.clone(), Scene { channels: current_channels.clone() });
            }
            current_name = Some(trimmed.trim_end_matches(':').to_string());
            current_channels = Vec::new();
            continue;
        }

        // Second-level: channels key with inline array or list items
        if indent == 2 {
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                if key == "channels" {
                    // Inline array: channels: [deploy, infra]
                    if value.starts_with('[') && value.ends_with(']') {
                        let inner = &value[1..value.len() - 1];
                        current_channels = inner
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    } else if value == "[]" {
                        current_channels = Vec::new();
                    }
                    // else: list items follow at indent 4
                }
            }
        }

        // Third-level: list items
        if indent == 4 {
            if let Some(channel) = trimmed.strip_prefix("- ") {
                current_channels.push(channel.trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }

    // Save last scene
    if let Some(ref name) = current_name {
        scenes.insert(name.clone(), Scene { channels: current_channels });
    }

    scenes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_scenes() {
        let yaml = r#"
private:
  channels: []
workroom:
  channels: [deploy, infra]
custom:
  channels:
    - alpha
    - beta
"#;
        let scenes = parse_scenes_yaml(yaml);
        assert_eq!(scenes.len(), 3);
        assert!(scenes["private"].channels.is_empty());
        assert_eq!(scenes["workroom"].channels, vec!["deploy", "infra"]);
        assert_eq!(scenes["custom"].channels, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_builtins() {
        let scenes = load_scenes();
        assert!(scenes.contains_key("private"));
        assert!(scenes["private"].channels.is_empty());
        // `open` is no longer a built-in scene (ADR-124 §2). The
        // equivalent — everyone in #open — is now the implicit base.
        assert!(!scenes.contains_key("open"));
    }
}
