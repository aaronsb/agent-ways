//! `ways corpus` against a stub way-embed: what reaches stderr about the body
//! sidecar under `--quiet`, and that the manifest is replaced, not written
//! through. The unit tests in `cmd/corpus.rs` cover the pieces; these drive
//! the real command so the wiring between them is covered too.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const NO_VECTORS_WARNING: &str = "warning: body sidecar not built: way-embed < 1.2.0 lacks --vectors";

struct Fx {
    root: PathBuf,
}

impl Fx {
    fn new(name: &str, version_out: &str) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-corpus-wiring-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fx = Fx { root };
        for d in ["config", "state", "runtime", "proj", "ways/dev/one", "out"] {
            std::fs::create_dir_all(fx.root.join(d)).unwrap();
        }
        let engine = fx.engine();
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::write(engine.join("minilm-l6-v2.gguf"), "model").unwrap();
        // The stub prints `version_out` for --version (exit 1 when it is
        // empty) and accepts `generate` without touching the staged files.
        let bin = engine.join("way-embed");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n  --version) [ -n '{version_out}' ] || exit 1; echo '{version_out}' ;;\n  generate) exit 0 ;;\n  *) exit 2 ;;\nesac\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            fx.root.join("ways/dev/one/one.md"),
            "---\ndescription: the one way\nvocabulary: giraffe\nrefire: 0.15\n---\n# One\n\nA section long enough to embed here.\n",
        )
        .unwrap();
        fx
    }

    fn engine(&self) -> PathBuf {
        self.root.join("cache/agent-ways/user")
    }

    fn out(&self) -> PathBuf {
        self.root.join("out")
    }

    fn manifest(&self) -> PathBuf {
        self.out().join("embed-manifest.json")
    }

    /// Run `ways corpus` with `flags`; (exit ok, stderr).
    fn corpus(&self, flags: &[&str]) -> (bool, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_ways"))
            .arg("corpus")
            .arg("--ways-dir")
            .arg(self.root.join("ways"))
            .arg("--output")
            .arg(self.out())
            .args(flags)
            .current_dir(self.root.join("proj"))
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .output()
            .expect("run ways corpus");
        (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    /// Make the manifest a symlink to a copy, so a write through the path
    /// changes the copy and a replacement leaves it alone. Returns the copy.
    fn symlink_manifest(&self) -> PathBuf {
        let target = self.root.join("manifest-target.json");
        std::fs::rename(self.manifest(), &target).unwrap();
        std::os::unix::fs::symlink(&target, self.manifest()).unwrap();
        target
    }

    /// Pretend a sidecar built by another engine sits beside the corpus, so
    /// `--if-stale` rebuilds the sidecar alone.
    fn plant_foreign_sidecar(&self) {
        let mut m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(self.manifest()).unwrap()).unwrap();
        m["body_sidecar"] = serde_json::json!({ "file": "ways-body-en.bin", "vectors": true, "model_id": "another-engine" });
        std::fs::write(self.manifest(), serde_json::to_string(&m).unwrap()).unwrap();
        std::fs::write(self.out().join("ways-body-en.bin"), "old sidecar").unwrap();
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn is_plain_file(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_type().is_file()).unwrap_or(false)
}

#[test]
fn a_full_build_is_silent_about_an_old_way_embed_under_quiet() {
    let fx = Fx::new("full-quiet", "way-embed 1.1.2");
    let (ok, err) = fx.corpus(&["--quiet"]);
    assert!(ok, "{err}");
    assert_eq!(err, "", "--quiet printed on an install state");
}

#[test]
fn a_full_build_still_warns_about_an_old_way_embed_without_quiet() {
    let fx = Fx::new("full-loud", "way-embed 1.1.2");
    let (ok, err) = fx.corpus(&[]);
    assert!(ok, "{err}");
    assert!(err.contains(NO_VECTORS_WARNING), "{err}");
}

#[test]
fn a_full_build_warns_under_quiet_when_the_way_embed_version_cannot_be_read() {
    for out in ["", "garbage"] {
        let fx = Fx::new("full-unreadable", if out.is_empty() { "" } else { out });
        let (ok, err) = fx.corpus(&["--quiet"]);
        assert!(ok, "{err}");
        assert!(err.contains("warning: body sidecar not built: way-embed --version"), "{out:?}: {err}");
    }
}

#[test]
fn a_sidecar_only_rebuild_is_silent_about_an_old_way_embed_under_quiet() {
    let fx = Fx::new("side-quiet", "way-embed 1.1.2");
    assert!(fx.corpus(&["--quiet"]).0);
    fx.plant_foreign_sidecar();
    let (ok, err) = fx.corpus(&["--quiet", "--if-stale"]);
    assert!(ok, "{err}");
    assert_eq!(err, "", "the sidecar-only path printed under --quiet");
    assert!(!fx.out().join("ways-body-en.bin").exists(), "the stale sidecar was not removed: nothing ran");
}

#[test]
fn a_sidecar_only_rebuild_warns_without_quiet_and_does_not_claim_a_rebuild() {
    let fx = Fx::new("side-loud", "way-embed 1.1.2");
    assert!(fx.corpus(&["--quiet"]).0);
    fx.plant_foreign_sidecar();
    let (ok, err) = fx.corpus(&["--if-stale"]);
    assert!(ok, "{err}");
    assert!(err.contains(NO_VECTORS_WARNING), "{err}");
    assert!(!err.contains("Body sidecar rebuilt"), "claimed a sidecar that was just removed:\n{err}");
}

#[test]
fn a_full_build_replaces_the_manifest_instead_of_writing_through_it() {
    let fx = Fx::new("full-atomic", "way-embed 1.1.2");
    assert!(fx.corpus(&["--quiet"]).0);
    let target = fx.symlink_manifest();
    let before = std::fs::read_to_string(&target).unwrap();
    let (ok, err) = fx.corpus(&["--quiet"]);
    assert!(ok, "{err}");
    assert!(is_plain_file(&fx.manifest()), "the manifest was written through its link");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), before);
    let names: Vec<String> =
        std::fs::read_dir(fx.out()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "staging left behind: {names:?}");
}

#[test]
fn a_sidecar_only_rebuild_replaces_the_manifest_instead_of_writing_through_it() {
    let fx = Fx::new("side-atomic", "way-embed 1.1.2");
    assert!(fx.corpus(&["--quiet"]).0);
    fx.plant_foreign_sidecar();
    let target = fx.symlink_manifest();
    let before = std::fs::read_to_string(&target).unwrap();
    let (ok, err) = fx.corpus(&["--quiet", "--if-stale"]);
    assert!(ok, "{err}");
    assert!(is_plain_file(&fx.manifest()), "the manifest was written through its link");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), before);
}
