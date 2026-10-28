//! `attend` CLI entry point. The command surface and every verb live in the
//! library (`lib.rs`); the binary is one frontend over it.

fn main() {
    // `attend keepwarm status | head -5` ends quietly once `head` exits.
    agent_fmt::exit_quietly_on_closed_pipe();
    // Output on a terminal draws in the chosen theme; piped, as the hooks'
    // is, it reads none (ADR-504 §11).
    agent_theme::install_from(|| attend_config::theme::choice().active);
    attend::cli_main();
}
