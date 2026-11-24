// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

/// Log diagnostic info to stderr, which the attend mod sends to its debug
/// log for the sensor host.
pub fn log(message: &str) {
    eprintln!("[attend] {message}");
}
