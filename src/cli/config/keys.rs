//! Completion for the dotted keys `config get`, `config set` and `config explain` take, and
//! how those keys are written: TOML's dotted keys, where a key with a dot in its name is
//! quoted (`tasks."build.sh"`).
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

/// The byte offsets of the dots in `path` that separate its keys: those outside quotes.
fn separators(path: &str) -> Vec<usize> {
    let mut dots = vec![];
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in path.char_indices() {
        match quote {
            Some(_) if escaped => escaped = false,
            Some('"') if c == '\\' => escaped = true,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '.' => dots.push(i),
            None => {}
        }
    }
    dots
}

/// A dotted key path split into its keys. A quoted key is read as TOML reads it, escapes
/// included, so `tasks."build.sh"` and `tasks."build\u002esh"` are both `tasks` and `build.sh`.
/// An unquoted key is taken as written, so `tools.asdf:mise-plugins/mise-foo` needs no quotes.
pub(super) fn split_path(path: &str) -> Vec<String> {
    split_keys(path).into_iter().map(|(key, _)| key).collect()
}

/// [`split_path`], with whether each key was quoted: a quoted key's edges are the user's own,
/// so [`rejoin`] never joins one to its neighbors.
pub(super) fn split_keys(path: &str) -> Vec<(String, bool)> {
    let mut keys = vec![];
    let mut start = 0;
    for dot in separators(path).into_iter().chain([path.len()]) {
        let key = &path[start..dot];
        let quoted = key
            .starts_with(['"', '\''])
            .then(|| toml_edit::Key::parse(key).ok())
            .flatten()
            .and_then(|parsed| parsed.into_iter().next());
        keys.push(match quoted {
            Some(k) => (k.get().to_string(), true),
            None => (key.to_string(), false),
        });
        start = dot + 1;
    }
    keys
}

/// `keys`, with unquoted keys joined back with dots where that reaches further into the file,
/// as `exists` says of each path from the top. A shell removes the quotes from
/// `tasks."build.sh"` unless the whole path is quoted too, and bash inserts a completion as
/// written, so `tasks.build.sh` still has to find the task `build.sh`. The keys as split win a
/// tie, so a `build` task with a key `sh` is taken first; keys past what the file has stay
/// split, and a quoted key is never joined, since its quotes say where it ends.
pub(super) fn rejoin(keys: Vec<(String, bool)>, exists: impl Fn(&[String]) -> bool) -> Vec<String> {
    let (reached, mut path) = furthest(&keys, 0, &mut vec![], &exists);
    path.extend(keys[reached..].iter().map(|(key, _)| key.clone()));
    path
}

/// How far into `keys` the file goes from `start`, below the keys already `found`, and the
/// path that gets there.
fn furthest(
    keys: &[(String, bool)],
    start: usize,
    found: &mut Vec<String>,
    exists: &dyn Fn(&[String]) -> bool,
) -> (usize, Vec<String>) {
    let mut best = (start, found.clone());
    for end in start + 1..=keys.len() {
        // A run of several keys is joined, which only unquoted ones may be.
        if best.0 == keys.len() || (end > start + 1 && keys[start..end].iter().any(|(_, q)| *q)) {
            break;
        }
        let run: Vec<&str> = keys[start..end]
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        found.push(run.join("."));
        if exists(found) {
            let reached = furthest(keys, end, found, exists);
            if reached.0 > best.0 {
                best = reached;
            }
        }
        found.pop();
    }
    best
}

/// A key as it is written in a path: as is, unless its name would not split back into itself,
/// such as the task `build.sh`, which TOML quotes.
pub(super) fn quote_key(name: &str) -> String {
    let plain = !name.is_empty()
        && !name.contains(['.', '"', '\'', '\\'])
        && !name.contains(|c: char| c.is_whitespace() || c.is_control());
    if plain {
        name.to_string()
    } else {
        toml_edit::Key::new(name).display_repr().into_owned()
    }
}

