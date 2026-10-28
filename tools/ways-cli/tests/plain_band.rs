//! A plain report under a theme with a ground is a band: every cell of every
//! row, blank rows included, sits on the theme's ground out to the width.
//! Only a terminal installs the theme; forced colour on a pipe keeps the
//! default painter and reads no theme file (ADR-504 §11).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Removes the fixture's root when the test ends.
struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const WIDTH: usize = 100;

/// A fixture home whose config makes `theme` the active theme.
fn home_with(tag: &str, theme: &str) -> (PathBuf, Root) {
    let root = std::env::temp_dir().join(format!("ways-band-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = home.join(".config/agent-ways");
    std::fs::create_dir_all(config.join("themes")).unwrap();
    std::fs::write(config.join("config.yaml"), format!("theme:\n  active: {theme}\n")).unwrap();
    (home, Root(root))
}

/// A fixture home whose config makes Miami Nights the active theme.
fn home(tag: &str) -> (PathBuf, Root) {
    home_with(tag, "miami-nights")
}

/// `cmd` in the fixture home, with nothing of this session's environment
/// that would reach ways' settings or colour.
fn fixture<'a>(cmd: &'a mut Command, home: &Path) -> &'a mut Command {
    cmd.current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("TERM", "xterm-256color")
        .env("COLUMNS", WIDTH.to_string())
        .env("SHELL", "/bin/sh")
        .env_remove("COLORTERM")
        .stdin(Stdio::null());
    for var in ["NO_COLOR", "FORCE_COLOR", "CLICOLOR_FORCE", "CLAUDE_PROJECT_DIR", "CLAUDE_SESSION_ID", "CLAUDE_CONFIG_DIR"] {
        cmd.env_remove(var);
    }
    cmd
}

