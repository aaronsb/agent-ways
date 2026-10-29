//! Reading a merged sensor file into a [`SensorSpec`], checking each field
//! as it goes.

use std::collections::BTreeMap;

use serde_yaml::{Mapping, Value};

use crate::path::JsonPath;
use crate::template::Template;
use crate::*;

/// The fields of the shared core and attend's block, which every attend
/// kind takes.
const CORE: &[&str] = &[
    "spec", "name", "description", "kind", "enabled", "interval", "min_interval", "threshold", "decay_threshold", "lane", "author", "lane_exception",
    "requires", "triggers", "state",
];
const CRATE: &[&str] = &["crate"];
const COMMAND: &[&str] = &["argv", "env", "cwd", "timeout", "output"];
const MCP: &[&str] = &["server", "tool", "args", "allow_unannotated", "probe"];
const WAYS: &[&str] = &["spec", "name", "description", "kind", "enabled", "rules", "dispatch"];

/// The outcome of [`parse`]: the sensor when nothing makes it invalid, and
/// every problem found.
#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub spec: Option<SensorSpec>,
    pub problems: Vec<Problem>,
}

/// Parse and check the merged file of the sensor `name` (the file's stem).
pub fn parse(name: &str, doc: &Value) -> Parsed {
    let mut p = Reader { problems: Vec::new() };
    let spec = p.sensor(name, doc);
    let mut problems = p.problems;
    let spec = match spec {
        Some(s) if !problems.iter().any(|x| x.severity == Severity::Invalid) => {
            if let Some(m) = s.lane_refusal() {
                problems.push(Problem { at: "lane".into(), message: m, severity: Severity::Error });
            }
            problems.extend(state_size(&s));
            Some(s)
        }
        _ => None,
    };
    Parsed { spec, problems }
}

/// A warning when the declared maximums could exceed the state's size bound.
fn state_size(s: &SensorSpec) -> Option<Problem> {
    let total: u64 = 2 + s.state.iter().map(|(n, f)| f.max_bytes(n)).sum::<u64>();
    (total > bounds::STATE_BYTES).then(|| Problem {
        at: "state".into(),
        message: format!("the fields' maximums could serialize to {total} bytes, past the {} byte bound; a write that large is refused", bounds::STATE_BYTES),
        severity: Severity::Warning,
    })
}

struct Reader {
    problems: Vec<Problem>,
}

impl Reader {
    fn bad(&mut self, at: impl Into<String>, message: impl Into<String>) {
        self.problems.push(Problem::invalid(at, message));
    }

