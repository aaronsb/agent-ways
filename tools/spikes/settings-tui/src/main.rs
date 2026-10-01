//! Spike: every ways setting as one tree, browsed and edited in a TUI.
//!
//! Reads the real user, project and agent config. Writes nothing: on exit it
//! prints the change set `ways settings` would write.
//!
//!   ways-settings-spike [--project DIR]          the TUI
//!   ways-settings-spike --print [FILTER]         the tree as text, for a pipe

mod tree;
mod ui;
mod ways;

use std::io::IsTerminal;
use std::path::PathBuf;

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

    let mut term = ratatui::init();
    let result = ui::App::new(format!(" ways settings — {} ", project.display()), roots).run(&mut term);
    ratatui::restore();
    let roots = result?;

    let changes = tree::changes(&roots);
    if changes.is_empty() {
        println!("no changes");
    } else {
        println!("{} change(s); the spike wrote nothing. `ways settings` would write:", changes.len());
        for (key, store, from, to) in changes {
            match store {
                Some(s) => println!("  {}  {}: {from} → {to}", s.file.display(), s.key),
                None => println!("  {key}: {from} → {to}"),
            }
        }
    }
    Ok(())
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
