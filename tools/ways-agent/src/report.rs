//! `ways agent cost`: the judge's spend, printed from the shared
//! aggregation in [`ways_agent_core::spend`].

use anyhow::Result;
use ways_agent_core::spend::{covers_since, filter, filter_project, load, parse_date, render_text, report, By};

pub fn run(since: Option<&str>, session: Option<&str>, project: Option<&str>, by: By, json: bool) -> Result<()> {
    let since = since.map(parse_date).transpose()?;
    let all = load();
    let covers = covers_since(&all);
    let project = project.map(ways_core::util::project_arg);
    let calls = filter_project(filter(all, since.as_deref(), session), project.as_deref());
    if json {
        println!("{}", serde_json::to_string_pretty(&report(&calls, covers))?);
    } else {
        // The log's start bounds the query unless --since starts later.
        let bounds = covers.as_deref().filter(|c| since.as_deref().is_none_or(|s| s < &c[..c.len().min(10)]));
        print!("{}", render_text(&calls, by, bounds));
    }
    Ok(())
}
