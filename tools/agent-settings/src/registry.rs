//! The registry: several components' schemas composed into one tree
//! (ADR-503 §1), and the canonical or effective fragments emitted from it (§2).

use crate::load::{bindings, resolve, Layer};
use crate::schema::{Kind, KeySpec, Schema, SectionSpec};
use serde_yaml::{Mapping, Value};

/// A key found by name, with its wildcards bound.
#[derive(Debug, Clone)]
pub struct Bound {
    pub schema: &'static Schema,
    pub spec: &'static KeySpec,
    pub bound: Vec<String>,
}

impl Bound {
    pub fn name(&self) -> String {
        self.spec.bind(&self.bound).0
    }

    pub fn path(&self) -> Vec<String> {
        self.spec.bind(&self.bound).1
    }
}

#[derive(Debug, Clone)]
pub struct Registry {
    schemas: Vec<&'static Schema>,
}

/// Whether a dotted name falls under a prefix: equal, or below it at a dot.
pub fn under(name: &str, prefix: &str) -> bool {
    prefix.is_empty() || name == prefix || name.strip_prefix(prefix).is_some_and(|r| r.starts_with('.'))
}

impl Registry {
    pub fn new(schemas: Vec<&'static Schema>) -> Registry {
        Registry { schemas }
    }

    pub fn schemas(&self) -> &[&'static Schema] {
        &self.schemas
    }

    pub fn keys(&self) -> impl Iterator<Item = (&'static Schema, &'static KeySpec)> + '_ {
        self.schemas.iter().flat_map(|s| s.keys.iter().map(move |k| (*s, k)))
    }

    pub fn sections(&self) -> impl Iterator<Item = (&'static Schema, &'static SectionSpec)> + '_ {
        self.schemas.iter().flat_map(|s| s.sections.iter().map(move |k| (*s, k)))
    }

    /// A key by its concrete dotted name. Fixed names win over patterns.
    pub fn lookup(&self, name: &str) -> Option<Bound> {
        if let Some((schema, spec)) = self.keys().find(|(_, k)| !k.is_pattern() && k.name == name) {
            return Some(Bound { schema, spec, bound: vec![] });
        }
        self.keys()
            .filter(|(_, k)| k.is_pattern())
            .find_map(|(schema, spec)| spec.match_name(name).map(|bound| Bound { schema, spec, bound }))
    }

    /// A key by the file kind and key path it is stored at.
    pub fn lookup_path(&self, file: &str, path: &[String]) -> Option<Bound> {
        self.keys()
            .filter(|(_, k)| k.file == file && k.computed.is_none())
            .find_map(|(schema, spec)| spec.match_path(path).map(|bound| Bound { schema, spec, bound }))
    }

    pub fn section(&self, name: &str) -> Option<(&'static Schema, &'static SectionSpec)> {
        self.sections().find(|(_, s)| s.name == name)
    }

    /// The section that owns a top-level key, in whichever file kind has it.
    pub fn owner_of_top(&self, top: &str) -> Option<(&'static Schema, &'static SectionSpec)> {
        self.sections().find(|(_, s)| s.top.contains(&top))
    }

    /// Every concrete key under `prefix`, with its bindings, in schema order.
    pub fn concrete(&self, prefix: &str, layers: &[Layer]) -> Vec<Bound> {
        let mut out = Vec::new();
        for (schema, spec) in self.keys() {
            for bound in bindings(spec, layers) {
                let b = Bound { schema, spec, bound };
                let section_match = !prefix.is_empty() && spec.section == prefix;
                if section_match || under(&b.name(), prefix) {
                    out.push(b);
                }
            }
        }
        out
    }

    /// The canonical fragment under `prefix`, or with `layers` the effective
    /// one, as one file-shaped mapping per file kind. Secrets, computed and
    /// read-only keys without a default are left out.
    pub fn emit(&self, prefix: &str, layers: Option<&[Layer]>) -> Vec<(&'static str, Value)> {
        crate::load::trace("emit");
        let mut files: Vec<(&'static str, Value)> = Vec::new();
        for b in self.concrete(prefix, layers.unwrap_or(&[])) {
            if b.spec.kind == Kind::Secret || b.spec.computed.is_some() {
                continue;
            }
            let v = match layers {
                Some(l) => resolve(b.spec, &b.bound, l).value,
                None => b.spec.default_for(&b.bound),
            };
            let Some(v) = v else { continue };
            let idx = match files.iter().position(|(f, _)| *f == b.spec.file) {
                Some(i) => i,
                None => {
                    files.push((b.spec.file, Value::Mapping(Mapping::new())));
                    files.len() - 1
                }
            };
            set_path(&mut files[idx].1, &b.path(), v);
        }
        files
    }
}