/// `ways <args>` on a pipe with the fixture home; `forced` sets truecolor
/// and `FORCE_COLOR`.
fn run_raw(home: &Path, args: &[&str], forced: bool) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
    fixture(cmd.args(args), home);
    if forced {
        cmd.env("COLORTERM", "truecolor").env("FORCE_COLOR", "1");
    }
    let out = cmd.output().expect("run ways");
    assert!(out.status.success(), "ways {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Whether util-linux `script` is here to give a command a terminal.
fn have_script() -> bool {
    let ok = Command::new("script").args(["-qec", "true", "/dev/null"]).stdin(Stdio::null()).output().is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("skipped: util-linux `script` is not available to give ways a terminal");
    }
    ok
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `ways <args>` on a terminal `WIDTH` columns wide at truecolor, through
/// util-linux `script`, ready to spawn.
fn tty(home: &Path, args: &[&str]) -> Command {
    let line: Vec<String> = std::iter::once(env!("CARGO_BIN_EXE_ways")).chain(args.iter().copied()).map(quote).collect();
    let mut cmd = Command::new("script");
    cmd.args(["-qec", &format!("stty cols {WIDTH} rows 50; exec {}", line.join(" ")), "/dev/null"]);
    fixture(&mut cmd, home).env("COLORTERM", "truecolor");
    cmd
}

/// What `ways <args>` writes to a terminal, the terminal's CRLF read back
/// as LF.
fn run_tty(home: &Path, args: &[&str]) -> String {
    let out = tty(home, args).output().expect("run script");
    assert!(out.status.success(), "ways {args:?} failed: {}", String::from_utf8_lossy(&out.stdout));
    String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n")
}

/// `ways <args>` on a terminal, parsed into cells.
fn run(home: &Path, args: &[&str]) -> tui_harness::Grid {
    tui_harness::sgr::parse(&run_tty(home, args))
}

/// Whether `cmd` exits within `limit`; killed when it does not.
fn exits_within(mut cmd: Command, limit: Duration) -> bool {
    let mut child = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("spawn");
    let start = Instant::now();
    while start.elapsed() < limit {
        if child.try_wait().expect("wait").is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    false
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
    if !have_script() {
        return;
    }
    let (home, _root) = home("status");
    let grid = run(&home, &["status"]);
    assert!(text(&grid).contains("Ways Engine Status"), "{}", text(&grid));
    assert!(text(&grid).contains(&"─".repeat(18)) && !text(&grid).contains("=="), "the heading's underline is the theme's rule: {}", text(&grid));
    let blank = grid.iter().filter(|row| row.iter().all(|c| c.ch == Some(' '))).count();
    assert!(blank > 0, "the report has blank rows to check");
    let grounds = grounded(&["status"], &grid);
    assert!(grounds.len() > 2, "the vertical gradient moves down the rows: {grounds:?}");
}

#[test]
fn the_session_and_author_reports_are_bands_too() {
    if !have_script() {
        return;
    }
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

#[test]
fn forced_colour_on_a_pipe_paints_no_band() {
    let (home, _root) = home("forced");
    let out = run_raw(&home, &["status"], true);
    assert!(out.contains("Ways Engine Status\n==================\n"), "the default painter keeps the ASCII underline: {out:?}");
    assert!(!out.contains("48;"), "forced colour paints no ground: {out:?}");
    assert!(out.lines().all(|l| l == l.trim_end()), "and pads nothing: {out:?}");
}

#[test]
fn a_piped_hook_command_under_forced_colour_reads_no_theme() {
    // The active theme's file is a FIFO: opening it to read blocks until a
    // writer comes, so a command that reads the theme never exits.
    let (home, _root) = home_with("hook", "probe");
    let fifo = home.join(".config/agent-ways/themes/probe.theme");
    let made = Command::new("mkfifo").arg(&fifo).status().expect("mkfifo");
    assert!(made.success(), "mkfifo {}", fifo.display());
    let way = home.join(".claude/ways/dom/a");
    std::fs::create_dir_all(&way).unwrap();
    std::fs::write(way.join("a.md"), "---\ndescription: a way\nvocabulary: alpha beta\nrefire: 0.5\n---\n# A\n\nbody\n").unwrap();
    let show = |session: &str| ["show", "way", "dom/a", "--session", session].map(String::from);

    let mut forced = Command::new(env!("CARGO_BIN_EXE_ways"));
    fixture(forced.args(show("forced-exits")), &home).env("COLORTERM", "truecolor").env("FORCE_COLOR", "1");
    assert!(exits_within(forced, Duration::from_secs(10)), "a piped hook command under FORCE_COLOR opened the theme file");
    let plain = run_raw(&home, &show("plain").each_ref().map(String::as_str), false);
    let colour = run_raw(&home, &show("forced").each_ref().map(String::as_str), true);
    assert!(plain.contains("body"), "{plain:?}");
    assert_eq!(colour, plain, "forced colour changes no byte of a hook's output");

    // The probe catches a read: on a terminal the theme is read, and blocks.
    if have_script() {
        let args = show("tty");
        let blocked = !exits_within(tty(&home, &args.each_ref().map(String::as_str)), Duration::from_secs(2));
        assert!(blocked, "on a terminal ways reads the active theme, so the FIFO holds it");
    }
}

#[test]
fn json_on_a_terminal_is_plain_json() {
    if !have_script() {
        return;
    }
    let (home, _root) = home("json");
    // The report itself is a band.
    grounded(&["projects", "list"], &run(&home, &["projects", "list"]));
    for args in [vec!["projects", "list", "--json"], vec!["projects", "search", "x", "--json"], vec!["projects", "show", "/tmp", "--json"]] {
        let out = run_tty(&home, &args);
        assert!(!out.contains('\x1b'), "ways {args:?} on a terminal paints its JSON: {out:?}");
        serde_json::from_str::<serde_json::Value>(&out).unwrap_or_else(|e| panic!("ways {args:?}: {e}: {out:?}"));
    }
}
