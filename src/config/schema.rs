//! The `schema/mise.json` compiled into mise, and a walk over it by key path.
//!
//! The schema is generated from `mise_config`'s declaration types, so it names every key mise
//! reads. Completion uses it to offer keys, and config loading uses it to suggest the key an
//! unknown one was meant to be.

use std::sync::LazyLock;

use serde_json::Value;

pub static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../schema/mise.json"))
        .expect("schema/mise.json should parse")
});

/// Deep enough for every chain in the schema; a bound so a cyclic `$ref` cannot recurse forever.
const MAX_DEPTH: usize = 32;

/// `node` and every schema it stands for through `$ref` and the combinators.
pub fn expand<'a>(root: &'a Value, node: &'a Value, out: &mut Vec<&'a Value>) {
    expand_at(root, node, out, 0);
}

fn expand_at<'a>(root: &'a Value, node: &'a Value, out: &mut Vec<&'a Value>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    out.push(node);
    if let Some(target) = node
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix('#'))
        .and_then(|pointer| root.pointer(pointer))
    {
        expand_at(root, target, out, depth + 1);
    }
    for combinator in ["allOf", "oneOf", "anyOf"] {
        for variant in node
            .get(combinator)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            expand_at(root, variant, out, depth + 1);
        }
    }
}

/// The schemas `key` has under any of `nodes`: a named property, else a matching
/// `patternProperties` entry, else the schema a table gives its unnamed keys. A numeric `key`
/// also reaches an array's `items`.
pub fn child<'a>(root: &'a Value, nodes: &[&'a Value], key: &str) -> Vec<&'a Value> {
    let mut out = vec![];
    for node in nodes {
        if let Some(schema) = direct_child(node, key) {
            expand(root, schema, &mut out);
        }
    }
    out
}

/// The schema `node` gives `key`, as written: see [`child`].
pub fn direct_child<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
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

/// The schemas the table at `path` has, from the top of a `mise.toml`.
pub fn at(path: &[&str]) -> Vec<&'static Value> {
    let root = &*SCHEMA;
    let mut nodes = vec![];
    expand(root, root, &mut nodes);
    for segment in path {
        nodes = child(root, &nodes, segment);
    }
    nodes
}

/// Whether any schema `schema` stands for is marked deprecated.
pub fn is_deprecated(root: &Value, schema: &Value) -> bool {
    let mut nodes = vec![];
    expand(root, schema, &mut nodes);
    nodes
        .iter()
        .any(|node| node.get("deprecated").is_some_and(|d| d != false))
}

/// The keys the schema names for the table at `path`, deprecated ones left out.
pub fn known_keys(path: &[&str]) -> Vec<&'static str> {
    let root = &*SCHEMA;
    let mut keys: Vec<&str> = at(path)
        .into_iter()
        .filter_map(|node| node.get("properties").and_then(Value::as_object))
        .flatten()
        .filter(|(_, schema)| !is_deprecated(root, schema))
        .map(|(key, _)| key.as_str())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}
