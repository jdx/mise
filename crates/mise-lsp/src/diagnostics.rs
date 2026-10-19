//! Problems in a config: TOML syntax errors, keys and values the schema rejects, and
//! deprecated keys.

use std::collections::HashMap;
use std::ops::Range;

use jsonschema::{Validator, error::ValidationErrorKind};

use crate::doc::{self, Entry, Parsed};
use crate::keypath;
use crate::schema::Schema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Error,
    Warning,
}

#[derive(Debug)]
pub(crate) struct Diagnostic {
    pub range: Range<usize>,
    pub severity: Severity,
    pub message: String,
    pub deprecated: bool,
}

pub(crate) fn diagnose(
    text: &str,
    schema: &Schema,
    validator: Option<&Validator>,
) -> Vec<Diagnostic> {
    let parsed = match doc::parse(text) {
        Ok(parsed) => parsed,
        Err(e) => {
            return vec![Diagnostic {
                range: e.span,
                severity: Severity::Error,
                message: e.message,
                deprecated: false,
            }];
        }
    };
    let by_path: HashMap<&[String], &Entry> = parsed
        .entries
        .iter()
        .map(|e| (e.path.as_slice(), e))
        .collect();
    let mut out = vec![];
    if let Some(validator) = validator {
        schema_errors(&parsed, &by_path, schema, validator, &mut out);
    }
    deprecated_keys(&parsed, schema, &mut out);
    out
}

fn schema_errors(
    parsed: &Parsed,
    by_path: &HashMap<&[String], &Entry>,
    schema: &Schema,
    validator: &Validator,
    out: &mut Vec<Diagnostic>,
) {
    for error in validator.iter_errors(&parsed.json) {
        let path = pointer_segments(error.instance_path().as_str());
        let unexpected = match error.kind() {
            ValidationErrorKind::AdditionalProperties { unexpected }
            | ValidationErrorKind::UnevaluatedProperties { unexpected } => Some(unexpected),
            _ => None,
        };
        if let Some(unexpected) = unexpected {
            for name in unexpected {
                let mut child = path.clone();
                child.push(name.clone());
                let range = by_path
                    .get(child.as_slice())
                    .and_then(|e| e.key.clone())
                    .or_else(|| nearest(by_path, &path));
                let Some(range) = range else { continue };
                let known = schema.keys(&path);
                let mut message = format!("unknown key `{name}`");
                if let Some(suggestion) = keypath::closest(name, known.iter().map(|(key, _)| *key))
                {
                    message.push_str(&format!("; did you mean `{suggestion}`?"));
                }
                push(out, range, Severity::Error, message, false);
            }
            continue;
        }
        let range = nearest(by_path, &path).unwrap_or(0..0);
        push(out, range, Severity::Error, error.to_string(), false);
    }
}

fn deprecated_keys(parsed: &Parsed, schema: &Schema, out: &mut Vec<Diagnostic>) {
    for entry in &parsed.entries {
        let Some(range) = entry.key.clone() else {
            continue;
        };
        if schema.is_deprecated(&schema.at(&entry.path)) {
            let name = entry.path.join(".");
            push(
                out,
                range,
                Severity::Warning,
                format!("`{name}` is deprecated"),
                true,
            );
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
    severity: Severity,
    message: String,
    deprecated: bool,
) {
    if !out.iter().any(|d| d.range == range && d.message == message) {
        out.push(Diagnostic {
            range,
            severity,
            message,
            deprecated,
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

    fn run(text: &str) -> Vec<(String, String)> {
        let schema = sample();
        let validator = jsonschema::draft202012::new(schema.root()).unwrap();
        diagnose(text, &schema, Some(&validator))
            .into_iter()
            .map(|d| (text[d.range].to_string(), d.message))
            .collect()
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
}
