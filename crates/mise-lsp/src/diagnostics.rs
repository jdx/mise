//! Problems in a config: TOML syntax errors, keys and values the schema rejects, and
//! deprecated keys.

use std::collections::HashMap;
use std::ops::Range;

use jsonschema::error::{TypeKind, ValidationErrorKind};
use jsonschema::{ValidationError, Validator};
use serde_json::Value;

use crate::doc::{self, Entry, Parsed};
use crate::keypath;
use crate::schema::Schema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub(crate) struct Diagnostic {
    pub range: Range<usize>,
    /// The key path the problem is about, to tell which problems sit inside which.
    pub path: Vec<String>,
    pub severity: Severity,
    pub message: String,
    /// A stable name for the kind of problem, for clients: `syntax`, `unknown-key`,
    /// `deprecated-key`, `invalid-value`, `unknown-task` or `task-cycle`.
    pub code: &'static str,
    /// The key an unknown one was probably meant to be.
    pub suggestion: Option<String>,
    /// The unknown key an `unknown-key` diagnostic is about, to tell a stale one from a current one.
    pub key: Option<String>,
    pub deprecated: bool,
}

impl Diagnostic {
    pub(crate) fn new(
        range: Range<usize>,
        path: &[String],
        severity: Severity,
        code: &'static str,
        message: String,
    ) -> Self {
        Self {
            range,
            path: path.to_vec(),
            severity,
            message,
            code,
            suggestion: None,
            key: None,
            deprecated: false,
        }
    }
}

/// The diagnostics of a document, and its parse when it has one.
pub(crate) struct Report {
    pub diagnostics: Vec<Diagnostic>,
    pub parsed: bool,
}

pub(crate) fn diagnose(text: &str, schema: &Schema, validator: Option<&Validator>) -> Report {
    let parsed = match doc::parse(text) {
        Ok(parsed) => parsed,
        Err(e) => {
            return Report {
                diagnostics: vec![Diagnostic {
                    range: e.span,
                    path: vec![],
                    severity: Severity::Error,
                    message: e.message,
                    code: "syntax",
                    suggestion: None,
                    key: None,
                    deprecated: false,
                }],
                parsed: false,
            };
        }
    };
    let by_path: HashMap<&[String], &Entry> = parsed
        .entries
        .iter()
        .map(|e| (e.path.as_slice(), e))
        .collect();
    let mut out = vec![];
    if let Some(validator) = validator {
        let cx = Context {
            by_path: &by_path,
            schema,
        };
        for error in validator.iter_errors(&parsed.json) {
            cx.report(&error, &mut out);
        }
    }
    deprecated_keys(text, &parsed, schema, validator, &mut out);
    // A value that fails its schema is not "evaluated", so the table also calls its key
    // unknown. The value's own error says what is wrong; the key is known.
    let failing: Vec<Vec<String>> = out
        .iter()
        .filter(|d| d.code == "invalid-value")
        .map(|d| d.path.clone())
        .collect();
    out.retain(|d| {
        d.code != "unknown-key" || !failing.iter().any(|path| path.starts_with(&d.path))
    });
    Report {
        diagnostics: out,
        parsed: true,
    }
}

struct Context<'a> {
    by_path: &'a HashMap<&'a [String], &'a Entry>,
    schema: &'a Schema,
}

