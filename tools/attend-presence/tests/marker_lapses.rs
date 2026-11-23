// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A sensor marker refuses only while the attend it names runs, and a
//! process that checked while that attend ran is allowed once it exits.
//! The check reads the marker on every call and keeps only the ancestry.
//!
//! This file holds one test, so the variable it sets in its own
//! environment is seen by no other test.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::os::unix::process::CommandExt;

use attend_presence::session::{marker_value, sensor_start, SensorStart, SENSOR_HOST_VAR};

/// A shell whose command line reads `sh -c 'sleep 30; :' <dir>/attend run`
/// stands in for the attend that set the marker: its argv starts a sensor
/// host, which is how attend tells an attend it cannot name by its
/// executable. The trailing `:` keeps the shell from exec'ing `sleep` in
/// its place. Its marker refuses this process; once the shell's group has
/// been killed and the shell reaped, the same marker refuses nothing, in
/// the same process.
#[test]
fn a_marker_lapses_when_its_attend_exits() {
    let stand_in = std::env::temp_dir().join("attend-marker-lapses").join("attend");
    let mut attend = std::process::Command::new("sh").args(["-c", "sleep 30; :", stand_in.to_str().unwrap(), "run"]).process_group(0).spawn().unwrap();
    let pid = attend.id();
    let marker = marker_value(pid);
    std::env::set_var(SENSOR_HOST_VAR, &marker);
    let own = std::process::id();
    let while_running = sensor_start(own);
    // SAFETY: kill(2) on the process group the shell leads; the shell is
    // not reaped yet, so its pid still names that group.
    unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
    attend.wait().unwrap();
    let after = sensor_start(own);
    assert_eq!(while_running, Some(SensorStart::Marker { pid, value: marker }), "the marker of a running attend");
    assert_eq!(after, None, "the same marker once its attend has exited");
}
