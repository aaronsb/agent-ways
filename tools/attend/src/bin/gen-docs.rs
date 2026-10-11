//! Generate the `attend` markdown reference from the same `Cli`
//! definition that drives runtime `--help`. Output goes to stdout;
//! `make docs` redirects it into `docs/cli/attend.md`.

fn main() {
    // Ensure subcommands have a stable parent name in the rendered tree.
    let cmd = attend::cli_command().name("attend");
    let md = clap_markdown::help_markdown_command(&cmd);
    print!("{md}");
}
