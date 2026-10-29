//! Description templates (§8): text with `{field}` holes. `{field}`,
//! `{value}` and nested item fields such as `{message.user.name}` read the
//! item a trigger saw; `{state.<field>}` and `{prev.<field>}` read the
//! sensor's declared state after and before the poll's write;
//! `{trigger.count}` is the number of items that changed. `state.`, `prev.`
//! and `trigger.` are reserved. Filling a template is the host's work.

/// One piece of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    /// An item field, dotted for a nested one; `value` is the value itself.
    Item(String),
    State(String),
    Prev(String),
    TriggerCount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub text: String,
    pub pieces: Vec<Piece>,
}

impl Template {
    /// Parse `text`, or say what is wrong with it.
    pub fn parse(text: &str) -> Result<Template, String> {
        let mut pieces = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            if open > 0 {
                pieces.push(Piece::Text(rest[..open].to_string()));
            }
            let after = &rest[open + 1..];
            let close = after.find('}').ok_or_else(|| format!("`{{` at `{}` is not closed", &rest[open..]))?;
            pieces.push(hole(&after[..close])?);
            rest = &after[close + 1..];
        }
        if rest.contains('}') {
            return Err("`}` without `{`".to_string());
        }
        if !rest.is_empty() {
            pieces.push(Piece::Text(rest.to_string()));
        }
        Ok(Template { text: text.to_string(), pieces })
    }

    /// The declared state fields the template reads, through `state.` or `prev.`.
    pub fn state_fields(&self) -> impl Iterator<Item = &str> {
        self.pieces.iter().filter_map(|p| match p {
            Piece::State(f) | Piece::Prev(f) => Some(f.as_str()),
            _ => None,
        })
    }

    /// Whether the template reads a field of an item.
    pub fn reads_item(&self) -> bool {
        self.pieces.iter().any(|p| matches!(p, Piece::Item(_)))
    }
}

fn hole(inner: &str) -> Result<Piece, String> {
    let ok = !inner.is_empty() && inner.split('.').all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    if !ok {
        return Err(format!("`{{{inner}}}` is not a field: use letters, digits, `_` and `-`, dotted for a nested field"));
    }
    let one = |prefix: &str| -> Result<String, String> {
        match inner.strip_prefix(prefix) {
            Some(f) if !f.contains('.') => Ok(f.to_string()),
            _ => Err(format!("`{{{inner}}}`: `{prefix}` takes one declared state field")),
        }
    };
    match inner {
        "trigger.count" => Ok(Piece::TriggerCount),
        i if i.starts_with("trigger.") || i == "trigger" => Err(format!("`{{{inner}}}`: `trigger.` holds only `{{trigger.count}}`")),
        i if i.starts_with("state.") => one("state.").map(Piece::State),
        i if i.starts_with("prev.") => one("prev.").map(Piece::Prev),
        "state" | "prev" => Err(format!("`{{{inner}}}` names no field; write `{{{inner}.<field>}}`")),
        i => Ok(Piece::Item(i.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holes_parse_by_prefix() {
        let t = Template::parse("from {state.user} at {prev.last_ts}: {message.text} ({trigger.count})").unwrap();
        assert_eq!(
            t.pieces,
            vec![
                Piece::Text("from ".into()),
                Piece::State("user".into()),
                Piece::Text(" at ".into()),
                Piece::Prev("last_ts".into()),
                Piece::Text(": ".into()),
                Piece::Item("message.text".into()),
                Piece::Text(" (".into()),
                Piece::TriggerCount,
                Piece::Text(")".into()),
            ]
        );
        assert_eq!(t.state_fields().collect::<Vec<_>>(), vec!["user", "last_ts"]);
        assert!(Template::parse("go check new messages in slack mcp").unwrap().pieces.len() == 1);
    }

    #[test]
    fn reserved_prefixes_and_bad_holes_are_refused() {
        for bad in ["{trigger.name}", "{state}", "{state.a.b}", "{}", "open {", "close }", "{a b}"] {
            assert!(Template::parse(bad).is_err(), "{bad}");
        }
    }
}
