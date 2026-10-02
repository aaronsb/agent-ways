//! `ways agent use` and `mode` write agent.yaml through the settings writer
//! (ADR-503 §6, §9). Their stdout, stderr and exit codes are pinned to what
//! the commit before that change printed (captured at cda1042b), and the
//! file they leave parses to the same settings. A comment in the file now
//! survives them.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("ways-agent-alias-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        Fixture { root }
    }

    fn agent_yaml(&self) -> PathBuf {
        self.root.join("xdg/config/agent-ways/agent.yaml")
    }

    fn run(&self, args: &[&str]) -> (String, String, i32) {
        let out = Command::new(env!("CARGO_BIN_EXE_ways-agent"))
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("xdg/config"))
            .env("XDG_STATE_HOME", self.root.join("xdg/state"))
            .env("XDG_CACHE_HOME", self.root.join("xdg/cache"))
            .env("XDG_DATA_HOME", self.root.join("xdg/data"))
            .output()
            .unwrap();
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code().unwrap_or(-1),
        )
    }

    fn parsed(&self) -> serde_yaml::Value {
        let text = std::fs::read_to_string(self.agent_yaml()).unwrap_or_default();
        match serde_yaml::from_str(&text).unwrap() {
            serde_yaml::Value::Null => serde_yaml::Value::Mapping(Default::default()),
            v => v,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn y(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

#[test]
fn use_and_mode_print_and_exit_as_before_and_write_the_same_settings() {
    let f = Fixture::new("seq");
    let cases: &[(&[&str], &str, &str, i32, &str)] = &[
        (&["mode", "shadow"], "mode shadow: every candidate is judged and logged; the matcher still decides\n", "", 0, "{mode: shadow}"),
        (&["mode", "bogus"], "", "ways agent: unknown mode 'bogus' (expected enforce, shadow or off)\n", 1, "{mode: shadow}"),
        (&["mode", "enforce"], "mode enforce: ways judged irrelevant are not injected\n", "", 0, "{}"),
        (
            &["use", "anthropic"],
            "engine: anthropic (anthropic claude-haiku-4-5), mode enforce\nno anthropic key yet; run `ways agent key add --provider anthropic`\n",
            "",
            0,
            "{engine: anthropic}",
        ),
        (
            &["use", "anthropic", "--model", "claude-sonnet-5-5"],
            "engine: anthropic (anthropic claude-sonnet-5-5), mode enforce\nnote: threshold 0.3 was tuned for claude-haiku-4-5. claude-sonnet-5-5 scores on its own scale, and slower or costlier models make every gated prompt wait longer.\nno anthropic key yet; run `ways agent key add --provider anthropic`\n",
            "",
            0,
            "{engine: anthropic, profiles: {anthropic: {model: claude-sonnet-5-5}}}",
        ),
        (&["use", "nope"], "", "ways agent: engine 'nope' names no profile\n", 1, "{engine: anthropic, profiles: {anthropic: {model: claude-sonnet-5-5}}}"),
        (
            &["use", "anthropic"],
            "engine: anthropic (anthropic claude-haiku-4-5), mode enforce\nno anthropic key yet; run `ways agent key add --provider anthropic`\n",
            "",
            0,
            "{engine: anthropic}",
        ),
    ];
    for (args, out, err, code, file) in cases {
        let got = f.run(args);
        assert_eq!(got, (out.to_string(), err.to_string(), *code), "ways-agent {}", args.join(" "));
        assert_eq!(f.parsed(), y(file), "agent.yaml after ways-agent {}", args.join(" "));
    }
}

#[test]
fn mode_and_use_keep_comments_and_keys_they_do_not_set() {
    let f = Fixture::new("keep");
    let path = f.agent_yaml();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let src = "# my gate settings\nengine: anthropic  # picked by hand\nprofiles:\n  anthropic:\n    threshold: 0.4  # tuned on my prompts\n";
    std::fs::write(&path, src).unwrap();
    assert_eq!(f.run(&["mode", "shadow"]).2, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{src}mode: shadow\n"));
    assert_eq!(f.run(&["use", "anthropic", "--model", "claude-sonnet-5-5"]).2, 0);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# my gate settings\nengine: anthropic  # picked by hand\nprofiles:\n  anthropic:\n    threshold: 0.4  # tuned on my prompts\n    model: claude-sonnet-5-5\nmode: shadow\n"
    );
    assert!(no_stray_files(path.parent().unwrap()));
}

fn no_stray_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().flatten().all(|e| e.file_name() == "agent.yaml")
}
