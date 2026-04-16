use attend_chat::{app, signal, watcher};
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
        println!("  Left / Right / Home / End move cursor");
        println!("  Backspace / Delete        edit");
        return;
    }

    let (tx, rx) = async_channel::unbounded::<signal::Signal>();
    let dir = signal::broadcast_dir();
    if let Err(e) = watcher::spawn_watcher(dir.clone(), tx) {
        eprintln!("attend-chat: failed to start signal watcher: {}", e);
        eprintln!("  signals dir: {}", dir.display());
        eprintln!("  (no point opening the TUI — nothing would stream in.)");
        std::process::exit(1);
    }

    // `.disable_mouse_capture()` opts out of iocraft's default
    // fullscreen mouse capture so the host terminal keeps native
    // select / copy / paste. We don't consume mouse events today;
    // when the sidebar lands and we want click-to-reply, we'll
    // re-enable capture scoped to that region.
    let result = smol::block_on(
        element! {
            app::App(receiver: Some(rx))
        }
        .fullscreen()
        .disable_mouse_capture(),
    );
    if let Err(e) = result {
        eprintln!("attend-chat: {}", e);
        std::process::exit(1);
    }
}