fn set_path(root: &mut Value, path: &[String], v: Value) {
    let mut cur = root;
    for seg in &path[..path.len() - 1] {
        let m = cur.as_mapping_mut().expect("fragments are mappings");
        if !m.contains_key(seg.as_str()) {
            m.insert(Value::String(seg.clone()), Value::Mapping(Mapping::new()));
        }
        cur = m.get_mut(seg.as_str()).expect("just inserted");
    }
    if let Some(m) = cur.as_mapping_mut() {
        m.insert(Value::String(path[path.len() - 1].clone()), v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::*;

    const BASE: KeySpec = KeySpec {
        name: "matching.prob",
        section: "matching",
        file: "cfg",
        path: &["prob"],
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: DefaultValue::Yaml("0.5"),
        instances: &[],
        scope: Scope::Both,
        doc: "",
        long: "",
        check: None,
        computed: None,
        fail_closed: None,
    };
    static SCHEMA: Schema = Schema {
        component: "t",
        files: &[FileSpec { id: "cfg", retired: &[] }, FileSpec { id: "agent", retired: &[] }],
        sections: &[
            SectionSpec { per_entry: false, entry: None, repair: None, name: "matching", file: "cfg", top: &["prob", "presets"], doc: "" },
            SectionSpec { per_entry: false, entry: None, repair: None, name: "gate", file: "agent", top: &["mode"], doc: "" },
        ],
        keys: &[
            BASE,
            KeySpec { name: "matching.presets.*", path: &["presets", "*"], instances: &["normal", "rare"], default: DefaultValue::Fn(preset), ..BASE },
            KeySpec { name: "gate.mode", section: "gate", file: "agent", path: &["mode"], kind: Kind::Choice(&["on", "off"]), default: DefaultValue::Yaml("on"), ..BASE },
            KeySpec { name: "gate.keys.*", section: "gate", file: "agent", path: &["-", "*"], kind: Kind::Secret, instances: &["a"], computed: Some(|_| Value::String("absent".into())), ..BASE },
        ],
    };

    fn preset(b: &[String]) -> Option<Value> {
        Some(serde_yaml::from_str(if b[0] == "normal" { "0.15" } else { "0.4" }).unwrap())
    }

    #[test]
    fn lookup_and_emit() {
        let r = Registry::new(vec![&SCHEMA]);
        assert_eq!(r.lookup("matching.presets.rare").unwrap().path(), vec!["presets", "rare"]);
        assert!(r.lookup("matching.nope").is_none());
        let out = r.emit("matching", None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1, serde_yaml::from_str::<Value>("{prob: 0.5, presets: {normal: 0.15, rare: 0.4}}").unwrap());
        let all = r.emit("", None);
        assert_eq!(all.iter().map(|(f, _)| *f).collect::<Vec<_>>(), vec!["cfg", "agent"]);
        assert_eq!(all[1].1, serde_yaml::from_str::<Value>("{mode: on}").unwrap(), "secrets never emit");
        assert!(under("matching.prob", "matching") && !under("matchingx", "matching"));
    }
}
