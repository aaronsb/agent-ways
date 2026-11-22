// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Evaluation of what a sensor file declares over a poll result: the
//! JSONPath subset, `where` conditions and description templates. A poll
//! result is a JSON document, whatever produced it, so a command sensor and
//! an mcp sensor evaluate through the same functions.
//!
//! - A path selects every value it reaches; `..` reaches the step after it
//!   at any depth.
//! - A property in `where` is a field of the item, dotted for a nested
//!   field. A property name with glob characters tests every field of the
//!   item it matches: the condition holds when at least one field matches
//!   and every matching field meets it. A property the item lacks fails
//!   its condition, `not` included.
//! - A bare value is equality, or a glob when it is a string with `*`, `?`
//!   or `[`. `equals` and `not` compare exactly. Numbers compare by value,
//!   so `1` equals `1.0`.
//! - A template hole the item or state does not fill becomes empty text.
//!
//! References:
//! - ADR-403 §3 and §8: triggers, `where`, the path subset, the operators
//!   and the template fields.

use std::collections::BTreeMap;

use sensor_spec::path::{JsonPath, Step};
use sensor_spec::template::{Piece, Template};
use sensor_spec::{Condition, Match, Op, Where};
use serde_json::Value;

/// Every value `path` selects in `doc`, in document order.
pub fn select<'a>(path: &JsonPath, doc: &'a Value) -> Vec<&'a Value> {
    let mut current = vec![doc];
    for step in &path.steps {
        current = match step {
            Step::Descend => current.into_iter().flat_map(descendants).collect(),
            step => current.into_iter().flat_map(|v| child(step, v)).collect(),
        };
    }
    current
}

/// `v` and every value nested in it, depth first.
fn descendants(v: &Value) -> Vec<&Value> {
    let mut out = vec![v];
    match v {
        Value::Array(items) => items.iter().for_each(|i| out.extend(descendants(i))),
        Value::Object(m) => m.values().for_each(|i| out.extend(descendants(i))),
        _ => {}
    }
    out
}

fn child<'a>(step: &Step, v: &'a Value) -> Vec<&'a Value> {
    match (step, v) {
        (Step::Field(f), Value::Object(m)) => m.get(f).into_iter().collect(),
        (Step::AnyField, Value::Object(m)) => m.values().collect(),
        (Step::Index(n), Value::Array(a)) => a.get(*n).into_iter().collect(),
        (Step::AnyIndex, Value::Array(a)) => a.iter().collect(),
        _ => Vec::new(),
    }
}

/// The field `name` of `item`, dotted for a nested field.
pub fn field<'a>(item: &'a Value, name: &str) -> Option<&'a Value> {
    name.split('.').try_fold(item, |v, part| v.as_object()?.get(part))
}

/// Whether `item` meets `conditions`, combined by `how`. No conditions is
/// met.
pub fn holds(conditions: &Where, how: Match, item: &Value) -> bool {
    let mut each = conditions.iter().map(|(prop, cond)| property_holds(prop, cond, item));
    match how {
        Match::All => each.all(|b| b),
        Match::Any => conditions.is_empty() || each.any(|b| b),
    }
}

fn property_holds(prop: &str, cond: &Condition, item: &Value) -> bool {
    if !has_glob(prop) {
        return field(item, prop).is_some_and(|v| condition_holds(cond, v));
    }
    let Some(m) = item.as_object() else { return false };
    let matching: Vec<&Value> = m.iter().filter(|(k, _)| glob(prop, k)).map(|(_, v)| v).collect();
    !matching.is_empty() && matching.iter().all(|v| condition_holds(cond, v))
}

/// Whether `v` meets `cond`.
pub fn condition_holds(cond: &Condition, v: &Value) -> bool {
    match cond {
        Condition::Is(want) => match want {
            serde_yaml::Value::String(p) if has_glob(p) => v.as_str().is_some_and(|s| glob(p, s)),
            want => same(&to_json(want), v),
        },
        Condition::Ops(ops) => ops.iter().all(|op| op_holds(op, v)),
    }
}

