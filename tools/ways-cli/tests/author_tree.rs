//! `ways author tree --ways-dir`: a contributor points the command at a
//! checkout's ways, and every name prints relative to that tree rather than
//! as a truncated absolute path.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Fx {
    root: PathBuf,
}

impl Fx {
    /// A checkout-shaped `hooks/ways` with a parent way and two children.
    fn new(name: &str) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-author-tree-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home", "config", "cache", "state", "share", "runtime"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let fx = Fx { root };
        Self::way(&fx.ways(), "prose", "draft prose outline");
        Self::way(&fx.ways(), "prose/editing", "edit prose revise");
        Self::way(&fx.ways(), "prose/tone", "tone voice prose");
        fx
    }

    fn ways(&self) -> PathBuf {
        self.root.join("checkout/hooks/ways")
    }

    fn way(root: &Path, id: &str, vocab: &str) {
        let leaf = id.rsplit('/').next().unwrap();
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let text = format!("---\ndescription: the {leaf} way\nvocabulary: {vocab}\nthreshold: 2.0\n---\n# {leaf}\n");
        std::fs::write(dir.join(format!("{leaf}.md")), text).unwrap();
    }

    /// Run the binary in `cwd` with an empty home; stdout and stderr together.
    fn run(&self, cwd: &Path, args: &[&str]) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_ways"))
            .args(["author", "tree"])
            .args(args)
            .current_dir(cwd)
            .env("NO_COLOR", "1")
            // The output is piped, so the table fits to $COLUMNS.
            .env("COLUMNS", "80")
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_DATA_HOME", self.root.join("share"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .output()
            .expect("run ways");
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn assert_relative(out: &str, root: &Path) {
    assert!(!out.contains(root.to_str().unwrap()), "names must not carry the absolute path:\n{out}");
    assert!(!out.contains('…'), "names must not be truncated:\n{out}");
}

#[test]
fn tree_prints_names_relative_to_the_ways_dir() {
    let fx = Fx::new("tree");
    let out = fx.run(&fx.root, &["--ways-dir", fx.ways().to_str().unwrap(), "prose"]);
    assert!(out.contains("(3 files)"), "{out}");
    for name in ["prose/editing", "prose/tone"] {
        assert!(out.contains(name), "missing {name}:\n{out}");
    }
    let rows: Vec<&str> = out.lines().filter_map(|l| l.split_whitespace().next()).collect();
    // The last match: the heading also starts with the tree's name.
    let at = |name: &str| rows.iter().rposition(|r| *r == name).unwrap_or_else(|| panic!("no {name} row:\n{out}"));
    assert!(at("prose") < at("prose/editing"), "the parent row comes first:\n{out}");
    assert_relative(&out, &fx.root);
}

#[test]
fn jaccard_prints_the_sibling_pair_by_relative_name() {
    let fx = Fx::new("jaccard");
    let out = fx.run(&fx.root, &["--ways-dir", fx.ways().to_str().unwrap(), "prose", "--jaccard"]);
    let pair = out
        .lines()
        .find(|l| l.contains("prose/editing") && l.contains("prose/tone"))
        .unwrap_or_else(|| panic!("missing the sibling pair:\n{out}"));
    // {edit prose revise} and {tone voice prose} share one of five words.
    assert!(pair.contains("0.200"), "{pair}");
    assert_relative(&out, &fx.root);
}

#[test]
fn jaccard_prints_the_value_in_full_beside_long_names() {
    let fx = Fx::new("long");
    let parent = "prose/a-rather-long-category-name";
    Fx::way(&fx.ways(), parent, "long names");
    Fx::way(&fx.ways(), &format!("{parent}/the-first-child-with-a-long-name"), "alpha beta gamma");
    Fx::way(&fx.ways(), &format!("{parent}/the-second-child-with-a-long-name"), "alpha delta");
    let out = fx.run(&fx.root, &["--ways-dir", fx.ways().to_str().unwrap(), parent, "--jaccard"]);
    let pair = out
        .lines()
        .find(|l| l.contains("0.") || l.contains("0…"))
        .unwrap_or_else(|| panic!("no pair row:\n{out}"));
    // {alpha beta gamma} and {alpha delta} share one of four words.
    assert!(pair.trim_end().ends_with(" 0.250"), "the value must print in full: {pair:?}\n{out}");
}

#[test]
fn a_short_name_resolves_under_a_relative_ways_dir() {
    let fx = Fx::new("short");
    let out = fx.run(&fx.root.join("checkout"), &["--ways-dir", "hooks/ways", "tone"]);
    assert!(out.contains("prose/tone") && out.contains("(1 files)"), "{out}");
    assert_relative(&out, &fx.root);
}

#[test]
fn a_tree_outside_the_ways_root_prints_relative_to_its_own_hooks_ways() {
    let fx = Fx::new("outside");
    // No --ways-dir: the empty home has no projection, so the tree is named
    // against its own hooks/ways.
    let out = fx.run(&fx.root, &[fx.ways().join("prose").to_str().unwrap()]);
    assert!(out.contains("prose/editing") && out.contains("prose/tone"), "{out}");
    assert_relative(&out, &fx.root);
}

#[test]
fn a_missing_ways_dir_is_an_error() {
    let fx = Fx::new("missing");
    let out = fx.run(&fx.root, &["--ways-dir", "nowhere", "prose"]);
    assert!(out.contains("--ways-dir is not a directory"), "{out}");
}
