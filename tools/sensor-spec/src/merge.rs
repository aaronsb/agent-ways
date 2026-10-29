//! Layering (§2): a later layer overrides any field of an earlier one.
//! Mappings merge key by key, `triggers` merge by trigger name, and every
//! other value, a list included, is replaced whole.

use serde_yaml::{Mapping, Value};

/// Merge `over` onto `base`.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Mapping(b), Value::Mapping(o)) => merge_map(b, o),
        (b, o) => *b = o.clone(),
    }
}

fn merge_map(base: &mut Mapping, over: &Mapping) {
    for (k, v) in over {
        match base.get_mut(k) {
            Some(Value::Sequence(b)) if k.as_str() == Some("triggers") => match v {
                Value::Sequence(o) => merge_triggers(b, o),
                o => *base.get_mut(k).expect("present") = o.clone(),
            },
            Some(b) => merge(b, v),
            None => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

/// A trigger of `over` with the name of one in `base` merges into it; any
/// other is added after them.
fn merge_triggers(base: &mut Vec<Value>, over: &[Value]) {
    for t in over {
        let name = t.get("name").and_then(Value::as_str);
        match name.and_then(|n| base.iter_mut().find(|b| b.get("name").and_then(Value::as_str) == Some(n))) {
            Some(b) => merge(b, t),
            None => base.push(t.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn y(s: &str) -> Value {
        serde_yaml::from_str(s).unwrap()
    }

    #[test]
    fn fields_override_and_triggers_merge_by_name() {
        let mut base = y("interval: 30\nrequires: [a, b]\nargs: {channel: general, limit: 5}\ntriggers:\n  - {name: one, magnitude: 1}\n  - {name: two, magnitude: 2}\n");
        merge(&mut base, &y("interval: 90\nrequires: [c]\nargs: {limit: 9}\ntriggers:\n  - {name: two, magnitude: 7}\n  - {name: three}\n"));
        assert_eq!(
            base,
            y("interval: 90\nrequires: [c]\nargs: {channel: general, limit: 9}\ntriggers:\n  - {name: one, magnitude: 1}\n  - {name: two, magnitude: 7}\n  - {name: three}\n")
        );
    }
}