    fn sensor(&mut self, stem: &str, doc: &Value) -> Option<SensorSpec> {
        let Some(m) = doc.as_mapping() else {
            self.bad("", "a sensor file is a mapping of fields");
            return None;
        };
        for k in m.keys().filter(|k| !k.is_string()) {
            self.bad(show(k), "a field name is text");
        }
        let spec = match m.get("spec") {
            None => SPEC_VERSION,
            Some(v) => match v.as_u64() {
                Some(n) if (1..=SPEC_VERSION).contains(&n) => n,
                Some(n) => {
                    self.bad("spec", format!("spec {n} is later than this host reads ({SPEC_VERSION})"));
                    return None;
                }
                None => {
                    self.bad("spec", "spec is a whole number");
                    return None;
                }
            },
        };
        if !is_name(stem) {
            self.bad("name", format!("'{stem}' is not a sensor name (letters, digits, - and _)"));
        }
        if let Some(n) = m.get("name") {
            match n.as_str() {
                Some(n) if n == stem => {}
                Some(n) => self.bad("name", format!("the file names the sensor '{n}' but is named {stem}.yaml; a sensor's name is its file's")),
                None => self.bad("name", "name is text"),
            }
        }
        let kind = match m.get("kind").map(|k| k.as_str().and_then(Kind::parse)) {
            Some(Some(k)) => k,
            Some(None) => {
                self.bad("kind", format!("kind {} is not one of crate, command, mcp or ways", show(&m["kind"])));
                return None;
            }
            None => {
                self.bad("kind", "kind is required: crate, command, mcp or ways");
                return None;
            }
        };
        let own: &[&str] = match kind {
            Kind::Crate => CRATE,
            Kind::Command => COMMAND,
            Kind::Mcp => MCP,
            Kind::Ways => &[],
        };
        let mut extra = Mapping::new();
        for (k, v) in m {
            let Some(k) = k.as_str() else { continue };
            let known = match kind {
                Kind::Ways => WAYS.contains(&k),
                _ => CORE.contains(&k) || own.contains(&k),
            };
            match (known, kind) {
                (true, _) => {}
                (false, Kind::Crate) => {
                    extra.insert(Value::String(k.to_string()), v.clone());
                }
                (false, _) => self.bad(k, format!("unknown field for a {} sensor", kind.as_str())),
            }
        }
        let description = self.text(m, "description").unwrap_or_default();
        let enabled = self.boolean(m, "enabled").unwrap_or(true);
        if kind == Kind::Ways {
            let block = m.iter().filter(|(k, _)| matches!(k.as_str(), Some("rules" | "dispatch"))).map(|(k, v)| (k.clone(), v.clone())).collect();
            return Some(SensorSpec {
                spec,
                name: stem.to_string(),
                description,
                kind,
                enabled,
                cadence: DEFAULTS,
                declared_lane: Lane::Event,
                author: None,
                lane_exception: None,
                requires: Vec::new(),
                triggers: Vec::new(),
                state: BTreeMap::new(),
                block: Block::Ways(block),
            });
        }
        let cadence = self.cadence(m);
        let declared_lane = match m.get("lane").map(|v| v.as_str()) {
            None => Lane::Event,
            Some(Some("event")) => Lane::Event,
            Some(Some("message")) => Lane::Message,
            Some(_) => {
                self.bad("lane", "lane is event or message");
                Lane::Event
            }
        };
        let author = self.text(m, "author");
        if author.is_some() && kind != Kind::Crate {
            self.bad("author", "a command or mcp sensor names its author on each trigger");
        }
        let lane_exception = self.text(m, "lane_exception");
        let requires = self.strings(m, "requires").unwrap_or_default();
        let state = self.state(kind, m);
        let triggers = self.triggers(m, &state);
        if kind == Kind::Crate {
            if !triggers.is_empty() {
                self.bad("triggers", "a crate sensor's poll is compiled and takes no triggers");
            }
            if !state.is_empty() {
                self.bad("state", "a crate sensor keeps its state in its code and declares none");
            }
        }
        let block = match kind {
            Kind::Crate => match self.text(m, "crate") {
                Some(id) => Block::Crate(CrateBlock { id, fields: extra }),
                None => {
                    self.bad("crate", "a crate sensor names the compiled sensor it configures in `crate`");
                    return None;
                }
            },
            Kind::Command => Block::Command(self.command(m, &state)?),
            Kind::Mcp => Block::Mcp(self.mcp(m)?),
            Kind::Ways => unreachable!("returned above"),
        };
        Some(SensorSpec { spec, name: stem.to_string(), description, kind, enabled, cadence, declared_lane, author, lane_exception, requires, triggers, state, block })
    }

    fn cadence(&mut self, m: &Mapping) -> Cadence {
        let interval = self.whole(m, "interval", 1, 7 * 86_400).unwrap_or(DEFAULTS.interval);
        let min_interval = self.whole(m, "min_interval", 1, 7 * 86_400).unwrap_or(DEFAULTS.min_interval.min(interval));
        if min_interval > interval {
            self.bad("min_interval", format!("min_interval {min_interval} is longer than interval {interval}"));
        }
        let threshold = match m.get("threshold") {
            None => DEFAULTS.threshold,
            Some(v) => match v.as_f64() {
                Some(t) if (0.0..=100.0).contains(&t) => t,
                _ => {
                    self.bad("threshold", "threshold is a number from 0 to 100");
                    DEFAULTS.threshold
                }
            },
        };
        let decay_threshold = self.whole(m, "decay_threshold", 1, u32::MAX as u64).map(|n| n as u32).unwrap_or(DEFAULTS.decay_threshold);
        Cadence { interval, min_interval, threshold, decay_threshold }
    }

