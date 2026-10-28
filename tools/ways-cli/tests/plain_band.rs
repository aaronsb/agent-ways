//! A plain report under a theme with a ground is a band: every cell of every
//! row, blank rows included, sits on the theme's ground out to the width.

use std::path::PathBuf;
use std::process::Command;

/// Removes the fixture's root when the test ends.
struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const WIDTH: usize = 100;

#[test]
fn ways_status_under_miami_nights_grounds_every_cell() {
    let root = std::env::temp_dir().join(format!("ways-band-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _guard = Root(root.clone());
    let home = root.join("home");
    let config = home.join(".config/agent-ways");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.yaml"), "theme:\n  active: miami-nights\n").unwrap();

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
    cmd.arg("status")
        .current_dir(&home)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("FORCE_COLOR", "1")
        .env("COLUMNS", WIDTH.to_string());
    for var in ["NO_COLOR", "CLICOLOR_FORCE", "CLAUDE_PROJECT_DIR", "CLAUDE_SESSION_ID", "CLAUDE_CONFIG_DIR"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().expect("run ways");
    assert!(out.status.success(), "ways status failed: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Ways Engine Status"), "{text:?}");

    let grid = tui_harness::sgr::parse(&text);
    let blank = grid.iter().filter(|row| row.iter().all(|c| c.ch == Some(' '))).count();
    assert!(blank > 0, "the report has blank rows to check");
    let mut grounds = std::collections::HashSet::new();
    for (i, row) in grid.iter().enumerate() {
        assert!(!row.is_empty() && row.len() % WIDTH == 0, "row {i} fills {} cells, not a multiple of {WIDTH}", row.len());
        for (col, cell) in row.iter().enumerate() {
            let bg = cell.style.bg.unwrap_or_else(|| panic!("row {i} col {col} has the default ground"));
            grounds.insert(bg);
        }
    }
    assert!(grounds.len() > 2, "the vertical gradient moves down the rows: {grounds:?}");
}
