// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use anyhow::Result;

pub fn run() -> Result<()> {
    let version = format!("v{} ({})", env!("CARGO_PKG_VERSION"), env!("WAYS_COMMIT"));
    agent_fmt::Banner::new("WAYS")
        .subtitle("cognitive steering for AI agents")
        .version(&version)
        .gradient(&agent_fmt::GRADIENT_CORAL)
        .print();
    Ok(())
}
