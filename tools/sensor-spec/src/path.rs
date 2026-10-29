//! The JSONPath subset of §8: `$`, `.field`, `[n]`, `[*]` and `..`. A
//! path is parsed and checked when its file loads; evaluating one over a
//! poll result is the command and mcp hosts' work.

/// One step of a path after the leading `$`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// `.field`; `.*` is [`Step::AnyField`].
    Field(String),
    /// `.*`: every field of an object.
    AnyField,
    /// `[n]`.
    Index(usize),
    /// `[*]`: every item of an array.
    AnyIndex,
    /// `..field` or `..*`: the step that follows, at any depth.
    Descend,
}

/// A parsed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPath {
    pub text: String,
    pub steps: Vec<Step>,
}

impl JsonPath {
    /// Parse `text`, or say what is wrong with it.
    pub fn parse(text: &str) -> Result<JsonPath, String> {
        let Some(rest) = text.strip_prefix('$') else {
            return Err(format!("`{text}` is not a path: a path starts with `$`"));
        };
        let chars: Vec<char> = rest.chars().collect();
        let mut steps = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '.' if chars.get(i + 1) == Some(&'.') => {
                    steps.push(Step::Descend);
                    i += 2;
                    let (step, next) = field(&chars, i, text)?;
                    steps.push(step);
                    i = next;
                }
                '.' => {
                    let (step, next) = field(&chars, i + 1, text)?;
                    steps.push(step);
                    i = next;
                }
                '[' => {
                    let close = chars[i..].iter().position(|c| *c == ']').map(|p| p + i).ok_or_else(|| format!("`{text}`: `[` is not closed"))?;
                    let inner: String = chars[i + 1..close].iter().collect();
                    let step = match inner.as_str() {
                        "*" => Step::AnyIndex,
                        n => Step::Index(n.parse().map_err(|_| format!("`{text}`: `[{n}]` is neither an index nor `[*]`"))?),
                    };
                    steps.push(step);
                    i = close + 1;
                }
                c => return Err(format!("`{text}`: `{c}` cannot start a step; use `.field`, `[n]`, `[*]` or `..`")),
            }
        }
        if steps.last() == Some(&Step::Descend) {
            return Err(format!("`{text}`: `..` needs a field after it"));
        }
        Ok(JsonPath { text: text.to_string(), steps })
    }

    /// Whether the path can select more than one value.
    pub fn is_wild(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::AnyField | Step::AnyIndex | Step::Descend))
    }
}

/// The field name starting at `i`, up to the next `.` or `[`.
fn field(chars: &[char], i: usize, text: &str) -> Result<(Step, usize), String> {
    let end = chars[i..].iter().position(|c| *c == '.' || *c == '[').map(|p| p + i).unwrap_or(chars.len());
    let name: String = chars[i..end].iter().collect();
    match name.as_str() {
        "" => Err(format!("`{text}`: a `.` needs a field name after it")),
        "*" => Ok((Step::AnyField, end)),
        n if n.contains(']') => Err(format!("`{text}`: `]` without `[`")),
        n => Ok((Step::Field(n.to_string()), end)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subset_parses() {
        let p = JsonPath::parse("$.messages[0].user.name").unwrap();
        assert_eq!(p.steps, vec![Step::Field("messages".into()), Step::Index(0), Step::Field("user".into()), Step::Field("name".into())]);
        assert!(!p.is_wild());
        let p = JsonPath::parse("$..ts").unwrap();
        assert_eq!(p.steps, vec![Step::Descend, Step::Field("ts".into())]);
        assert!(JsonPath::parse("$.items[*].id").unwrap().is_wild());
        assert_eq!(JsonPath::parse("$").unwrap().steps, vec![]);
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in ["messages", "$.", "$..", "$[x]", "$[0", "$x", "$.a]"] {
            assert!(JsonPath::parse(bad).is_err(), "{bad}");
        }
    }
}
