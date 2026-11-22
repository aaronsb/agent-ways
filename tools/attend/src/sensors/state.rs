// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A sensor's declared state: the typed fields its file declares, filled
//! from each poll result and checked whole before anything is kept.
//!
//! A field fills from its `from` path, which must select one scalar, or
//! from the `state` object a command prints, for a field with no `from`. A
//! path that selects nothing leaves the field as it was. A write that fails
//! a check is refused whole, so the state stays as it was. A stored state is
//! kept with a fingerprint of the declaration that wrote it; under another
//! declaration, or failing the check, it is discarded and the sensor starts
//! from a fresh baseline.
//!
//! References:
//! - ADR-403 §3b and §8: the declaration, the mapping, emitted fields, the
//!   checks and their bounds.

use std::collections::BTreeMap;

use sensor_spec::{bounds, StateField, StateType};
use serde_json::Value;

use super::eval;

/// The declared fields of a sensor, by name.
pub type Declaration = BTreeMap<String, StateField>;

/// Field values, by name.
pub type Fields = BTreeMap<String, Value>;

/// The fields a fresh store holds: `0`, `""` or `false` for each.
pub fn fresh(decl: &Declaration) -> Fields {
    decl.iter()
        .map(|(name, f)| {
            let v = match f.ty {
                StateType::Number => Value::from(0),
                StateType::String => Value::from(""),
                StateType::Boolean => Value::from(false),
            };
            (name.clone(), v)
        })
        .collect()
}

/// A fingerprint of the declaration: each field's name, type, `max` and
/// `from`, in name order.
pub fn fingerprint(decl: &Declaration) -> String {
    let text: String = decl
        .iter()
        .map(|(name, f)| format!("{name}:{:?}:{}:{};", f.ty, f.max, f.from.as_ref().map_or("", |p| p.text.as_str())))
        .collect();
    format!("{:016x}", agent_identity::identity::fnv1a_64(text.as_bytes()))
}

/// The state after one poll: `current` with each mapped field read from
/// `result` and each emitted field taken from `emitted`, checked whole.
/// The error names the field and the check that refused the write.
pub fn write(decl: &Declaration, current: &Fields, result: &Value, emitted: Option<&serde_json::Map<String, Value>>) -> Result<Fields, String> {
    let mut next = fresh(decl);
    next.extend(current.iter().filter(|(k, _)| decl.contains_key(*k)).map(|(k, v)| (k.clone(), v.clone())));
    for (name, f) in decl {
        let Some(path) = &f.from else { continue };
        match eval::select(path, result).as_slice() {
            [] => {}
            [one] if !one.is_object() && !one.is_array() && !one.is_null() => {
                next.insert(name.clone(), (*one).clone());
            }
            [_] => return Err(format!("state field {name}: {} selects no scalar", path.text)),
            many => return Err(format!("state field {name}: {} selects {} values, and a mapping takes one", path.text, many.len())),
        }
    }
    for (name, v) in emitted.into_iter().flatten() {
        match decl.get(name) {
            None => return Err(format!("state field {name}: not declared")),
            Some(f) if f.from.is_some() => return Err(format!("state field {name}: it has a `from`, so the command may not emit it")),
            Some(_) => {
                next.insert(name.clone(), v.clone());
            }
        }
    }
    check(decl, &next)?;
    Ok(next)
}

/// Check `fields` whole against `decl`: every name declared, every type
/// exact, every string within its `max` and free of control and format
/// characters, and the whole within the size bound.
pub fn check(decl: &Declaration, fields: &Fields) -> Result<(), String> {
    for (name, v) in fields {
        let Some(f) = decl.get(name) else { return Err(format!("state field {name}: not declared")) };
        let typed = match (f.ty, v) {
            (StateType::Number, Value::Number(_)) | (StateType::Boolean, Value::Bool(_)) => true,
            (StateType::String, Value::String(s)) => {
                if s.chars().count() as u64 > f.max {
                    return Err(format!("state field {name}: longer than its max of {} characters", f.max));
                }
                if let Some(c) = s.chars().find(|c| is_control_or_format(*c)) {
                    return Err(format!("state field {name}: holds the control or format character U+{:04X}", c as u32));
                }
                true
            }
            _ => false,
        };
        if !typed {
            return Err(format!("state field {name}: {v} is not a {}", type_word(f.ty)));
        }
    }
    let size = serde_json::to_string(fields).map(|s| s.len() as u64).unwrap_or(u64::MAX);
    if size > bounds::STATE_BYTES {
        return Err(format!("the state is {size} bytes, over the {} byte bound", bounds::STATE_BYTES));
    }
    Ok(())
}

