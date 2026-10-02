//! `tui-harness`: run a TUI in a detached tmux session, send it keys, and
//! screenshot it as a PNG.

use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
        #[arg(long, default_value_t = DEFAULT_COLS)]
        cols: u32,
        #[arg(long, default_value_t = DEFAULT_ROWS)]
        rows: u32,
        #[arg(long, default_value = DEFAULT_FONT)]
        font: String,
        /// Font size in pixels per em
        #[arg(long, default_value_t = DEFAULT_SIZE)]
        size: u32,
        /// The command and its arguments, after `--`
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
    /// Kill the session and remove its state
    Down { name: String },
    /// List sessions
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
        #[arg(long, default_value_t = DEFAULT_SIZE)]
        size: u32,
        /// Pad the image to N columns
        #[arg(long)]
        cols: Option<u32>,
        /// Pad the image to M rows
        #[arg(long)]
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
            let opts = LaunchOptions {
                cols,
                rows,
                font,
                size,
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
            let path = harness.session(&name)?.shot(out.as_deref())?;
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
        Cmd::Down { name } => {
            harness.session(&name)?.down()?;
            println!("down: {name}");
        }
        Cmd::Ls => {
            let sessions = harness.list()?;
            if sessions.is_empty() {
                println!("(no sessions)");
                return Ok(());
            }
            println!(
                "{:<20} {:<10} {:<6} {:<25} CMD",
                "NAME", "GEOMETRY", "STATE", "FONT"
            );
            for s in sessions {
                let state = if s.alive() { "up" } else { "gone" };
                let geometry = format!("{}x{}", s.cols, s.rows);
                println!(
                    "{:<20} {:<10} {:<6} {:<25} {}",
                    s.name, geometry, state, s.font, s.cmd
                );
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
            Renderer::new(&font, size).render_to(&grid, cols, rows, &out)?;
            println!("{}", out.display());
        }
    }
    Ok(())
}
