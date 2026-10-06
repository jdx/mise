//! Set some keys of a JSON, TOML, or YAML file and leave the rest alone.
//!
//! This is for files an application also writes: the caller owns the keys in
//! `source`, the application owns everything else. [`merge`] returns the
//! target's new text, editing it in place where the format allows so
//! comments, key order, and formatting on other keys survive. [`contains`]
//! reports whether a target already satisfies the source, which is how a
//! caller tells "in sync" from "drifted" without being bothered by keys it
//! does not own.
//!
//! Semantics, identical for all three formats:
//! - A key present on both sides whose values are both tables/objects merges
//!   recursively.
//! - Any other value, including an array, is replaced by the source's.
//! - Keys only the target has are never touched, and nothing is ever removed.
//! - An empty target is an empty table.

use crate::yaml_merge::merge_yaml;
use eyre::{Result, bail, eyre};
use serde_json::{Map, Value};
use std::path::Path;
use toml_edit::{DocumentMut, Item, TableLike};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Format {
    Json,
    Toml,
    Yaml,
}

impl Format {
    /// Infer the format from a file extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
        }
    }
}

/// Whether every key in `source` is already set to the same value in
/// `target`.
pub fn contains(format: Format, target: &str, source: &str) -> Result<bool> {
    let target = parse(format, target, "target")?;
    let source = parse(format, source, "source")?;
    Ok(is_subset(&source, &target))
}

/// Set the keys of `source` on `target` and return the new text. A target that
/// already satisfies `source` is returned byte for byte.
pub fn merge(format: Format, target: &str, source: &str) -> Result<String> {
    if contains(format, target, source)? {
        return Ok(target.to_string());
    }
    match format {
        Format::Json => merge_json(target, source),
        Format::Toml => merge_toml(target, source),
        Format::Yaml => merge_yaml(target, source),
    }
}

fn parse(format: Format, text: &str, what: &str) -> Result<Value> {
    let blank = text.trim().is_empty() || (format != Format::Json && is_comments_only(text));
    let value = if blank {
        Value::Object(Map::new())
    } else {
        match format {
            Format::Json => serde_json::from_str(text).map_err(|e| e.to_string()),
            Format::Toml => toml::from_str(text).map_err(|e| e.to_string()),
            Format::Yaml => serde_yaml::from_str(text).map_err(|e| e.to_string()),
        }
        .map_err(|e| eyre!("failed to parse the {} {what}: {e}", format.name()))?
    };
    if !value.is_object() {
        bail!(
            "the {} {what} must hold a table at the top level",
            format.name()
        );
    }
    Ok(value)
}

fn is_comments_only(text: &str) -> bool {
    text.lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
}

fn is_subset(source: &Value, target: &Value) -> bool {
    match (source, target) {
        (Value::Object(s), Value::Object(t)) => s
            .iter()
            .all(|(k, sv)| t.get(k).is_some_and(|tv| is_subset(sv, tv))),
        _ => source == target,
    }
}

fn merge_json(target: &str, source: &str) -> Result<String> {
    let mut merged = parse(Format::Json, target, "target")?;
    merge_value(&mut merged, parse(Format::Json, source, "source")?);
    let indent = json_indent(target);
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    serde::Serialize::serialize(
        &merged,
        &mut serde_json::Serializer::with_formatter(&mut out, formatter),
    )?;
    let mut out = String::from_utf8(out)?;
    // Hand-edited JSON nearly always ends in a newline; keep that.
    if target.is_empty() || target.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn merge_value(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(t), Value::Object(s)) => {
            for (k, sv) in s {
                match t.get_mut(&k) {
                    Some(tv) => merge_value(tv, sv),
                    None => {
                        t.insert(k, sv);
                    }
                }
            }
        }
        (t, s) => *t = s,
    }
}

/// The indent of the target's first indented line, two spaces if it has none.
fn json_indent(text: &str) -> String {
    text.lines()
        .find_map(|l| {
            let indent: String = l.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            (!indent.is_empty() && indent.len() < l.len()).then_some(indent)
        })
        .unwrap_or_else(|| "  ".to_string())
}

fn merge_toml(target: &str, source: &str) -> Result<String> {
    let mut doc = target
        .parse::<DocumentMut>()
        .map_err(|e| eyre!("failed to parse the TOML target: {e}"))?;
    let source = source
        .parse::<DocumentMut>()
        .map_err(|e| eyre!("failed to parse the TOML source: {e}"))?;
    merge_toml_tables(doc.as_table_mut(), source.as_table());
    Ok(doc.to_string())
}

fn merge_toml_tables(target: &mut dyn TableLike, source: &dyn TableLike) {
    for (key, source_item) in source.iter() {
        let Some(target_item) = target.get_mut(key) else {
            target.insert(key, source_item.clone());
            continue;
        };
        if let (Some(t), Some(s)) = (target_item.as_table_like_mut(), source_item.as_table_like()) {
            merge_toml_tables(t, s);
        } else if !same_toml_item(target_item, source_item) {
            let mut replacement = source_item.clone();
            // Keep the spacing and trailing comment of the line being replaced.
            if let (Some(old), Some(new)) = (target_item.as_value(), replacement.as_value_mut()) {
                *new.decor_mut() = old.decor().clone();
            }
            *target_item = replacement;
        }
    }
}