fn op_holds(op: &Op, v: &Value) -> bool {
    match op {
        Op::Equals(want) => same(&to_json(want), v),
        Op::Not(want) => !same(&to_json(want), v),
        Op::In(list) => list.iter().any(|w| same(&to_json(w), v)),
        Op::Contains(want) => match (v, to_json(want)) {
            (Value::String(s), Value::String(w)) => s.contains(&w),
            (Value::Array(items), w) => items.iter().any(|i| same(&w, i)),
            _ => false,
        },
        Op::Like(p) => v.as_str().is_some_and(|s| glob(p, s)),
        Op::Matches(re) => v.as_str().is_some_and(|s| re.0.is_match(s)),
        Op::Above(n) => v.as_f64().is_some_and(|x| x > *n),
        Op::Below(n) => v.as_f64().is_some_and(|x| x < *n),
    }
}

/// A YAML value from a sensor file as JSON.
fn to_json(v: &serde_yaml::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Equality, with numbers compared by value.
pub fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Whether `s` holds a glob character.
pub fn has_glob(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// Whether `text` matches the glob `pattern`: `*` is any run of
/// characters, `?` one character, and `[...]` one character of a set, with
/// ranges such as `a-z` and a leading `!` or `^` to negate it. A `[` with
/// no closing `]` is itself. The match walks both strings once, going back
/// only to the last `*`, so its time is linear in practice.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p = tokens(pattern);
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some(Token::Star) => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(tok) if tok.matches(t[ti]) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|tok| *tok == Token::Star)
}

/// One piece of a glob.
#[derive(Debug, PartialEq)]
enum Token {
    Star,
    Any,
    Char(char),
    /// The set's characters, and whether it is negated.
    Set(Vec<char>, bool),
}

impl Token {
    fn matches(&self, c: char) -> bool {
        match self {
            Token::Star => false,
            Token::Any => true,
            Token::Char(x) => *x == c,
            Token::Set(set, negate) => in_set(set, c) != *negate,
        }
    }
}

fn tokens(pattern: &str) -> Vec<Token> {
    let p: Vec<char> = pattern.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            '*' => out.push(Token::Star),
            '?' => out.push(Token::Any),
            '[' => match p[i + 1..].iter().position(|c| *c == ']').map(|n| n + i + 1) {
                Some(close) if close > i + 1 => {
                    let set = &p[i + 1..close];
                    let (negate, set) = match set.first() {
                        Some('!' | '^') => (true, &set[1..]),
                        _ => (false, set),
                    };
                    out.push(Token::Set(set.to_vec(), negate));
                    i = close;
                }
                _ => out.push(Token::Char('[')),
            },
            c => out.push(Token::Char(c)),
        }
        i += 1;
    }
    out
}