impl Context<'_> {
    fn report(&self, error: &ValidationError, out: &mut Vec<Diagnostic>) {
        let path = pointer_segments(error.instance_path().as_str());
        match error.kind() {
            ValidationErrorKind::AdditionalProperties { unexpected }
            | ValidationErrorKind::UnevaluatedProperties { unexpected } => {
                for name in unexpected {
                    self.unknown_key(&path, name, out);
                }
            }
            ValidationErrorKind::OneOfNotValid { context }
            | ValidationErrorKind::AnyOf { context } => {
                self.report_branches(error, &path, context, out);
            }
            _ => {
                let range = nearest(self.by_path, &path).unwrap_or(0..0);
                push(
                    out,
                    range,
                    &path,
                    Severity::Error,
                    error.to_string(),
                    "invalid-value",
                    None,
                );
            }
        }
    }

    /// A value that must match one of several schemas: say what is wrong with the branch it was
    /// written for. That is the one that accepts its type; when none does, say which types do.
    fn report_branches(
        &self,
        error: &ValidationError,
        path: &[String],
        context: &[Vec<ValidationError>],
        out: &mut Vec<Diagnostic>,
    ) {
        let is_type_mismatch = |e: &ValidationError| {
            matches!(e.kind(), ValidationErrorKind::Type { .. })
                && e.instance_path().as_str() == error.instance_path().as_str()
        };
        let viable = context
            .iter()
            .filter(|branch| !branch.is_empty() && !branch.iter().any(is_type_mismatch))
            .min_by_key(|branch| branch.len());
        if let Some(branch) = viable {
            for e in branch {
                self.report(e, out);
            }
            return;
        }
        let mut expected: Vec<String> = context
            .iter()
            .flatten()
            .filter(|e| is_type_mismatch(e))
            .filter_map(|e| match e.kind() {
                ValidationErrorKind::Type { kind } => Some(match kind {
                    TypeKind::Single(t) => vec![t.to_string()],
                    TypeKind::Multiple(set) => set.into_iter().map(|t| t.to_string()).collect(),
                }),
                _ => None,
            })
            .flatten()
            .collect();
        expected.sort();
        expected.dedup();
        let range = nearest(self.by_path, path).unwrap_or(0..0);
        let message = if expected.is_empty() {
            error.to_string()
        } else {
            format!(
                "expected {}, found {}",
                expected.join(" or "),
                json_type(error.instance())
            )
        };
        push(
            out,
            range,
            path,
            Severity::Error,
            message,
            "invalid-value",
            None,
        );
    }

    fn unknown_key(&self, path: &[String], name: &str, out: &mut Vec<Diagnostic>) {
        let mut child = path.to_vec();
        child.push(name.to_string());
        let Some(range) = self
            .by_path
            .get(child.as_slice())
            .and_then(|e| e.key.clone())
            .or_else(|| nearest(self.by_path, path))
        else {
            return;
        };
        let known = self.schema.keys(path);
        let suggestion = keypath::closest(name, known.iter().map(|(key, _)| *key));
        let mut message = format!("unknown key `{name}`");
        if let Some(suggestion) = suggestion {
            message.push_str(&format!("; did you mean `{suggestion}`?"));
        }
        // mise only warns about a setting it does not know, and still loads the file.
        let severity = if path.first().is_some_and(|first| first == "settings") {
            Severity::Warning
        } else {
            Severity::Error
        };
        push(
            out,
            range,
            &child,
            severity,
            message,
            "unknown-key",
            suggestion.map(str::to_string),
        );
        if let Some(d) = out.last_mut().filter(|d| d.path == child) {
            d.key = Some(name.to_string());
        }
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "table",
    }
}

fn deprecated_keys(
    text: &str,
    parsed: &Parsed,
    schema: &Schema,
    validator: Option<&Validator>,
    out: &mut Vec<Diagnostic>,
) {
    for entry in &parsed.entries {
        let Some(range) = entry.key.clone() else {
            continue;
        };
        let nodes = schema.at(&entry.path);
        if schema.is_deprecated(&nodes) {
            let name = entry.path.join(".");
            let mut message = format!("`{name}` is deprecated");
            if let Some(first) = schema
                .description(&nodes)
                .as_deref()
                .and_then(|d| d.lines().next())
            {
                message.push_str(&format!(": {first}"));
            }
            let replacement = schema
                .replacement(&nodes)
                .and_then(|r| verified_replacement(text, parsed, schema, validator, entry, &r));
            push(
                out,
                range,
                &entry.path,
                Severity::Warning,
                message,
                "deprecated-key",
                replacement,
            );
            let d = out.last_mut().expect("just pushed");
            d.deprecated = true;
            if d.suggestion.is_some() {
                d.key = entry.path.last().cloned();
            }
        }
    }
}

