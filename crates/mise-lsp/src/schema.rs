//! A walk over mise's JSON schema by key path.
//!
//! The host hands the server `schema/mise.json`, which is generated from the types mise parses
//! config with, so it names every key mise reads.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

/// Deep enough for every chain in the schema; a bound so a cyclic `$ref` cannot recurse forever.
const MAX_DEPTH: usize = 32;

pub(crate) struct Schema {
    root: Value,
}

impl Schema {
    pub(crate) fn new(root: Value) -> Self {
        Self { root }
    }

    pub(crate) fn root(&self) -> &Value {
        &self.root
    }

    /// `node` and every schema it stands for through `$ref` and the combinators.
    fn expand<'a>(&'a self, node: &'a Value, out: &mut Vec<&'a Value>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        out.push(node);
        if let Some(target) = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix('#'))
            .and_then(|pointer| self.root.pointer(pointer))
        {
            self.expand(target, out, depth + 1);
        }
        for combinator in ["allOf", "oneOf", "anyOf"] {
            for variant in node
                .get(combinator)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                self.expand(variant, out, depth + 1);
            }
        }
    }

    fn expanded<'a>(&'a self, node: &'a Value) -> Vec<&'a Value> {
        let mut out = vec![];
        self.expand(node, &mut out, 0);
        out
    }

    /// The schemas `key` has under any of `nodes`: a named property, else a matching
    /// `patternProperties` entry, else the schema a table gives its unnamed keys. A numeric
    /// `key` also reaches an array's `items`.
    fn child<'a>(&'a self, nodes: &[&'a Value], key: &str) -> Vec<&'a Value> {
        let mut out = vec![];
        for node in nodes {
            if let Some(schema) = direct_child(node, key) {
                self.expand(schema, &mut out, 0);
            }
        }
        out
    }

    /// The schemas the value at `path` has, from the top of a `mise.toml`.
    pub(crate) fn at<S: AsRef<str>>(&self, path: &[S]) -> Vec<&Value> {
        let mut nodes = self.expanded(&self.root);
        for segment in path {
            nodes = self.child(&nodes, segment.as_ref());
        }
        nodes
    }

    /// The keys the schema names for the table at `path`, with the schema of each.
    pub(crate) fn keys<S: AsRef<str>>(&self, path: &[S]) -> Vec<(&str, &Value)> {
        let mut keys: Vec<(&str, &Value)> = self
            .at(path)
            .into_iter()
            .filter_map(|node| node.get("properties").and_then(Value::as_object))
            .flatten()
            .map(|(key, schema)| (key.as_str(), schema))
            .collect();
        keys.sort_unstable_by_key(|(key, _)| *key);
        keys.dedup_by_key(|(key, _)| *key);
        keys
    }

    /// The description of the first schema `nodes` stand for that has one.
    pub(crate) fn description(&self, nodes: &[&Value]) -> Option<String> {
        nodes
            .iter()
            .find_map(|node| node.get("description").and_then(Value::as_str))
            .map(str::to_string)
    }

    /// The description of one named key's schema, through its `$ref`.
    pub(crate) fn schema_description(&self, schema: &Value) -> Option<String> {
        self.description(&self.expanded(schema))
    }

    /// Whether the schemas `nodes` stand for are deprecated. A `oneOf` or `anyOf` that has a
    /// current variant is not: `[hooks]` accepts both a deprecated and a current spelling of
    /// each key.
    pub(crate) fn is_deprecated(&self, nodes: &[&Value]) -> bool {
        nodes.first().is_some_and(|node| self.deprecated(node, 0))
    }

    pub(crate) fn schema_is_deprecated(&self, schema: &Value) -> bool {
        self.deprecated(schema, 0)
    }

    fn deprecated(&self, node: &Value, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        if node.get("deprecated").is_some_and(|d| d != false) {
            return true;
        }
        let target = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.strip_prefix('#'))
            .and_then(|pointer| self.root.pointer(pointer));
        if target.is_some_and(|target| self.deprecated(target, depth + 1)) {
            return true;
        }
        let variants = |keyword: &str| {
            node.get(keyword)
                .and_then(Value::as_array)
                .filter(|variants| !variants.is_empty())
        };
        variants("allOf").is_some_and(|all| all.iter().any(|v| self.deprecated(v, depth + 1)))
            || ["oneOf", "anyOf"].iter().any(|keyword| {
                variants(keyword)
                    .is_some_and(|any| any.iter().all(|v| self.deprecated(v, depth + 1)))
            })
    }

    /// The key a deprecated key's description says replaces it, as written there: a dotted path
    /// from the table the deprecated key is in (`task.output`), or from the top of the file
    /// (`env._.path`). Nothing is guessed: a description that names no key has no replacement.
    pub(crate) fn replacement(&self, nodes: &[&Value]) -> Option<String> {
        static NAMED: OnceLock<[Regex; 2]> = OnceLock::new();
        let [alias, instead] = NAMED.get_or_init(|| {
            [
                Regex::new(r"(?i)\b(?:alias|name) (?:for|of) (?:`([A-Za-z0-9_.]+)`|([A-Za-z0-9_.]+)\s*(?:[.;,]|$))").unwrap(),
                Regex::new(r"(?i)\buse `?([A-Za-z0-9_.]+)`? instead").unwrap(),
            ]
        });
        let description = self.description(nodes)?;
        [alias, instead].into_iter().find_map(|re| {
            let captures = re.captures(&description)?;
            let name = captures.get(1).or_else(|| captures.get(2))?.as_str();
            Some(name.trim_end_matches('.').to_string())
        })
    }

    /// Whether every key of `path` is one the schema names, rather than one a table takes for
    /// any name.
    pub(crate) fn declares<S: AsRef<str>>(&self, path: &[S]) -> bool {
        let mut nodes = self.expanded(&self.root);
        for segment in path {
            let mut next = vec![];
            for node in &nodes {
                if let Some(schema) = node
                    .get("properties")
                    .and_then(|properties| properties.get(segment.as_ref()))
                {
                    self.expand(schema, &mut next, 0);
                }
            }
            if next.is_empty() {
                return false;
            }
            nodes = next;
        }
        true
    }

    /// The values a key can be given as a literal: its enum, or both booleans.
    pub(crate) fn values(&self, nodes: &[&Value]) -> Vec<Value> {
        let mut out: Vec<Value> = vec![];
        let mut add = |value: Value| {
            if !out.contains(&value) {
                out.push(value);
            }
        };
        for node in nodes {
            for value in node
                .get("enum")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                add(value.clone());
            }
            if let Some(value) = node.get("const") {
                add(value.clone());
            }
            if node.get("type").and_then(Value::as_str) == Some("boolean") {
                add(Value::Bool(true));
                add(Value::Bool(false));
            }
        }
        out
    }

    /// The JSON type names the schemas at `nodes` accept, for hover.
    pub(crate) fn types(&self, nodes: &[&Value]) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for node in nodes {
            match node.get("type") {
                Some(Value::String(t)) => out.push(t.clone()),
                Some(Value::Array(types)) => {
                    out.extend(types.iter().filter_map(Value::as_str).map(str::to_string));
                }
                _ => {}
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// The schema `node` gives `key`, as written.
fn direct_child<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    node.get("properties")
        .and_then(|properties| properties.get(key))
        .or_else(|| pattern_property(node, key))
        .or_else(|| node.get("additionalProperties").filter(|v| v.is_object()))
        .or_else(|| {
            key.parse::<usize>()
                .ok()
                .and(node.get("items"))
                .filter(|v| v.is_object())
        })
}

fn pattern_property<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    node.get("patternProperties")?
        .as_object()?
        .iter()
        .find(|(pattern, _)| regex::Regex::new(pattern).is_ok_and(|re| re.is_match(key)))
        .map(|(_, schema)| schema)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn sample() -> Schema {
        Schema::new(json!({
            "type": "object",
            "properties": {
                "min_version": {"description": "minimum version of mise", "type": "string"},
                "old": {"deprecated": true, "type": "string"},
                "settings": {"$ref": "#/$defs/settings"},
                "list": {
                    "type": "array",
                    "items": {"type": "object", "properties": {"name": {"type": "string"}}}
                },
                "tools": {
                    "type": "object",
                    "patternProperties": {"^node$": {"type": "string"}},
                    "additionalProperties": {"type": "string"}
                }
            },
            "unevaluatedProperties": false,
            "$defs": {
                "settings": {
                    "type": "object",
                    "description": "mise settings",
                    "unevaluatedProperties": false,
                    "properties": {
                        "color": {"description": "use color\nmore", "type": "boolean"},
                        "mode": {"enum": ["a", "b"]}
                    }
                }
            }
        }))
    }

    #[test]
    fn a_oneof_with_a_current_variant_is_not_deprecated() {
        let schema = Schema::new(json!({
            "properties": {
                "a": {"oneOf": [{"deprecated": true, "type": "string"}, {"type": "array"}]},
                "b": {"oneOf": [{"deprecated": true, "type": "string"}]},
                "c": {"$ref": "#/$defs/old"}
            },
            "$defs": {"old": {"deprecated": true}}
        }));
        let at = |key: &str| schema.is_deprecated(&schema.at(&[key]));
        assert!(!at("a"));
        assert!(at("b"));
        assert!(at("c"));
    }

    #[test]
    fn keys_follow_refs() {
        let schema = sample();
        let keys: Vec<_> = schema.keys::<&str>(&[]).iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["list", "min_version", "old", "settings", "tools"]);
        let keys: Vec<_> = schema.keys(&["settings"]).iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["color", "mode"]);
    }

    #[test]
    fn values_come_from_enums_and_booleans() {
        let schema = sample();
        assert_eq!(
            schema.values(&schema.at(&["settings", "mode"])),
            [json!("a"), json!("b")]
        );
        assert_eq!(
            schema.values(&schema.at(&["settings", "color"])),
            [json!(true), json!(false)]
        );
    }

    #[test]
    fn user_named_keys_get_the_unnamed_schema() {
        let schema = sample();
        assert_eq!(schema.types(&schema.at(&["tools", "whatever"])), ["string"]);
    }
}
