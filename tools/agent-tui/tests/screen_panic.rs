//! A panic during a screen session ends the command in flight, as one
//! under `agent_tui::run` does. Its own test binary: the panic hook is
//! process-wide.

#![cfg(unix)]

use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn a_panic_ends_the_command_in_flight() {
    let mut child = Command::new("sleep").arg("30").process_group(0).spawn().expect("sleep");
    agent_tui::register_job_group(child.id());
    std::panic::set_hook(agent_tui::screen::panic_hook(Box::new(|_| {})));
    let _ = std::panic::catch_unwind(|| panic!("a screen panicked"));
    let _ = std::panic::take_hook();

    let start = Instant::now();
    let ended = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break Some(status);
        }
        if start.elapsed() > Duration::from_secs(5) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if ended.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(ended.is_some(), "the panic hook left the job running");
}