/// The replacement a deprecated key's description names, if rewriting the key to it keeps the
/// document meaning what it did: the new key is one the schema names and does not deprecate, the
/// value goes there without colliding with anything, the text still parses to exactly that, and
/// the schema accepts the value in its new place.
fn verified_replacement(
    text: &str,
    parsed: &Parsed,
    schema: &Schema,
    validator: Option<&Validator>,
    entry: &Entry,
    candidate: &str,
) -> Option<String> {
    let to: Vec<String> = candidate.split('.').map(str::to_string).collect();
    if to.iter().any(String::is_empty) {
        return None;
    }
    let (key, parent) = entry.path.split_last()?;
    let full = [parent, to.as_slice()].concat();
    if full == entry.path || !schema.declares(&full) || schema.is_deprecated(&schema.at(&full)) {
        return None;
    }
    let expected = moved(&parsed.json, parent, key, &to)?;
    let mut edited = text.to_string();
    edited.replace_range(entry.key.clone()?, &replacement_key(candidate));
    if doc::parse(&edited).ok()?.json != expected {
        return None;
    }
    if let Some(validator) = validator {
        // An error at the destination or above it, up to the first key the move adds: `env`
        // failing its `oneOf` is as much a refusal as `env._.path` failing its type.
        let first = [parent, &to[..1]].concat();
        let misplaced = validator
            .iter_errors(&expected)
            .any(|e| pointer_segments(e.instance_path().as_str()).starts_with(&first));
        if misplaced {
            return None;
        }
    }
    Some(candidate.to_string())
}

/// Whether the deprecated key at `path` can still be rewritten to `candidate` in `text`, which
/// may have changed since the diagnostic was made.
pub(crate) fn replacement_holds(
    text: &str,
    schema: &Schema,
    validator: Option<&Validator>,
    path: &[String],
    candidate: &str,
) -> bool {
    let Ok(parsed) = doc::parse(text) else {
        return false;
    };
    parsed
        .entries
        .iter()
        .find(|e| e.path == path)
        .is_some_and(|entry| {
            verified_replacement(text, &parsed, schema, validator, entry, candidate).is_some()
        })
}

/// A dotted replacement as it is written as a key: each part bare or quoted.
pub(crate) fn replacement_key(path: &str) -> String {
    path.split('.')
        .map(keypath::quote)
        .collect::<Vec<_>>()
        .join(".")
}