    fn command(&mut self, m: &Mapping, state: &BTreeMap<String, StateField>) -> Option<CommandBlock> {
        let argv = self.strings(m, "argv").unwrap_or_default();
        if argv.is_empty() {
            self.bad("argv", "a command sensor's argv is a list: the program, then its arguments");
            return None;
        }
        let output = match m.get("output").map(|v| v.as_str()) {
            Some(Some("json")) => Output::Json,
            Some(Some("lines")) => Output::Lines,
            _ => {
                self.bad("output", "output is json or lines");
                return None;
            }
        };
        if output == Output::Lines && !state.is_empty() {
            self.bad("state", "a state block reads a JSON poll result; set output: json");
        }
        let timeout = self.whole(m, "timeout", 1, bounds::COMMAND_TIMEOUT_MAX).unwrap_or(bounds::COMMAND_TIMEOUT);
        Some(CommandBlock { argv, env: self.text_map(m, "env"), cwd: self.text(m, "cwd"), timeout, output })
    }

    fn mcp(&mut self, m: &Mapping) -> Option<McpBlock> {
        let server = match m.get("server") {
            Some(Value::String(s)) if !s.is_empty() => Server::Named(s.clone()),
            Some(Value::Mapping(s)) => {
                let Some(command) = self.text(s, "command") else {
                    self.bad("server.command", "an inline server names its command");
                    return None;
                };
                for k in s.keys().filter_map(Value::as_str).filter(|k| !["command", "args", "env"].contains(k)) {
                    self.bad(format!("server.{k}"), "unknown field of an inline server: command, args or env");
                }
                Server::Inline { command, args: self.strings(s, "args").unwrap_or_default(), env: self.text_map(s, "env") }
            }
            _ => {
                self.bad("server", "server names a server of Claude Code's MCP configuration, or defines one with command, args and env");
                return None;
            }
        };
        let Some(tool) = self.text(m, "tool") else {
            self.bad("tool", "an mcp sensor names the tool it calls");
            return None;
        };
        let args = self.mapping(m, "args").unwrap_or_default();
        let allow_unannotated = self.boolean(m, "allow_unannotated").unwrap_or(false);
        let probe = match m.get("probe") {
            None | Some(Value::Null) => None,
            Some(Value::Mapping(p)) => Some(ProbeRecord {
                server: self.text(p, "server").unwrap_or_default(),
                tool: self.text(p, "tool").unwrap_or_default(),
                args: self.mapping(p, "args").unwrap_or_default(),
                calls_match: self.boolean(p, "calls_match").unwrap_or(false),
                confirmed: self.boolean(p, "confirmed").unwrap_or(false),
                at: self.text(p, "at"),
            }),
            Some(_) => {
                self.bad("probe", "probe is the record `attend sensor probe` writes");
                None
            }
        };
        Some(McpBlock { server, tool, args, allow_unannotated, probe })
    }

