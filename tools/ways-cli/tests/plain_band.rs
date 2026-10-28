//! A plain report under a theme with a ground is a band: every cell of every
//! row, blank rows included, sits on the theme's ground out to the width.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Removes the fixture's root when the test ends.
struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const WIDTH: usize = 100;

/// A fixture home whose config makes Miami Nights the active theme.
fn home(tag: &str) -> (PathBuf, Root) {
    let root = std::env::temp_dir().join(format!("ways-band-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = home.join(".config/agent-ways");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.yaml"), "theme:\n  active: miami-nights\n").unwrap();
    (home, Root(root))
}

/// `ways <args>` on a pipe with the fixture home; `styled` forces truecolor.
fn run_raw(home: &Path, args: &[&str], styled: bool) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
    cmd.args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TERM", "xterm-256color")
        .env("COLUMNS", WIDTH.to_string());
    if styled {
        cmd.env("COLORTERM", "truecolor").env("FORCE_COLOR", "1");
    } else {
        cmd.env_remove("COLORTERM").env_remove("FORCE_COLOR");
    }
    for var in ["NO_COLOR", "CLICOLOR_FORCE", "CLAUDE_PROJECT_DIR", "CLAUDE_SESSION_ID", "CLAUDE_CONFIG_DIR"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().expect("run ways");
    assert!(out.status.success(), "ways {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// `ways <args>` at truecolor with colour forced, parsed into cells.
fn run(home: &Path, args: &[&str]) -> tui_harness::Grid {
    tui_harness::sgr::parse(&run_raw(home, args, true))
}

/// Every cell of every row has a ground, each row fills whole rows of the
/// width, and the grounds the rows carry.
fn grounded(args: &[&str], grid: &tui_harness::Grid) -> std::collections::HashSet<[u8; 3]> {
    assert!(!grid.is_empty() && !grid[0].is_empty(), "ways {args:?} printed nothing");
    let mut grounds = std::collections::HashSet::new();
    for (i, row) in grid.iter().enumerate() {
        assert!(!row.is_empty() && row.len() % WIDTH == 0, "ways {args:?} row {i} fills {} cells, not a multiple of {WIDTH}", row.len());
        for (col, cell) in row.iter().enumerate() {
            let bg = cell.style.bg.unwrap_or_else(|| panic!("ways {args:?} row {i} col {col} has the default ground"));
            grounds.insert(bg);
        }
    }
    grounds
}

fn text(grid: &tui_harness::Grid) -> String {
    tui_harness::sgr::plain_text(grid)
}

#[test]
fn ways_status_under_miami_nights_grounds_every_cell() {
    let (home, _root) = home("status");
    let grid = run(&home, &["status"]);
    assert!(text(&grid).contains("Ways Engine Status"), "{}", text(&grid));
    let blank = grid.iter().filter(|row| row.iter().all(|c| c.ch == Some(' '))).count();
    assert!(blank > 0, "the report has blank rows to check");
    let grounds = grounded(&["status"], &grid);
    assert!(grounds.len() > 2, "the vertical gradient moves down the rows: {grounds:?}");
}

#[test]
fn the_session_and_author_reports_are_bands_too() {
    let (home, _root) = home("reports");
    let ways = home.join("ways");
    std::fs::create_dir_all(ways.join("dom/a")).unwrap();
    std::fs::write(ways.join("dom/a/a.md"), "---\ndescription: a way\nvocabulary: alpha beta\n---\n# A\n").unwrap();
    let ways = ways.to_string_lossy().into_owned();
    for (args, says) in [
        (vec!["session", "fires"], "No events recorded yet."),
        (vec!["session", "ways"], "No session markers found."),
        (vec!["session", "reset"], "No session state found."),
        (vec!["author", "tree", "dom", "--ways-dir", &ways], "dom/a"),
        (vec!["author", "tree", "dom", "--jaccard", "--ways-dir", &ways], "dom"),
    ] {
        let grid = run(&home, &args);
        assert!(text(&grid).contains(says), "ways {args:?}: {}", text(&grid));
        grounded(&args, &grid);
    }
}

#[test]
fn the_same_reports_piped_carry_no_escape_and_no_padding() {
    let (home, _root) = home("piped");
    let ways = home.join("ways");
    std::fs::create_dir_all(ways.join("dom/a")).unwrap();
    std::fs::write(ways.join("dom/a/a.md"), "---\ndescription: a way\nvocabulary: alpha beta\n---\n# A\n").unwrap();
    let ways = ways.to_string_lossy().into_owned();
    for args in [
        vec!["session", "fires"],
        vec!["session", "ways"],
        vec!["session", "reset"],
        vec!["author", "tree", "dom", "--ways-dir", &ways],
        vec!["author", "tree", "dom", "--jaccard", "--ways-dir", &ways],
    ] {
        let out = run_raw(&home, &args, false);
        assert!(!out.is_empty(), "ways {args:?} printed nothing");
        assert!(!out.contains('\x1b'), "ways {args:?} piped carries an escape: {out:?}");
        assert!(out.lines().all(|l| l.chars().count() <= WIDTH && l == l.trim_end()), "ways {args:?} piped is padded: {out:?}");
    }
}
