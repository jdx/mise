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
    /// `deprecated-key` or `invalid-value`.
    pub code: &'static str,
    /// The key an unknown one was probably meant to be.
    pub suggestion: Option<String>,
    /// The unknown key an `unknown-key` diagnostic is about, to tell a stale one from a current one.
    pub key: Option<String>,
    pub deprecated: bool,
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
    deprecated_keys(&parsed, schema, &mut out);
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

fn deprecated_keys(parsed: &Parsed, schema: &Schema, out: &mut Vec<Diagnostic>) {
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
            push(
                out,
                range,
                &entry.path,
                Severity::Warning,
                message,
                "deprecated-key",
                None,
            );
            out.last_mut().expect("just pushed").deprecated = true;
        }
    }
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