/// Candidates for a dotted key being typed as `prefix`, against the TOML at `file`.
pub(super) fn candidates(prefix: &str, file: Option<&Path>) -> Vec<Candidate<'static>> {
    // `config set KEY=VALUE`: past the `=` is a value, not a key.
    if prefix.contains('=') {
        return vec![];
    }
    // The parent stays as typed, so every candidate starts with what the shell has.
    let parent = separators(prefix).last().map_or("", |&dot| &prefix[..dot]);
    let path: Vec<String> = if parent.is_empty() {
        vec![]
    } else {
        split_path(parent)
    };

    let root = &*SCHEMA;
    let mut nodes = vec![];
    expand(root, root, &mut nodes);
    for segment in &path {
        nodes = child(root, &nodes, segment.as_str());
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
                quote_key(&key)
            } else {
                format!("{parent}.{}", quote_key(&key))
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
fn file_keys(file: Option<&Path>, path: &[String]) -> Vec<String> {
    let Some(content) = file.and_then(|file| std::fs::read_to_string(file).ok()) else {
        return vec![];
    };
    let Ok(mut value) = content.parse::<toml::Table>().map(toml::Value::Table) else {
        return vec![];
    };
    for segment in path {
        match value.get(segment.as_str()) {
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

    #[test]
    fn quoted_keys_split_as_toml_reads_them() {
        assert_eq!(split_path("tasks.build"), ["tasks", "build"]);
        assert_eq!(
            split_path(r#"tasks."build.sh".run"#),
            ["tasks", "build.sh", "run"]
        );
        assert_eq!(
            split_path(r#"tasks."build\u002esh""#),
            ["tasks", "build.sh"]
        );
        assert_eq!(split_path("tasks.'build.sh'"), ["tasks", "build.sh"]);
        assert_eq!(
            split_path(r#"tasks."say \"hi\".sh""#),
            ["tasks", r#"say "hi".sh"#]
        );
        assert_eq!(
            split_path("tools.asdf:mise-plugins/mise-foo.version"),
            ["tools", "asdf:mise-plugins/mise-foo", "version"]
        );
    }

    #[test]
    fn a_quoted_key_splits_back_into_its_name() {
        for name in [
            "build",
            "asdf:mise-plugins/mise-foo",
            "build.sh",
            r#"say "hi""#,
            r#"it's "x""#,
            "a b",
            "",
        ] {
            let path = format!("tasks.{}", quote_key(name));
            assert_eq!(split_path(&path), ["tasks", name], "{path}");
        }
        assert_eq!(quote_key("build"), "build");
        assert_eq!(quote_key("build.sh"), r#""build.sh""#);
    }

    #[test]
    fn a_task_with_a_dot_in_its_name_completes_quoted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mise.toml");
        std::fs::write(&file, "[tasks.\"hello.sh\"]\nrun = \"echo\"\n").unwrap();
        assert_eq!(values("tasks.", Some(&file)), [r#"tasks."hello.sh""#]);
        let task = values(r#"tasks."hello.sh"."#, Some(&file));
        assert!(
            task.contains(&r#"tasks."hello.sh".run"#.to_string()),
            "{task:?}"
        );
    }

    #[test]
    fn a_path_the_shell_unquoted_finds_the_dotted_key() {
        let keys = |path: &str| split_keys(path);
        let exists = |known: &'static [&'static [&'static str]]| {
            move |path: &[String]| {
                known
                    .iter()
                    .any(|k| k.starts_with(&path.iter().map(String::as_str).collect::<Vec<_>>()))
            }
        };
        // only `hello.sh`: the split keys are joined back
        let only_dotted: &[&[&str]] = &[&["tasks", "hello.sh", "run"]];
        assert_eq!(
            rejoin(keys("tasks.hello.sh.run"), exists(only_dotted)),
            ["tasks", "hello.sh", "run"]
        );
        // a `hello` task is in the way, so the path stays as split
        let both: &[&[&str]] = &[&["tasks", "hello.sh", "run"], &["tasks", "hello", "sh"]];
        assert_eq!(
            rejoin(keys("tasks.hello.sh"), exists(both)),
            ["tasks", "hello", "sh"]
        );
        // both exist, but only hello.sh goes on: it is the one meant
        let longer: &[&[&str]] = &[&["tasks", "hello.sh", "run"], &["tasks", "hello", "run"]];
        assert_eq!(
            rejoin(keys("tasks.hello.sh.run"), exists(longer)),
            ["tasks", "hello.sh", "run"]
        );
        // a key the file lacks yet, below the dotted name
        assert_eq!(
            rejoin(keys("tasks.hello.sh.dir"), exists(only_dotted)),
            ["tasks", "hello.sh", "dir"]
        );
        // quoted keys keep the edges the user gave them, even where `a.b` exists
        let dotted: &[&[&str]] = &[&["_", "a.b"]];
        assert_eq!(
            rejoin(keys(r#"_."a"."b""#), exists(dotted)),
            ["_", "a", "b"]
        );
        assert_eq!(rejoin(keys("_.a.b"), exists(dotted)), ["_", "a.b"]);
        // nothing matches: as split
        assert_eq!(
            rejoin(keys("tasks.new.sh"), exists(&[])),
            ["tasks", "new", "sh"]
        );
    }
}