fn type_word(t: StateType) -> &'static str {
    match t {
        StateType::Number => "number",
        StateType::String => "string",
        StateType::Boolean => "boolean",
    }
}

/// Whether `c` is in Unicode category Cc (control) or Cf (format).
fn is_control_or_format(c: char) -> bool {
    let n = c as u32;
    c.is_control()
        || matches!(
            n,
            0xAD | 0x600..=0x605 | 0x61C | 0x6DD | 0x70F | 0x890..=0x891 | 0x8E2 | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decl(yaml: &str) -> Declaration {
        let doc = format!("kind: command\nargv: [x]\noutput: json\nstate: {yaml}\n");
        sensor_spec::parse("s", &serde_yaml::from_str(&doc).unwrap()).spec.unwrap_or_else(|| panic!("{yaml}")).state
    }

    #[test]
    fn a_fresh_store_holds_zero_empty_and_false() {
        let d = decl("{n: {type: number}, s: {type: string}, b: {type: boolean}}");
        assert_eq!(serde_json::to_string(&fresh(&d)).unwrap(), r#"{"b":false,"n":0,"s":""}"#);
    }

    #[test]
    fn mapped_and_emitted_fields_fill_and_a_miss_keeps_the_old_value() {
        let d = decl("{first: {type: string, from: \"$.units[0].unit\"}, failed: {type: number}, gone: {type: number, from: \"$.nope\"}}");
        let current = Fields::from([("gone".to_string(), json!(7))]);
        let emitted = json!({"failed": 2});
        let out = write(&d, &current, &json!({"units": [{"unit": "a.service"}]}), emitted.as_object()).unwrap();
        assert_eq!(serde_json::to_string(&out).unwrap(), r#"{"failed":2,"first":"a.service","gone":7}"#);
    }

    #[test]
    fn a_write_that_fails_a_check_is_refused_whole_naming_the_field() {
        let d = decl("{n: {type: number}, s: {type: string, max: 3}, m: {type: number, from: \"$.m\"}}");
        let none = Fields::new();
        let refuse = |result: Value, emitted: Value| write(&d, &none, &result, emitted.as_object()).unwrap_err();
        assert_eq!(refuse(json!({}), json!({"n": "3"})), "state field n: \"3\" is not a number");
        assert_eq!(refuse(json!({}), json!({"s": "four"})), "state field s: longer than its max of 3 characters");
        assert_eq!(refuse(json!({}), json!({"s": "a\u{200B}"})), "state field s: holds the control or format character U+200B");
        assert_eq!(refuse(json!({}), json!({"s": "a\n"})), "state field s: holds the control or format character U+000A");
        assert_eq!(refuse(json!({}), json!({"x": 1})), "state field x: not declared");
        assert_eq!(refuse(json!({}), json!({"m": 1})), "state field m: it has a `from`, so the command may not emit it");
        assert_eq!(refuse(json!({"m": {"a": 1}}), json!({})), "state field m: $.m selects no scalar");
    }

    #[test]
    fn the_state_stays_within_its_byte_bound() {
        let fields: String = (0..5).map(|i| format!("s{i}: {{type: string, max: 1024}}, ")).collect();
        let d = decl(&format!("{{{fields}}}"));
        let big: Fields = (0..5).map(|i| (format!("s{i}"), json!("x".repeat(1000)))).collect();
        assert_eq!(check(&d, &big).unwrap_err(), "the state is 5041 bytes, over the 4096 byte bound");
    }

    #[test]
    fn the_fingerprint_follows_the_declaration() {
        let a = fingerprint(&decl("{n: {type: number}}"));
        assert_eq!(a, fingerprint(&decl("{n: {type: number}}")));
        assert_ne!(a, fingerprint(&decl("{n: {type: string}}")));
        assert_ne!(a, fingerprint(&decl("{n: {type: number, from: \"$.n\"}}")));
    }
}