    fn state(&mut self, kind: Kind, m: &Mapping) -> BTreeMap<String, StateField> {
        let mut out = BTreeMap::new();
        let Some(v) = m.get("state") else { return out };
        let Some(fields) = v.as_mapping() else {
            self.bad("state", "state is a mapping from field name to its declaration");
            return out;
        };
        if fields.len() > bounds::STATE_FIELDS {
            self.bad("state", format!("{} fields declared; the bound is {}", fields.len(), bounds::STATE_FIELDS));
        }
        for (k, decl) in fields {
            let name = show(k);
            let at = format!("state.{name}");
            let ok = name.len() <= bounds::STATE_NAME
                && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !ok {
                self.bad(&at, format!("a state field is named [a-z][a-z0-9_]* in at most {} characters", bounds::STATE_NAME));
                continue;
            }
            let Some(d) = decl.as_mapping() else {
                self.bad(&at, "a state field is a mapping: type, and max or from");
                continue;
            };
            for f in d.keys().filter_map(Value::as_str).filter(|f| !["type", "max", "from"].contains(f)) {
                self.bad(format!("{at}.{f}"), "unknown field of a state declaration: type, max or from");
            }
            let ty = match d.get("type").and_then(Value::as_str) {
                Some("number") => StateType::Number,
                Some("string") => StateType::String,
                Some("boolean") => StateType::Boolean,
                _ => {
                    self.bad(format!("{at}.type"), "type is number, string or boolean");
                    continue;
                }
            };
            let max = match (ty, d.get("max")) {
                (StateType::String, None) => bounds::STRING_MAX_DEFAULT,
                (StateType::String, Some(v)) => match v.as_u64() {
                    Some(n) if (1..=bounds::STRING_MAX).contains(&n) => n,
                    _ => {
                        self.bad(format!("{at}.max"), format!("max is a whole number of characters from 1 to {}", bounds::STRING_MAX));
                        continue;
                    }
                },
                (_, Some(_)) => {
                    self.bad(format!("{at}.max"), "max bounds a string field only");
                    continue;
                }
                (_, None) => 0,
            };
            let from = match d.get("from") {
                None => None,
                Some(Value::String(p)) => match JsonPath::parse(p) {
                    Ok(p) if p.is_wild() => {
                        self.bad(format!("{at}.from"), format!("`{}` can select many values; a state field takes one scalar", p.text));
                        continue;
                    }
                    Ok(p) => Some(p),
                    Err(e) => {
                        self.bad(format!("{at}.from"), e);
                        continue;
                    }
                },
                Some(_) => {
                    self.bad(format!("{at}.from"), "from is a path such as \"$.messages[0].ts\"");
                    continue;
                }
            };
            if from.is_none() && kind == Kind::Mcp {
                self.problems.push(Problem {
                    at: at.clone(),
                    message: "an mcp sensor fills a field only through from; this one stays at its starting value".into(),
                    severity: Severity::Warning,
                });
            }
            out.insert(name, StateField { ty, max, from });
        }
        out
    }

    fn triggers(&mut self, m: &Mapping, state: &BTreeMap<String, StateField>) -> Vec<Trigger> {
        let mut out: Vec<Trigger> = Vec::new();
        let Some(v) = m.get("triggers") else { return out };
        let Some(items) = v.as_sequence() else {
            self.bad("triggers", "triggers is a list");
            return out;
        };
        for (i, t) in items.iter().enumerate() {
            let Some(t) = t.as_mapping() else {
                self.bad(format!("triggers[{i}]"), "a trigger is a mapping");
                continue;
            };
            let name = match t.get("name").and_then(Value::as_str) {
                Some(n) if is_name(n) => n.to_string(),
                Some(n) => {
                    self.bad(format!("triggers[{i}].name"), format!("'{n}' is not a trigger name (letters, digits, - and _)"));
                    continue;
                }
                None => {
                    self.bad(format!("triggers[{i}]"), "a trigger has a name, so the settings key path can address it");
                    continue;
                }
            };
            let at = format!("triggers.{name}");
            if out.iter().any(|o| o.name == name) {
                self.bad(&at, "two triggers share this name");
                continue;
            }
            for f in t.keys().filter_map(Value::as_str) {
                if !["name", "path", "key", "on", "where", "match", "state", "author", "magnitude", "description"].contains(&f) {
                    self.bad(format!("{at}.{f}"), "unknown field of a trigger");
                }
            }
            let match_ = match t.get("match").map(|v| v.as_str()) {
                None | Some(Some("all")) => Match::All,
                Some(Some("any")) => Match::Any,
                Some(_) => {
                    self.bad(format!("{at}.match"), "match is all or any");
                    Match::All
                }
            };
            let Some(test) = self.test(&at, t, state) else { continue };
            let author = self.text(t, "author");
            if let (Test::State(_), Some(a)) = (&test, &author) {
                if !state.contains_key(a) {
                    self.bad(format!("{at}.author"), format!("a state trigger's author names a declared state field, and {a} is none"));
                }
            }
            let magnitude = match t.get("magnitude") {
                None => None,
                Some(v) => match v.as_f64() {
                    Some(x) if (0.0..=100.0).contains(&x) => Some(x),
                    _ => {
                        self.bad(format!("{at}.magnitude"), "magnitude is a number from 0 to 100");
                        None
                    }
                },
            };
            let description = match t.get("description") {
                None => None,
                Some(Value::String(s)) => match Template::parse(s) {
                    Ok(tpl) => {
                        for f in tpl.state_fields().filter(|f| !state.contains_key(*f)) {
                            self.bad(format!("{at}.description"), format!("{{state.{f}}} or {{prev.{f}}} reads no declared state field"));
                        }
                        if matches!(test, Test::State(_)) && tpl.reads_item() {
                            self.bad(format!("{at}.description"), "a state trigger sees no item; read {state.<field>} instead");
                        }
                        Some(tpl)
                    }
                    Err(e) => {
                        self.bad(format!("{at}.description"), e);
                        None
                    }
                },
                Some(_) => {
                    self.bad(format!("{at}.description"), "description is text");
                    None
                }
            };
            out.push(Trigger { name, test, match_, author, magnitude, description });
        }
        out
    }