/// Equality that ignores the whitespace around a value, so a matching value
/// keeps its own formatting.
fn same_toml_item(a: &Item, b: &Item) -> bool {
    match (a.as_value(), b.as_value()) {
        (Some(a), Some(b)) => {
            let (mut a, mut b) = (a.clone(), b.clone());
            a.decor_mut().clear();
            b.decor_mut().clear();
            a.to_string() == b.to_string()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn formats_come_from_extensions() {
        for (path, format) in [
            ("a.json", Some(Format::Json)),
            ("a.TOML", Some(Format::Toml)),
            ("a.yaml", Some(Format::Yaml)),
            ("a.yml", Some(Format::Yaml)),
            ("a.jsonc", None),
            ("a", None),
        ] {
            assert_eq!(Format::from_path(Path::new(path)), format, "{path}");
        }
    }

    #[test]
    fn json_keeps_unowned_keys_in_order_and_the_indent() {
        let target = "{\n    \"z\": 1,\n    \"model\": \"old\",\n    \"a\": {\"x\": 1}\n}\n";
        let source = r#"{"model": "new", "a": {"y": 2}, "n": [1, 2]}"#;
        assert_eq!(
            merge(Format::Json, target, source).unwrap(),
            "{\n    \"z\": 1,\n    \"model\": \"new\",\n    \"a\": {\n        \"x\": 1,\n        \"y\": 2\n    },\n    \"n\": [\n        1,\n        2\n    ]\n}\n"
        );
    }

    #[test]
    fn json_arrays_are_replaced() {
        let merged = merge(Format::Json, r#"{"l": [1, 2, 3]}"#, r#"{"l": [9]}"#).unwrap();
        assert_eq!(merged, "{\n  \"l\": [\n    9\n  ]\n}");
    }

    #[test]
    fn toml_edits_in_place() {
        let target = "\
# written by the desktop app
notify = [\"a\"]

[desktop]
theme = \"dark\"  # app choice

[plugins.foo]
path = \"/Applications/X.app\"
";
        let source = "model = \"gpt\"\n\n[desktop]\ntheme = \"light\"\nsize = 3\n";
        assert_eq!(
            merge(Format::Toml, target, source).unwrap(),
            "\
# written by the desktop app
notify = [\"a\"]
model = \"gpt\"

[desktop]
theme = \"light\"  # app choice
size = 3

[plugins.foo]
path = \"/Applications/X.app\"
"
        );
    }

    #[test]
    fn toml_merges_inline_tables_and_replaces_arrays() {
        let merged = merge(
            Format::Toml,
            "t = { a = 1, b = 2 }\nl = [1, 2]\n",
            "t = { b = 3 }\nl = [9]\n",
        )
        .unwrap();
        assert_eq!(merged, "t = { a = 1, b = 3 }\nl = [9]\n");
    }

    #[test]
    fn toml_creates_a_missing_file() {
        assert_eq!(
            merge(Format::Toml, "", "[a]\nb = 1\n").unwrap(),
            "[a]\nb = 1\n"
        );
    }

    #[test]
    fn yaml_goes_through_the_yaml_merge_helper() {
        assert_eq!(
            merge(Format::Yaml, "# c\na: 1\nb: 2\n", "a: 5\n").unwrap(),
            "# c\na: 5\nb: 2\n"
        );
    }

    #[test]
    fn a_satisfied_target_comes_back_untouched() {
        for (format, target, source) in [
            (Format::Json, "{\"a\":1,   \"b\":2}", r#"{"a": 1}"#),
            (Format::Toml, "a   = 1 # x\nb = 2\n", "a = 1\n"),
            (Format::Yaml, "a:   1   # x\nb: 2\n", "a: 1\n"),
        ] {
            assert_eq!(merge(format, target, source).unwrap(), target);
            assert!(contains(format, target, source).unwrap());
        }
    }

    #[test]
    fn contains_only_looks_at_owned_keys() {
        let source = "a = 1\n[t]\nx = 2\n";
        assert!(
            contains(
                Format::Toml,
                "extra = 9\na = 1\n[t]\nx = 2\ny = 3\n",
                source
            )
            .unwrap()
        );
        assert!(!contains(Format::Toml, "a = 1\n[t]\nx = 3\n", source).unwrap());
        assert!(!contains(Format::Toml, "a = 1\n", source).unwrap());
        // arrays are compared whole
        assert!(!contains(Format::Json, r#"{"l":[1,2]}"#, r#"{"l":[1]}"#).unwrap());
    }

    #[test]
    fn merging_twice_changes_nothing() {
        for (format, target, source) in [
            (Format::Json, "{\"x\": 1}\n", "{\"a\": {\"b\": [1]}}"),
            (Format::Toml, "x = 1\n", "[a]\nb = [1]\n"),
            (Format::Yaml, "x: 1\n", "a:\n  b: [1]\n"),
        ] {
            let once = merge(format, target, source).unwrap();
            assert_eq!(merge(format, &once, source).unwrap(), once);
        }
    }

    #[test]
    fn unparseable_or_non_table_input_is_an_error() {
        assert!(merge(Format::Json, "{// c\n}", "{}").is_err());
        assert!(merge(Format::Json, "[1]", "{}").is_err());
        assert!(merge(Format::Toml, "a = ", "a = 1").is_err());
        assert!(merge(Format::Yaml, "- a\n", "a: 1\n").is_err());
        assert!(merge(Format::Json, "{}", "[1]").is_err());
    }
}
