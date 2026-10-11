//! `attend` CLI entry point. The command surface and every verb live in the
//! library (`lib.rs`); the binary is one frontend over it.

fn main() {
    attend::cli_main();
}