    fn test(&mut self, at: &str, t: &Mapping, state: &BTreeMap<String, StateField>) -> Option<Test> {
        match (t.get("state"), t.get("path")) {
            (Some(_), Some(_)) => {
                self.bad(at, "a trigger tests state, or path with on; not both");
                None
            }
            (Some(s), None) => {
                for f in ["key", "on", "where"].into_iter().filter(|f| t.contains_key(*f)) {
                    self.bad(format!("{at}.{f}"), "a state trigger takes no key, on or where");
                }
                let Some(s) = s.as_mapping().filter(|s| !s.is_empty()) else {
                    self.bad(format!("{at}.state"), "state maps declared fields to changed, rises or falls");
                    return None;
                };
                let mut out = BTreeMap::new();
                for (f, c) in s {
                    let f = show(f);
                    let Some(decl) = state.get(&f) else {
                        self.bad(format!("{at}.state.{f}"), "names no declared state field");
                        continue;
                    };
                    let c = match c.as_str() {
                        Some("changed") => Change::Changed,
                        Some("rises") => Change::Rises,
                        Some("falls") => Change::Falls,
                        _ => {
                            self.bad(format!("{at}.state.{f}"), "is changed, rises or falls");
                            continue;
                        }
                    };
                    if c != Change::Changed && decl.ty != StateType::Number {
                        self.bad(format!("{at}.state.{f}"), "rises and falls test a number field");
                        continue;
                    }
                    out.insert(f, c);
                }
                Some(Test::State(out))
            }
            (None, Some(p)) => {
                let path = match p.as_str().map(JsonPath::parse) {
                    Some(Ok(p)) => p,
                    Some(Err(e)) => {
                        self.bad(format!("{at}.path"), e);
                        return None;
                    }
                    None => {
                        self.bad(format!("{at}.path"), "path is text, such as \"$.items[*]\"");
                        return None;
                    }
                };
                let on = match t.get("on").and_then(Value::as_str).and_then(On::parse) {
                    Some(o) => o,
                    None => {
                        self.bad(format!("{at}.on"), "on is added, removed, changed, enters or leaves");
                        return None;
                    }
                };
                let where_ = self.where_(&format!("{at}.where"), t.get("where"));
                if matches!(on, On::Enters | On::Leaves) && where_.is_empty() {
                    self.bad(format!("{at}.where"), "on: enters and on: leaves test a where condition");
                }
                Some(Test::Items { path, key: self.text(t, "key"), on, where_ })
            }
            (None, None) => {
                self.bad(at, "a trigger tests state, or path with on");
                None
            }
        }
    }

