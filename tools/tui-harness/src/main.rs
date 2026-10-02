//! `tui-harness`: run a TUI in a detached tmux session, send it keys, and
//! screenshot it as a PNG.

use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tui_harness::render::{fc_has_family, MAX_CELLS, MAX_SIZE};
use tui_harness::session::DownOutcome;
use tui_harness::{
    parse, Harness, LaunchOptions, Renderer, DEFAULT_COLS, DEFAULT_FONT, DEFAULT_ROWS, DEFAULT_SIZE,
};

#[derive(Parser)]
#[command(
    name = "tui-harness",
    version,
    about = "Drive a TUI in a detached tmux terminal and screenshot it"
)]
struct Cli {
    /// State root for sessions and shots
    /// [default: $XDG_STATE_HOME/agent-ways/tui-harness]
    #[arg(long, global = true)]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start <cmd> in a detached tmux session
    Launch {
        name: String,
        #[arg(long, default_value_t = DEFAULT_COLS, value_parser = clap::value_parser!(u32).range(1..=MAX_CELLS as i64))]
        cols: u32,
        #[arg(long, default_value_t = DEFAULT_ROWS, value_parser = clap::value_parser!(u32).range(1..=MAX_CELLS as i64))]
        rows: u32,
        #[arg(long, default_value = DEFAULT_FONT)]
        font: String,
        /// Font size in pixels per em
        #[arg(long, default_value_t = DEFAULT_SIZE, value_parser = clap::value_parser!(u32).range(1..=MAX_SIZE as i64))]
        size: u32,
        /// The command and its arguments, after `--`. It runs with this
        /// shell's environment and working directory.
        #[arg(last = true, required = true)]
        cmd: Vec<String>,
    },
    /// Pass keys to `tmux send-keys` (e.g. j Enter C-c, or -l "literal")
    Send {
        name: String,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
    /// Render the pane to a PNG and print its path
    Shot {
        name: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Print the pane contents
    Text {
        name: String,
        /// Keep SGR colour escapes
        #[arg(long)]
        ansi: bool,
    },
    /// Attach this terminal to the session, to look in (detach with C-b d)
    Attach { name: String },
    /// Kill the session (if this root launched it) and remove its state
    Down {
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        name: Option<String>,
        /// Every session of this root, and its orphans
        #[arg(long)]
        all: bool,
    },
    /// Kill this root's orphan sessions and remove state for sessions that are gone
    Prune,
    /// List sessions, and orphans on the server with no state here
    Ls,
    /// Render ANSI text (a saved `capture-pane -ep`) from a file or stdin to a PNG
    Render {
        /// Input file; stdin when absent
        #[arg(long = "in")]
        input: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = DEFAULT_FONT)]
        font: String,
        #[arg(long, default_value_t = DEFAULT_SIZE, value_parser = clap::value_parser!(u32).range(1..=MAX_SIZE as i64))]
        size: u32,
        /// Pad the image to N columns
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=MAX_CELLS as i64))]
        cols: Option<u32>,
        /// Pad the image to M rows
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=MAX_CELLS as i64))]
        rows: Option<u32>,
    },
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let harness = cli.dir.map(Harness::new).unwrap_or_else(Harness::from_env);
    match cli.command {
        Cmd::Launch {
            name,
            cols,
            rows,
            font,
            size,
            cmd,
        } => {
            if !fc_has_family(&font) {
                eprintln!("warning: font '{font}' is not installed; shots will use a substitute");
            }
            let opts = LaunchOptions {
                cols,
                rows,
                font,
                size,
                ..LaunchOptions::default()
            };
            let s = harness.launch(&name, &opts, &cmd)?;
            println!(
                "launched '{}' ({} x {}, {} @ {}pt)",
                s.name, s.cols, s.rows, s.font, s.size
            );
            println!(
                "  tmux:   {} (socket {})",
                s.tmux_name,
                tui_harness::session::TMUX_SOCKET
            );
            println!("  attach: tui-harness attach {}", s.name);
            println!("  cmd:    {}", s.cmd);
            println!(
                "  shots:  {}",
                harness.shots_dir().join(format!("{name}-*.png")).display()
            );
        }
        Cmd::Send { name, keys } => harness.session(&name)?.send(&keys)?,
        Cmd::Shot { name, out } => {
            let session = harness.session(&name)?;
            let renderer = Renderer::new(&session.font, session.size);
            warn(&renderer);
            let path = session.shot_with(&renderer, out.as_deref())?;
            println!("{}", path.display());
        }
        Cmd::Text { name, ansi } => print!("{}", harness.session(&name)?.text(ansi)?),
        Cmd::Attach { name } => {
            let mut cmd = harness.session(&name)?.attach_command();
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                let err = cmd.exec();
                return Err(err).context("exec tmux attach");
            }
            #[cfg(not(unix))]
            {
                let status = cmd.status().context("running tmux attach")?;
                if !status.success() {
                    anyhow::bail!("tmux attach exited with {status}");
                }
            }
        }
        Cmd::Down {
            name: Some(name), ..
        } => {
            let outcome = harness.session(&name)?.down()?;
            report_down(&name, &outcome);
        }
        Cmd::Down { name: None, .. } => {
            let all = harness.down_all()?;
            if all.is_empty() {
                println!("(no sessions)");
            }
            for (name, outcome) in all {
                report_down(&name, &outcome);
            }
        }
        Cmd::Prune => {
            let (killed, removed) = harness.prune()?;
            for k in &killed {
                println!("killed orphan: {k}");
            }
            for r in &removed {
                println!("removed stale state: {r}");
            }
            if killed.is_empty() && removed.is_empty() {
                println!("nothing to prune");
            }
        }
        Cmd::Ls => {
            let sessions = harness.list()?;
            let orphans = harness.orphans();
            if sessions.is_empty() && orphans.is_empty() {
                println!("(no sessions)");
                return Ok(());
            }
            println!(
                "{:<20} {:<10} {:<8} {:<25} CMD",
                "NAME", "GEOMETRY", "STATE", "FONT"
            );
            for s in sessions {
                let state = match (s.alive(), s.owned()) {
                    (false, _) => "gone",
                    (true, true) => "up",
                    (true, false) => "foreign",
                };
                let geometry = format!("{}x{}", s.cols, s.rows);
                println!(
                    "{:<20} {:<10} {:<8} {:<25} {}",
                    s.name, geometry, state, s.font, s.cmd
                );
            }
            for o in orphans {
                println!("{:<20} {:<10} {:<8} {:<25} -", o.name(), "-", "orphan", "-");
            }
        }
        Cmd::Render {
            input,
            out,
            font,
            size,
            cols,
            rows,
        } => {
            let mut bytes = Vec::new();
            match input {
                Some(p) => {
                    bytes = std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?
                }
                None => {
                    std::io::stdin()
                        .read_to_end(&mut bytes)
                        .context("reading stdin")?;
                }
            }
            let grid = parse(&String::from_utf8_lossy(&bytes));
            let renderer = Renderer::new(&font, size);
            warn(&renderer);
            renderer.render_to(&grid, cols, rows, &out)?;
            println!("{}", out.display());
        }
    }
    Ok(())
}

fn warn(renderer: &Renderer) {
    for w in renderer.warnings() {
        eprintln!("warning: {w}");
    }
}

fn report_down(name: &str, outcome: &DownOutcome) {
    match outcome {
        DownOutcome::Killed => println!("down: {name}"),
        DownOutcome::AlreadyGone => println!("down: {name} (was not running)"),
        DownOutcome::LeftRunning { owner } => println!(
            "down: {name} (state removed; the running session belongs to {} and was left alone)",
            owner.as_deref().unwrap_or("no root")
        ),
    }
}
