//! Spike: every ways setting as one tree, browsed and edited in a TUI.
//!
//! Reads the real user, project and agent config. Writes nothing: `w` reviews
//! the pending items and walks a simulated apply, and on exit it prints what
//! remains pending: the change set `ways settings` would write and the
//! commands its queued actions would run.
//!
//!   ways-settings-spike [--project DIR]          the TUI
//!   ways-settings-spike --print [FILTER]         the tree as text, for a pipe

mod tree;
mod ui;
mod ways;

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;

use ratatui::crossterm::event::DisableMouseCapture;
use ratatui::crossterm::execute;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut project = std::env::current_dir()?;
    let mut print = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--project" => project = PathBuf::from(args.next().expect("--project DIR")),
            "--print" => print = Some(args.next().unwrap_or_default()),
            "-h" | "--help" => {
                println!("ways-settings-spike [--project DIR] | --print [FILTER]");
                return Ok(());
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let paths = ways::Paths::resolve(&project);
    let roots = ways::build(&paths, &project);

    if print.is_some() || !std::io::stdout().is_terminal() {
        print_tree(&roots, &print.unwrap_or_default());
        return Ok(());
    }

    // ratatui::init's panic hook restores raw mode and the screen but not
    // mouse capture; this hook runs first and turns capture off.
    let mut term = ratatui::init();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        hook(info);
    }));
    // Read once: SPIKE_FAIL_STEP=<n> makes the n-th apply step fail, to see the failure path.
    let fail_step = std::env::var("SPIKE_FAIL_STEP").ok().and_then(|v| v.parse().ok());
    let app = ui::App::new(format!(" ways settings — {} ", project.display()), roots).shape(ui::theme::Shape::from_env()).fail_step(fail_step).helpers(ways::helpers(ways::Env::resolve(&paths, &project)));
    let result = app.run(&mut term);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let session = result?;
    print!("{}", summary(&session.roots, &session.queue));
    Ok(())
}

/// What a real `ways settings` would do: value changes by file, then the
/// queued commands in order. Secrets appear only as `<stdin>`.
fn summary(roots: &[tree::Node], queue: &tree::Queue) -> String {
    let changes = tree::changes(roots);
    if changes.is_empty() && queue.is_empty() {
        return "nothing pending\n".into();
    }
    let mut out = String::new();
    if !changes.is_empty() {
        let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (key, store, from, to) in &changes {
            let (file, key) = match store {
                Some(s) => (s.file.display().to_string(), s.key.clone()),
                None => ("(no store)".into(), key.clone()),
            };
            by_file.entry(file).or_default().push(format!("    {key}: {from} → {to}"));
        }
        out += &format!("{} change(s); the spike wrote nothing. `ways settings` would write:\n", changes.len());
        for (file, lines) in by_file {
            out += &format!("  {file}\n{}\n", lines.join("\n"));
        }
    }
    if !queue.is_empty() {
        out += &format!("{} action(s) queued; the spike ran none. A real `ways settings` would run, in order:\n", queue.len());
        for (i, q) in queue.items().iter().enumerate() {
            out += &format!("  {}. {}\n", i + 1, q.command);
        }
    }
    out
}

/// Every row with groups expanded, as `key = value  (source)`.
fn print_tree(roots: &[tree::Node], filter: &str) {
    let f = if filter.is_empty() { "." } else { filter };
    let mut open = roots.to_vec();
    fn expand(n: &mut tree::Node) {
        n.open = true;
        n.children.iter_mut().for_each(expand);
    }
    open.iter_mut().for_each(expand);
    for r in tree::rows(&open, if filter.is_empty() { "" } else { f }) {
        let n = tree::get(&open, &r.path);
        let indent = "  ".repeat(r.depth);
        match &n.setting {
            Some(s) => println!("{indent}{} = {}  ({})", n.name, s.value, s.source),
            None => println!("{indent}{}/", n.name),
        }
    }
}
