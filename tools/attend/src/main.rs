//! `attend` CLI entry point. The command surface and every verb live in the
//! library (`lib.rs`); the binary is one frontend over it.

fn main() {
    // `attend keepwarm status | head -5` ends quietly once `head` exits.
    agent_fmt::exit_quietly_on_closed_pipe();
    attend::cli_main();
}
