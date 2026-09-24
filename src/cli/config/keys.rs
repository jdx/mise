//! Completion for the dotted keys `config get` and `config set` take.
//!
//! The known keys come from `schema/mise.json`, walked through `$ref`, `allOf`, `oneOf` and
//! `anyOf` so that `tools.node.` reaches a tool's options and `settings.python.` reaches the
//! python settings. Tables whose keys are user-chosen — tools, tasks, env vars — have no names
//! in the schema, so those come from the file the command would read or write.

use serde_json::Value;
use std::collections::{BTreeMap, btree_map};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use usage_rs::complete::{Candidate, CompleteCtx};

static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../schema/mise.json"))
        .expect("schema/mise.json should parse")
});

/// Deep enough for every chain in the schema; a bound so a cyclic `$ref` cannot recurse forever.
const MAX_DEPTH: usize = 32;

/// Candidates for a dotted key being typed as `prefix`, against the TOML at `file`.
pub(super) fn candidates(prefix: &str, file: Option<&Path>) -> Vec<Candidate<'static>> {
    // `config set KEY=VALUE`: past the `=` is a value, not a key.
    if prefix.contains('=') {
        return vec![];
    }
    let (parent, _) = prefix.rsplit_once('.').unwrap_or(("", prefix));
    let path: Vec<&str> = if parent.is_empty() {
        vec![]
    } else {
        parent.split('.').collect()
    };

    let root = &*SCHEMA;
    let mut nodes = vec![];
    expand(root, root, &mut nodes, 0);
    for segment in &path {
        nodes = child(root, &nodes, segment);
    }

    // Keyed by name, so a key the schema knows and the file also sets appears once, with the
    // schema's description.
    let mut keys: BTreeMap<String, Option<String>> = BTreeMap::new();
    for node in &nodes {
        let Some(properties) = node.get("properties").and_then(Value::as_object) else {
            continue;
        };
        for (key, schema) in properties {
            if is_deprecated(root, schema) {
                continue;
            }
            keys.entry(key.clone())
                .or_insert_with(|| description(root, schema));
        }
    }
    for key in file_keys(file, &path) {
        if let btree_map::Entry::Vacant(entry) = keys.entry(key) {
            let description = child(root, &nodes, entry.key())
                .iter()
                .find_map(|node| node.get("description").and_then(Value::as_str))
                .map(str::to_string);
            entry.insert(description);
        }
    }

    keys.into_iter()
        .map(|(key, description)| {
            let value = if parent.is_empty() {
                key
            } else {
                format!("{parent}.{key}")
            };
            match description {
                Some(description) => Candidate::described(value, first_line(&description)),
                None => Candidate::new(value),
            }
        })
        .collect()
}

/// `node` and every schema it stands for through `$ref` and the combinators.
fn expand<'a>(root: &'a Value, node: &'a Value, out: &mut Vec<&'a Value>, depth: usize) {
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
        expand(root, target, out, depth + 1);
    }
    for combinator in ["allOf", "oneOf", "anyOf"] {
        for variant in node
            .get(combinator)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            expand(root, variant, out, depth + 1);
        }
    }
}

