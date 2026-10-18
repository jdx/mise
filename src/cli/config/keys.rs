//! Completion for the dotted keys `config get` and `config set` take.
//!
//! The known keys come from `schema/mise.json`, walked through `$ref`, `allOf`, `oneOf` and
//! `anyOf` so that `tools.node.` reaches a tool's options and `settings.python.` reaches the
//! python settings. Tables whose keys are user-chosen — tools, tasks, env vars — have no names
//! in the schema, so those come from the file the command would read or write.

use serde_json::Value;

use crate::config::schema::{SCHEMA, child, expand, is_deprecated};
use std::collections::{BTreeMap, btree_map};
use std::path::{Path, PathBuf};
use usage_rs::complete::{Candidate, CompleteCtx};

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
    expand(root, root, &mut nodes);
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

fn description(root: &Value, schema: &Value) -> Option<String> {
    let mut nodes = vec![];
    expand(root, schema, &mut nodes);
    nodes
        .iter()
        .find_map(|node| node.get("description").and_then(Value::as_str))
        .map(str::to_string)
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
    override_file: bool,
) -> Vec<Candidate<'static>> {
    let file = file.map(|bytes| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()));
    let target = super::target_file(file, global, system, override_file)
        .ok()
        .flatten();
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
    fn pypi_python_selector_is_a_known_tool_option() {
        let python = SCHEMA
            .pointer("/$defs/tool_pypi_options/properties/python")
            .expect("PyPI selector should be present in the schema");
        assert_eq!(python.get("type").and_then(Value::as_str), Some("string"));
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
