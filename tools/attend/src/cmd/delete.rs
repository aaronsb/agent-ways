//! `attend message delete`: mark what no live session still has to read
//! (ADR-404 D3e). The rule is `verbs::delete`; this reads the flags and
//! renders the result.

use crate::cli::DeleteArgs;
use crate::verbs::delete::{self, DeleteRequest, OlderThan, Scope};

pub(crate) fn cmd_delete(a: DeleteArgs) {
    if let Some(batch) = a.undo {
        let batch = (!batch.is_empty()).then_some(batch);
        match delete::undo(batch.as_deref()) {
            Ok(u) if a.json => crate::cmd::print_json(&u),
            Ok(u) => println!("unmarked {} message(s) from delete {}", u.unmarked, u.batch),
            Err(e) => refuse(&e, a.json),
        }
        return;
    }
    let scope = match (a.open, a.channel, a.everywhere) {
        (true, _, _) => Scope::Open,
        (_, Some(name), _) => Scope::Channel(name),
        (_, _, true) => Scope::Everywhere,
        _ => {
            let why = "name a scope: --open, --channel <name> or --everywhere";
            crate::cmd::refuse_usage(&format!("attend message delete: {why}"), why, a.json);
        }
    };
    let Some(older_than) = OlderThan::parse(&a.older_than, std::time::SystemTime::now()) else {
        let why = format!(
            "--older-than takes current, today, all, a duration such as 90m or 3d, or a date YYYY-MM-DD; got `{}`",
            a.older_than
        );
        crate::cmd::refuse_usage(&format!("attend message delete: {why}"), &why, a.json);
    };
    match delete::delete(&DeleteRequest { scope, older_than, dry_run: a.dry_run }) {
        Ok(d) if a.json => crate::cmd::print_json(&d),
        Ok(d) => {
            let verb = if d.dry_run { "would mark" } else { "marked" };
            match &d.batch {
                Some(batch) => println!("{verb} {} message(s) in {} for deletion (batch {batch})", d.marked, d.scope),
                None => println!("{verb} {} message(s) in {} for deletion", d.marked, d.scope),
            }
            for h in &d.held {
                println!("  kept {} that {} ({}) has not read", h.kept, h.name, h.session);
            }
            if d.batch.is_some() {
                println!("undo with: attend message delete --undo");
            }
        }
        Err(e) => refuse(&e, a.json),
    }
}

fn refuse(e: &delete::DeleteError, json: bool) -> ! {
    if json {
        crate::cmd::print_json_error(e, &e.to_string());
    }
    eprintln!("attend message delete: {e}");
    std::process::exit(1);
}
