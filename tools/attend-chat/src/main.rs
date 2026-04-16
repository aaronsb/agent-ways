mod app;
mod signal;
mod watcher;

use iocraft::prelude::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!(
            "attend-chat {} ({})",
            env!("CARGO_PKG_VERSION"),
            env!("ATTEND_CHAT_COMMIT")
        );
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("attend-chat — interactive chat TUI for attend (ADR-120)");
        println!();
        println!("usage: attend-chat");
        println!();
        println!("  Esc                       exit");
        println!("  Enter                     send to broadcast");
        println!("  Shift-Enter / Alt-Enter   insert newline");
        return;
    }

    let (tx, rx) = async_channel::unbounded::<signal::Signal>();
    watcher::spawn_watcher(signal::broadcast_dir(), tx);

    let result = smol::block_on(
        element! {
            app::App(receiver: Some(rx))
        }
        .fullscreen(),
    );
    if let Err(e) = result {
        eprintln!("attend-chat: {}", e);
        std::process::exit(1);
    }
}
