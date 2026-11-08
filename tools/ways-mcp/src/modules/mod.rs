// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The modules this server hosts. A module joins by adding itself here
//! (ADR-501 item 3).

mod lookup;
mod ways;

use crate::server::Module;

pub fn all() -> Vec<Box<dyn Module>> {
    vec![Box::new(ways::Ways)]
}