    fn where_(&mut self, at: &str, v: Option<&Value>) -> Where {
        let mut out = Where::new();
        let Some(v) = v else { return out };
        let Some(m) = v.as_mapping() else {
            self.bad(at, "where maps properties to a value or to operators");
            return out;
        };
        for (k, c) in m {
            let prop = show(k);
            let at = format!("{at}.{prop}");
            let cond = match c {
                Value::Mapping(ops) => {
                    let mut list = Vec::new();
                    for (op, arg) in ops {
                        let op = show(op);
                        if matches!(arg, Value::Mapping(_)) {
                            self.bad(format!("{at}.{op}"), "conditions do not nest");
                            continue;
                        }
                        let num = |a: &Value| a.as_f64();
                        let parsed = match op.as_str() {
                            "equals" => Some(Op::Equals(arg.clone())),
                            "not" => Some(Op::Not(arg.clone())),
                            "contains" => Some(Op::Contains(arg.clone())),
                            "in" => arg.as_sequence().map(|s| Op::In(s.clone())),
                            "like" => arg.as_str().map(|s| Op::Like(s.to_string())),
                            "matches" => arg.as_str().map(|s| Op::Matches(s.to_string())),
                            "above" => num(arg).map(Op::Above),
                            "below" => num(arg).map(Op::Below),
                            _ => {
                                self.bad(format!("{at}.{op}"), "not an operator: equals, not, in, contains, like, matches, above or below");
                                continue;
                            }
                        };
                        match parsed {
                            Some(p) => list.push(p),
                            None => self.bad(format!("{at}.{op}"), "takes a list for in, text for like and matches, a number for above and below"),
                        }
                    }
                    Condition::Ops(list)
                }
                Value::Sequence(_) => {
                    self.bad(&at, "a list is written as `in: [...]`");
                    continue;
                }
                v => Condition::Is(v.clone()),
            };
            out.insert(prop, cond);
        }
        out
    }

    fn text(&mut self, m: &Mapping, k: &str) -> Option<String> {
        match m.get(k)? {
            Value::String(s) => Some(s.clone()),
            Value::Null => None,
            _ => {
                self.bad(k, format!("{k} is text"));
                None
            }
        }
    }

    fn boolean(&mut self, m: &Mapping, k: &str) -> Option<bool> {
        match m.get(k)? {
            Value::Bool(b) => Some(*b),
            _ => {
                self.bad(k, format!("{k} is true or false"));
                None
            }
        }
    }

    fn whole(&mut self, m: &Mapping, k: &str, min: u64, max: u64) -> Option<u64> {
        let v = m.get(k)?;
        match v.as_u64() {
            Some(n) if (min..=max).contains(&n) => Some(n),
            _ => {
                self.bad(k, format!("{k} is a whole number from {min} to {max}"));
                None
            }
        }
    }

    fn strings(&mut self, m: &Mapping, k: &str) -> Option<Vec<String>> {
        let v = m.get(k)?;
        match v.as_sequence().map(|s| s.iter().map(|i| i.as_str().map(str::to_string)).collect::<Option<Vec<_>>>()) {
            Some(Some(list)) => Some(list),
            _ => {
                self.bad(k, format!("{k} is a list of text"));
                None
            }
        }
    }

    fn mapping(&mut self, m: &Mapping, k: &str) -> Option<Mapping> {
        match m.get(k)? {
            Value::Mapping(m) => Some(m.clone()),
            Value::Null => None,
            _ => {
                self.bad(k, format!("{k} is a mapping"));
                None
            }
        }
    }

    fn text_map(&mut self, m: &Mapping, k: &str) -> BTreeMap<String, String> {
        let Some(map) = self.mapping(m, k) else { return BTreeMap::new() };
        let mut out = BTreeMap::new();
        for (key, v) in &map {
            match (key.as_str(), v) {
                (Some(key), Value::String(s)) => {
                    out.insert(key.to_string(), s.clone());
                }
                (Some(key), Value::Number(n)) => {
                    out.insert(key.to_string(), n.to_string());
                }
                _ => self.bad(format!("{k}.{}", show(key)), "an entry maps a name to text"),
            }
        }
        out
    }
}

/// A YAML value as one short line.
fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
    }
}
