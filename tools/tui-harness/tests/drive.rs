//! Drive a fixture script through a real tmux session: launch, send a key,
//! assert on the captured text and on one coloured pixel, stop.

use std::path::PathBuf;
use std::time::Duration;

use tui_harness::{sgr::BASIC, tmux_available, Harness, LaunchOptions, Renderer};

#[test]
fn drives_fixture_through_tmux() {
    if !tmux_available() {
        // CI sets this after installing tmux, so a broken install fails
        // instead of passing as a skip.
        if std::env::var_os("TUI_HARNESS_REQUIRE_TMUX").is_some() {
            panic!("TUI_HARNESS_REQUIRE_TMUX is set but tmux is not installed");
        }
        eprintln!("SKIPPED drives_fixture_through_tmux: tmux is not installed");
        return;
    }
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/drive.sh");
    let root = std::env::temp_dir().join(format!("tui-harness-test-{}", std::process::id()));
    let harness = Harness::new(&root);
    let name = format!("it-{}", std::process::id());
    let opts = LaunchOptions {
        cols: 40,
        rows: 6,
        ..LaunchOptions::default()
    };
    let session = harness
        .launch(
            &name,
            &opts,
            &["bash".into(), fixture.display().to_string()],
        )
        .expect("launch");

    let result = std::panic::catch_unwind(|| {
        let before = session
            .wait_for("press a key", Duration::from_secs(10))
            .unwrap();
        assert!(before.starts_with("RED plain"), "pane was:\n{before}");
        // No border status line eats a row: the pane has every requested row.
        assert_eq!(before.lines().count(), 6, "pane was:\n{before}");

        session.send(&["x"]).unwrap();
        let after = session.wait_for("got:x", Duration::from_secs(10)).unwrap();
        assert!(after.contains("got:x"));

        let ansi = session.text(true).unwrap();
        assert!(
            ansi.contains('\x1b'),
            "capture lost its SGR escapes: {ansi:?}"
        );

        // The four blue-background spaces sit at columns 10..14 of row 0.
        let renderer = Renderer::without_fonts(8, 16);
        let img = session.capture_image_with(&renderer).unwrap();
        assert_eq!(img.dimensions(), (40 * 8, 6 * 16));
        assert_eq!(img.get_pixel(11 * 8 + 4, 8).0, BASIC[4]);

        let shot = session.shot(Some(&root.join("fixture.png"))).unwrap();
        assert!(shot.is_file());
    });

    let listed = harness.list().unwrap();
    assert!(listed.iter().any(|s| s.name == name));
    harness.session(&name).unwrap().down().unwrap();
    assert!(harness.list().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&root);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
