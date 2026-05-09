//! Generate the `attend` markdown reference from the same `Cli`
//! definition that drives runtime `--help`. Output goes to stdout;
//! `make docs` redirects it into `docs/cli/attend.md`.
//!
//! `#[path]` lets this bin share the canonical `cli.rs` with `main.rs`
//! without forcing the crate into a lib+bin split.

#[path = "../cli.rs"]
mod cli;

use clap::CommandFactory;

fn main() {
    let mut cmd = cli::Cli::command();
    // Ensure subcommands have a stable parent name in the rendered tree.
    cmd = cmd.name("attend");
    let md = clap_markdown::help_markdown_command(&cmd);
    print!("{md}");
}
