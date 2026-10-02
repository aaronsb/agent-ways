//! Removed legacy CLI surface stays removed (#692, ADR-505).
//!
//! `ways embed` was an alias for `ways match`, and `ways match --cosine` showed
//! a single-vector view that no longer reflects the fire path. Both are gone;
//! clap must reject them rather than accept and ignore them.

use std::process::Command;

fn ways(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ways"))
        .args(args)
        .output()
        .expect("run ways")
}

#[test]
fn embed_subcommand_is_rejected() {
    let out = ways(&["embed", "some query"]);
    assert!(!out.status.success(), "`ways embed` must be rejected");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unrecognized subcommand"), "stderr: {err}");
}

#[test]
fn match_cosine_flag_is_rejected() {
    let out = ways(&["match", "--cosine", "some query"]);
    assert!(!out.status.success(), "`ways match --cosine` must be rejected");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unexpected argument"), "stderr: {err}");
}

#[test]
fn match_corpus_flag_is_rejected() {
    // `--corpus` only fed the removed single-vector view.
    let out = ways(&["match", "--corpus", "/nonexistent.jsonl", "some query"]);
    assert!(!out.status.success(), "`ways match --corpus` must be rejected");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unexpected argument"), "stderr: {err}");
}