fn in_set(set: &[char], c: char) -> bool {
    let mut i = 0;
    while i < set.len() {
        if i + 2 < set.len() && set[i + 1] == '-' {
            if set[i] <= c && c <= set[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if set[i] == c {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// What a template reads: the item a trigger saw, the declared state after
/// and before the poll's write, and how many items changed.
pub struct Fill<'a> {
    pub item: Option<&'a Value>,
    pub state: &'a BTreeMap<String, Value>,
    pub prev: &'a BTreeMap<String, Value>,
    pub count: usize,
}

/// `template` with each hole filled from `f`. `{value}` is the item itself,
/// or its `value` field when the item is an object.
pub fn fill(template: &Template, f: &Fill) -> String {
    template
        .pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text(t) => t.clone(),
            Piece::Item(name) if name == "value" && !f.item.is_some_and(Value::is_object) => f.item.map(text).unwrap_or_default(),
            Piece::Item(name) => f.item.and_then(|i| field(i, name)).map(text).unwrap_or_default(),
            Piece::State(name) => f.state.get(name).map(text).unwrap_or_default(),
            Piece::Prev(name) => f.prev.get(name).map(text).unwrap_or_default(),
            Piece::TriggerCount => f.count.to_string(),
        })
        .collect()
}

/// A value as a template shows it: a string as written, anything else as
/// JSON.
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn path(p: &str) -> JsonPath {
        JsonPath::parse(p).unwrap()
    }

    /// The `where` of a trigger as a file writes it.
    fn cond(yaml: &str) -> (Where, Match) {
        let doc = format!("kind: command\nargv: [x]\noutput: json\ntriggers: [{{name: t, path: \"$\", on: added, {yaml}}}]\n");
        let spec = sensor_spec::parse("s", &serde_yaml::from_str(&doc).unwrap()).spec.unwrap_or_else(|| panic!("{yaml}"));
        let t = &spec.triggers[0];
        match &t.test {
            sensor_spec::Test::Items { where_, .. } => (where_.clone(), t.match_),
            _ => unreachable!(),
        }
    }

    fn meets(yaml: &str, item: Value) -> bool {
        let (w, m) = cond(yaml);
        holds(&w, m, &item)
    }

    #[test]
    fn a_path_selects_fields_indexes_wildcards_and_descendants() {
        let doc = json!({"units": [{"unit": "a.service", "sub": {"ts": 1}}, {"unit": "b.timer", "sub": {"ts": 2}}], "n": 3});
        assert_eq!(select(&path("$"), &doc), vec![&doc]);
        assert_eq!(select(&path("$.n"), &doc), vec![&json!(3)]);
        assert_eq!(select(&path("$.units[1].unit"), &doc), vec![&json!("b.timer")]);
        assert_eq!(select(&path("$.units[*].unit"), &doc), vec![&json!("a.service"), &json!("b.timer")]);
        assert_eq!(select(&path("$.units[0].*"), &doc), vec![&json!("a.service"), &json!({"ts": 1})]);
        assert_eq!(select(&path("$..ts"), &doc), vec![&json!(1), &json!(2)]);
        assert!(select(&path("$.units[5]"), &doc).is_empty());
        assert!(select(&path("$.n.x"), &doc).is_empty());
    }

    #[test]
    fn each_operator_tests_its_value() {
        let item = json!({"s": "web.service", "n": 5, "list": ["a", "b"], "f": 1.0});
        let cases: &[(&str, bool)] = &[
            ("where: {s: web.service}", true),
            ("where: {s: db.service}", false),
            ("where: {n: 5}", true),
            ("where: {f: 1}", true),
            ("where: {s: {equals: web.service}}", true),
            ("where: {s: {equals: \"*.service\"}}", false),
            ("where: {s: {not: db.service}}", true),
            ("where: {s: {not: web.service}}", false),
            ("where: {n: {in: [1, 5]}}", true),
            ("where: {n: {in: [1, 2]}}", false),
            ("where: {s: {contains: eb.se}}", true),
            ("where: {s: {contains: timer}}", false),
            ("where: {list: {contains: b}}", true),
            ("where: {list: {contains: c}}", false),
            ("where: {s: {like: \"w?b.*\"}}", true),
            ("where: {s: {like: \"*.timer\"}}", false),
            ("where: {s: {matches: \"^w[a-z]+\\\\.service$\"}}", true),
            ("where: {s: {matches: \"^db\"}}", false),
            ("where: {n: {above: 4}}", true),
            ("where: {n: {above: 5}}", false),
            ("where: {n: {below: 6}}", true),
            ("where: {n: {below: 5}}", false),
            ("where: {n: {above: 1, below: 10}}", true),
            ("where: {n: {above: 1, below: 3}}", false),
            ("where: {s: {above: 1}}", false),
        ];
        for (yaml, want) in cases {
            assert_eq!(meets(yaml, item.clone()), *want, "{yaml}");
        }
    }

    #[test]
    fn a_missing_property_fails_every_condition() {
        assert!(!meets("where: {gone: x}", json!({"s": 1})));
        assert!(!meets("where: {gone: {not: x}}", json!({"s": 1})));
    }

    #[test]
    fn glob_values_and_glob_property_names() {
        let item = json!({"s": "web.service", "wind_speed": 12, "wind_gust": 30, "temp": 4});
        assert!(meets("where: {s: \"*.service\"}", item.clone()));
        assert!(meets("where: {s: \"web.servic?\"}", item.clone()));
        assert!(meets("where: {s: \"[uvw]eb.*\"}", item.clone()));
        assert!(!meets("where: {s: \"[!w]eb.*\"}", item.clone()));
        assert!(meets("where: {s: \"[a-z]eb.*\"}", item.clone()));
        assert!(!meets("where: {s: \"*.timer\"}", item.clone()));
        assert!(meets("where: {\"wind_*\": {above: 10}}", item.clone()), "every matching field is above 10");
        assert!(!meets("where: {\"wind_*\": {above: 20}}", item.clone()), "wind_speed is not");
        assert!(!meets("where: {\"rain_*\": {above: 0}}", item.clone()), "no field matches");
        assert!(meets("where: {\"user.name\": ann}", json!({"user": {"name": "ann"}})), "a dotted property is nested");
    }

    #[test]
    fn match_all_needs_every_condition_and_any_needs_one() {
        let item = json!({"a": 1, "b": 2});
        assert!(meets("where: {a: 1, b: 2}", item.clone()));
        assert!(!meets("where: {a: 1, b: 3}", item.clone()));
        assert!(meets("where: {a: 1, b: 3}, match: any", item.clone()));
        assert!(!meets("where: {a: 0, b: 3}, match: any", item.clone()));
        assert!(holds(&Where::new(), Match::Any, &item) && holds(&Where::new(), Match::All, &item));
    }

    #[test]
    fn glob_handles_literals_sets_and_runs() {
        assert!(glob("a*b*c", "aXXbYc"));
        assert!(!glob("a*b*c", "aXXbY"));
        assert!(glob("[^x]", "y") && !glob("[^x]", "x"));
        assert!(glob("[", "[") && !glob("[", "a"));
        assert!(glob("", "") && !glob("", "a"));
        assert!(glob("*", "") && glob("a*", "a") && !glob("a?", "a"));
    }

    /// The match does not go back further than the last `*`, so a pattern
    /// of many stars fails fast on a long text it cannot match.
    #[test]
    fn glob_time_stays_linear_on_many_stars() {
        let long = "a".repeat(10_000);
        let started = std::time::Instant::now();
        assert!(!glob("*a*a*a*a*b", &long));
        assert!(glob("*a*a*a*a*", &long));
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "{:?}", started.elapsed());
    }

    #[test]
    fn a_template_fills_item_state_prev_and_count() {
        let t = Template::parse("{unit} {sub.ts} {value} | {state.failed} was {prev.failed} | {trigger.count} | {missing}.").unwrap();
        let state = BTreeMap::from([("failed".to_string(), json!(3))]);
        let prev = BTreeMap::from([("failed".to_string(), json!(1))]);
        let item = json!({"unit": "a.service", "sub": {"ts": 7}, "value": "v"});
        let f = Fill { item: Some(&item), state: &state, prev: &prev, count: 2 };
        assert_eq!(fill(&t, &f), "a.service 7 v | 3 was 1 | 2 | .");
        let scalar = json!("plain");
        let t = Template::parse("got {value}").unwrap();
        assert_eq!(fill(&t, &Fill { item: Some(&scalar), state: &state, prev: &prev, count: 1 }), "got plain");
        assert_eq!(fill(&t, &Fill { item: None, state: &state, prev: &prev, count: 0 }), "got ");
    }
}