/// The schemas `key` has under any of `nodes`: a named property, or else the schema a table
/// gives its unnamed keys.
fn child<'a>(root: &'a Value, nodes: &[&'a Value], key: &str) -> Vec<&'a Value> {
    let mut out = vec![];
    for node in nodes {
        let schema = node
            .get("properties")
            .and_then(|properties| properties.get(key))
            .or_else(|| node.get("additionalProperties").filter(|v| v.is_object()));
        if let Some(schema) = schema {
            expand(root, schema, &mut out, 0);
        }
    }
    out
}

fn description(root: &Value, schema: &Value) -> Option<String> {
    let mut nodes = vec![];
    expand(root, schema, &mut nodes, 0);
    nodes
        .iter()
        .find_map(|node| node.get("description").and_then(Value::as_str))
        .map(str::to_string)
}

fn is_deprecated(root: &Value, schema: &Value) -> bool {
    let mut nodes = vec![];
    expand(root, schema, &mut nodes, 0);
    nodes
        .iter()
        .any(|node| node.get("deprecated").is_some_and(|d| d != false))
}

fn first_line(description: &str) -> String {
    description.lines().next().unwrap_or_default().to_string()
}

/// The keys the table at `path` already has in `file`.
fn file_keys(file: Option<&Path>, path: &[&str]) -> Vec<String> {
    let Some(content) = file.and_then(|file| std::fs::read_to_string(file).ok()) else {
        return vec![];
    };
    let Ok(mut value) = content.parse::<toml::Table>().map(toml::Value::Table) else {
        return vec![];
    };
    for segment in path {
        match value.get(segment) {
            Some(child) => value = child.clone(),
            None => return vec![],
        }
    }
    match value {
        toml::Value::Table(table) => table.keys().cloned().collect(),
        _ => vec![],
    }
}

/// The completer `config get` and `config set` both declare, given the flags they were typed
/// with so far.
pub(super) fn complete(
    ctx: &CompleteCtx<'_>,
    file: Option<&[u8]>,
    global: bool,
    system: bool,
) -> Vec<Candidate<'static>> {
    let file = file.map(|bytes| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()));
    let target = super::target_file(file, global, system).ok().flatten();
    candidates(ctx.prefix, target.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(prefix: &str, file: Option<&Path>) -> Vec<String> {
        candidates(prefix, file)
            .into_iter()
            .map(|c| c.value)
            .collect()
    }

    #[test]
    fn top_level_keys_come_from_the_schema() {
        let keys = values("", None);
        for key in ["env", "tools", "tasks", "settings", "vars"] {
            assert!(keys.contains(&key.to_string()), "{key} missing: {keys:?}");
        }
        let tools = candidates("to", None)
            .into_iter()
            .find(|c| c.value == "tools")
            .unwrap();
        assert_eq!(tools.description.as_deref(), Some("dev tools to use"));
    }

    #[test]
    fn settings_follow_refs() {
        let keys = values("settings.", None);
        assert!(keys.contains(&"settings.jobs".to_string()), "{keys:?}");
        assert!(keys.contains(&"settings.python".to_string()), "{keys:?}");
        let python = values("settings.python.", None);
        assert!(
            python.contains(&"settings.python.uv_venv_auto".to_string()),
            "{python:?}"
        );
    }

    #[test]
    fn deprecated_keys_are_not_offered() {
        let keys = values("settings.", None);
        assert!(
            !keys.contains(&"settings.legacy_version_file".to_string()),
            "{keys:?}"
        );
    }

    #[test]
    fn tool_options_follow_additional_properties() {
        let keys = values("tools.node.", None);
        assert!(
            keys.contains(&"tools.node.postinstall".to_string()),
            "{keys:?}"
        );
        assert!(keys.contains(&"tools.node.version".to_string()), "{keys:?}");
    }

    #[test]
    fn user_named_keys_come_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mise.toml");
        std::fs::write(
            &file,
            "[tools]\nnode = \"22\"\n[tasks.build]\nrun = \"make\"\n[env]\nFOO = \"1\"\n",
        )
        .unwrap();
        assert_eq!(values("tools.", Some(&file)), vec!["tools.node"]);
        assert_eq!(values("env.", Some(&file)), vec!["env.FOO", "env._"]);
        let build = values("tasks.build.", Some(&file));
        assert!(build.contains(&"tasks.build.run".to_string()), "{build:?}");
        assert!(
            build.contains(&"tasks.build.depends".to_string()),
            "{build:?}"
        );
    }

    #[test]
    fn values_are_not_completed_as_keys() {
        assert!(candidates("tools.node=", None).is_empty());
    }
}