/// `json` with the value of `key` in the table at `parent` moved to the path `to` below that
/// table, or `None` when something is already there.
fn moved(json: &Value, parent: &[String], key: &str, to: &[String]) -> Option<Value> {
    let mut json = json.clone();
    let mut table = &mut json;
    for segment in parent {
        table = match table {
            Value::Object(map) => map.get_mut(segment)?,
            Value::Array(items) => items.get_mut(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    let value = table.as_object_mut()?.remove(key)?;
    let (last, middle) = to.split_last()?;
    let mut at = table;
    for segment in middle {
        at = at
            .as_object_mut()?
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    let map = at.as_object_mut()?;
    if map.contains_key(last) {
        return None;
    }
    map.insert(last.clone(), value);
    Some(json)
}

/// The value at `path`, or at its nearest ancestor that has a span.
fn nearest(by_path: &HashMap<&[String], &Entry>, path: &[String]) -> Option<Range<usize>> {
    (1..=path.len()).rev().find_map(|n| {
        by_path
            .get(&path[..n])
            .and_then(|e| e.value.clone().or_else(|| e.key.clone()))
    })
}

fn push(
    out: &mut Vec<Diagnostic>,
    range: Range<usize>,
    path: &[String],
    severity: Severity,
    message: String,
    code: &'static str,
    suggestion: Option<String>,
) {
    if !out.iter().any(|d| d.range == range && d.message == message) {
        out.push(Diagnostic {
            range,
            path: path.to_vec(),
            severity,
            message,
            code,
            suggestion,
            key: None,
            deprecated: false,
        });
    }
}

/// The segments of a JSON pointer such as `/tools/node/0`.
fn pointer_segments(pointer: &str) -> Vec<String> {
    pointer
        .split('/')
        .skip(1)
        .map(|s| s.replace("~1", "/").replace("~0", "~"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tests::sample;
    use serde_json::json;

    fn run_with(schema: &Schema, text: &str) -> Vec<(String, String)> {
        let validator = jsonschema::draft202012::new(schema.root()).unwrap();
        diagnose(text, schema, Some(&validator))
            .diagnostics
            .into_iter()
            .map(|d| (text[d.range].to_string(), d.message))
            .collect()
    }

    fn run(text: &str) -> Vec<(String, String)> {
        run_with(&sample(), text)
    }

    #[test]
    fn syntax_errors_are_reported() {
        assert_eq!(run("a = \n").len(), 1);
    }

    #[test]
    fn unknown_keys_get_a_suggestion() {
        let found = run("[settings]\ncolr = true\n");
        assert_eq!(
            found,
            [(
                "colr".to_string(),
                "unknown key `colr`; did you mean `color`?".to_string()
            )]
        );
    }

    #[test]
    fn unknown_settings_are_warnings() {
        let schema = sample();
        let validator = jsonschema::draft202012::new(schema.root()).unwrap();
        let text = "[settings]\ncolr = true\n";
        let report = diagnose(text, &schema, Some(&validator));
        assert_eq!(report.diagnostics[0].severity, Severity::Warning);
        assert_eq!(report.diagnostics[0].code, "unknown-key");
        assert_eq!(report.diagnostics[0].suggestion.as_deref(), Some("color"));
    }

    #[test]
    fn wrong_types_point_at_the_value() {
        let found = run("[settings]\ncolor = \"yes\"\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "\"yes\"");
    }

    #[test]
    fn deprecated_keys_warn() {
        let found = run("old = \"x\"\n");
        assert_eq!(
            found,
            [("old".to_string(), "`old` is deprecated".to_string())]
        );
    }

    fn replacements(schema: &Schema, text: &str) -> Vec<(String, Option<String>)> {
        let validator = jsonschema::draft202012::new(schema.root()).unwrap();
        diagnose(text, schema, Some(&validator))
            .diagnostics
            .into_iter()
            .filter(|d| d.code == "deprecated-key")
            .map(|d| (text[d.range].to_string(), d.suggestion))
            .collect()
    }

    fn renamed() -> Schema {
        Schema::new(json!({
            "type": "object",
            "properties": {
                "old_path": {"deprecated": true, "description": "Deprecated; use env._.path instead.",
                    "type": "array", "items": {"type": "string"}},
                "env": {"type": "object", "properties": {"_": {"type": "object", "properties": {
                    "path": {"type": "array", "items": {"type": "string"}}}}}},
                "settings": {"type": "object", "properties": {
                    "task_output": {"deprecated": true, "description": "Deprecated alias for `task.output`.",
                        "type": "string"},
                    "task": {"type": "object", "properties": {"output": {"type": "string"}}},
                    "gone": {"deprecated": true, "description": "[deprecated] Has no effect.", "type": "string"},
                    "old_mode": {"deprecated": true, "description": "Old name for `mode`.", "type": "string"},
                    "mode": {"type": "integer"},
                    "tool": {"deprecated": true, "description": "use env._.nowhere instead", "type": "string"}
                }}
            },
            "unevaluatedProperties": false
        }))
    }

    #[test]
    fn a_deprecated_key_with_a_named_replacement_gets_one() {
        let schema = renamed();
        assert_eq!(
            replacements(&schema, "old_path = [\"a\"]\n"),
            [("old_path".to_string(), Some("env._.path".to_string()))]
        );
        assert_eq!(
            replacements(&schema, "[settings]\ntask_output = \"x\"\n"),
            [("task_output".to_string(), Some("task.output".to_string()))]
        );
    }

    #[test]
    fn no_replacement_is_offered_without_certainty() {
        let schema = renamed();
        let none = |text: &str| {
            let found = replacements(&schema, text);
            assert_eq!(found.len(), 1, "{text}");
            assert_eq!(found[0].1, None, "{text}");
        };
        // the description names no key
        none("[settings]\ngone = \"x\"\n");
        // the named key is not one the schema declares
        none("[settings]\ntool = \"x\"\n");
        // the value does not fit where it would go
        none("[settings]\nold_mode = \"fast\"\n");
        // something is already there
        none("[settings]\ntask_output = \"x\"\ntask.output = \"y\"\n");
        // the rewritten key would clash with a table written elsewhere
        none("old_path = [\"a\"]\n[env]\nX = \"1\"\n");
    }

    #[test]
    fn prose_after_alias_is_not_a_key() {
        let schema = Schema::new(json!({
            "type": "object",
            "properties": {
                "script": {"deprecated": true, "type": "string",
                    "description": "deprecated alias for run in preinstall hooks"},
                "run": {"type": "string"},
            },
            "unevaluatedProperties": false
        }));
        let found = replacements(&schema, "script = \"x\"\n");
        assert_eq!(found, [("script".to_string(), None)]);
    }

    #[test]
    fn a_value_that_fails_above_the_destination_is_not_moved() {
        let schema =
            Schema::new(serde_json::from_str(include_str!("../../../schema/mise.json")).unwrap());
        assert_eq!(
            replacements(&schema, "env_path = 42\n"),
            [("env_path".to_string(), None)]
        );
    }

    #[test]
    fn a_value_that_fits_is_moved() {
        assert_eq!(
            replacements(&renamed(), "[settings]\nold_mode = 3\n"),
            [("old_mode".to_string(), Some("mode".to_string()))]
        );
    }

    #[test]
    fn mises_own_deprecated_keys_name_their_replacements() {
        let schema =
            Schema::new(serde_json::from_str(include_str!("../../../schema/mise.json")).unwrap());
        for (text, key, replacement) in [
            ("env_path = [\"bin\"]\n", "env_path", "env._.path"),
            ("env_file = \".env\"\n", "env_file", "env._.file"),
            (
                "[settings]\ntask_output = \"prefix\"\n",
                "task_output",
                "task.output",
            ),
        ] {
            assert_eq!(
                replacements(&schema, text),
                [(key.to_string(), Some(replacement.to_string()))],
                "{text}"
            );
        }
    }

    #[test]
    fn a_valid_config_is_clean() {
        assert!(
            run("min_version = \"2024.1.1\"\n[settings]\ncolor = true\nmode = \"a\"\n").is_empty()
        );
    }

    fn task_schema() -> Schema {
        Schema::new(json!({
            "type": "object",
            "properties": {"tasks": {"type": "object", "additionalProperties": {
                "oneOf": [
                    {"type": "string"},
                    {"type": "object", "unevaluatedProperties": false,
                     "properties": {"run": {"type": "string"}, "description": {"type": "string"}}}
                ]
            }}}
        }))
    }

    #[test]
    fn errors_inside_a_oneof_name_the_branch_that_applies() {
        let found = run_with(
            &task_schema(),
            "[tasks.build]\nrun = \"x\"\ndescripton = \"y\"\n",
        );
        assert_eq!(
            found,
            [(
                "descripton".to_string(),
                "unknown key `descripton`; did you mean `description`?".to_string()
            )]
        );
        let found = run_with(&task_schema(), "[tasks.build]\nrun = 1\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "1");
    }

    #[test]
    fn a_failing_value_does_not_make_its_key_unknown() {
        let found = run_with(&task_schema(), "[tasks.build]\nrun = 1\n");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, "1");
    }

    #[test]
    fn a_value_of_no_accepted_type_lists_the_types() {
        let found = run_with(&task_schema(), "[tasks]\nbuild = 1\n");
        assert_eq!(
            found,
            [(
                "1".to_string(),
                "expected object or string, found integer".to_string()
            )]
        );
    }
}
