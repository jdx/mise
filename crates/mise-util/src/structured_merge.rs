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

use crate::yaml_merge::{mapping_subset, merge_yaml};
use eyre::{Result, eyre};
use indexmap::IndexMap;
use serde_json::value::RawValue;
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

/// A parsed document in its format's own value type. JSON's cannot hold TOML
/// datetimes or YAML's non-string keys, so each format is compared as itself.
enum Doc {
    Json(Value),
    Toml(toml::Table),
    Yaml(serde_yaml::Mapping),
}

/// Whether every key in `source` is already set to the same value in
/// `target`.
pub fn contains(format: Format, target: &str, source: &str) -> Result<bool> {
    Ok(
        match (
            parse(format, target, "target")?,
            parse(format, source, "source")?,
        ) {
            (Doc::Json(target), Doc::Json(source)) => json_subset(&source, &target),
            (Doc::Toml(target), Doc::Toml(source)) => toml_subset(&source, &target),
            (Doc::Yaml(target), Doc::Yaml(source)) => mapping_subset(&source, &target),
            _ => unreachable!("both sides are parsed as the same format"),
        },
    )
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

/// The part of `source` that `target` has no value for, as text in the same
/// format, or `None` when the target has a value for every key in `source`. A
/// key the target holds counts as present whatever its value, and a table both
/// sides hold is compared key by key.
pub fn missing(format: Format, target: &str, source: &str) -> Result<Option<String>> {
    Ok(
        match (
            parse(format, target, "target")?,
            parse(format, source, "source")?,
        ) {
            (Doc::Json(Value::Object(target)), Doc::Json(Value::Object(source))) => {
                let missing = json_missing(&source, &target);
                (!missing.is_empty())
                    .then(|| serde_json::to_string(&Value::Object(missing)))
                    .transpose()?
            }
            (Doc::Toml(target), Doc::Toml(source)) => {
                let missing = toml_missing(&source, &target);
                (!missing.is_empty())
                    .then(|| toml::to_string(&missing))
                    .transpose()?
            }
            (Doc::Yaml(target), Doc::Yaml(source)) => {
                let missing = yaml_missing(&source, &target);
                (!missing.is_empty())
                    .then(|| serde_yaml::to_string(&missing))
                    .transpose()?
            }
            _ => unreachable!("both sides are parsed as the same format"),
        },
    )
}

/// Set only the keys of `source` that `target` has no value for and return the
/// new text. Existing values are never changed, so a value the application
/// rewrote stays, and a key it removed comes back.
pub fn fill_missing(format: Format, target: &str, source: &str) -> Result<String> {
    match missing(format, target, source)? {
        Some(missing) => merge(format, target, &missing),
        None => Ok(target.to_string()),
    }
}

fn json_missing(source: &Map<String, Value>, target: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, sv) in source {
        match (sv, target.get(key)) {
            (_, None) => {
                out.insert(key.clone(), sv.clone());
            }
            (Value::Object(s), Some(Value::Object(t))) => {
                let inner = json_missing(s, t);
                if !inner.is_empty() {
                    out.insert(key.clone(), Value::Object(inner));
                }
            }
            _ => {}
        }
    }
    out
}

fn toml_missing(source: &toml::Table, target: &toml::Table) -> toml::Table {
    let mut out = toml::Table::new();
    for (key, sv) in source {
        match (sv, target.get(key)) {
            (_, None) => {
                out.insert(key.clone(), sv.clone());
            }
            (toml::Value::Table(s), Some(toml::Value::Table(t))) => {
                let inner = toml_missing(s, t);
                if !inner.is_empty() {
                    out.insert(key.clone(), toml::Value::Table(inner));
                }
            }
            _ => {}
        }
    }
    out
}

fn yaml_missing(source: &serde_yaml::Mapping, target: &serde_yaml::Mapping) -> serde_yaml::Mapping {
    let mut out = serde_yaml::Mapping::new();
    for (key, sv) in source {
        match (sv, target.get(key)) {
            (_, None) => {
                out.insert(key.clone(), sv.clone());
            }
            (serde_yaml::Value::Mapping(s), Some(serde_yaml::Value::Mapping(t))) => {
                let inner = yaml_missing(s, t);
                if !inner.is_empty() {
                    out.insert(key.clone(), serde_yaml::Value::Mapping(inner));
                }
            }
            _ => {}
        }
    }
    out
}

fn parse(format: Format, text: &str, what: &str) -> Result<Doc> {
    // an empty file, or one holding only comments, is an empty table
    let blank = text.trim().is_empty() || (format != Format::Json && is_comments_only(text));
    let fail =
        |e: &dyn std::fmt::Display| eyre!("failed to parse the {} {what}: {e}", format.name());
    let not_a_table = || {
        eyre!(
            "the {} {what} must hold a table at the top level",
            format.name()
        )
    };
    Ok(match format {
        Format::Json if blank => Doc::Json(Value::Object(Map::new())),
        Format::Json => match serde_json::from_str(text).map_err(|e| fail(&e))? {
            value @ Value::Object(_) => Doc::Json(value),
            _ => return Err(not_a_table()),
        },
        Format::Toml if blank => Doc::Toml(toml::Table::new()),
        Format::Toml => Doc::Toml(toml::from_str(text).map_err(|e| fail(&e))?),
        Format::Yaml if blank => Doc::Yaml(serde_yaml::Mapping::new()),
        Format::Yaml => match serde_yaml::from_str(text).map_err(|e| fail(&e))? {
            serde_yaml::Value::Mapping(map) => Doc::Yaml(map),
            _ => return Err(not_a_table()),
        },
    })
}

/// Dotted paths that `a` and `b` both set, to different values. Two sources
/// that agree on a key, or that set different keys of the same table, do not
/// conflict.
pub fn conflicts(format: Format, a: &str, b: &str) -> Result<Vec<String>> {
    let mut paths = vec![];
    match (parse(format, a, "source")?, parse(format, b, "source")?) {
        (Doc::Json(a), Doc::Json(b)) => json_conflicts(&a, &b, "", &mut paths),
        (Doc::Toml(a), Doc::Toml(b)) => toml_conflicts(&a, &b, "", &mut paths),
        (Doc::Yaml(a), Doc::Yaml(b)) => yaml_conflicts(&a, &b, "", &mut paths),
        _ => unreachable!("both sides are parsed as the same format"),
    }
    Ok(paths)
}

fn child_path(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

fn json_conflicts(a: &Value, b: &Value, prefix: &str, paths: &mut Vec<String>) {
    let (Value::Object(a), Value::Object(b)) = (a, b) else {
        return;
    };
    for (key, av) in a {
        let Some(bv) = b.get(key) else { continue };
        let path = child_path(prefix, key);
        if av.is_object() && bv.is_object() {
            json_conflicts(av, bv, &path, paths);
        } else if av != bv {
            paths.push(path);
        }
    }
}

fn toml_conflicts(a: &toml::Table, b: &toml::Table, prefix: &str, paths: &mut Vec<String>) {
    for (key, av) in a {
        let Some(bv) = b.get(key) else { continue };
        let path = child_path(prefix, key);
        match (av, bv) {
            (toml::Value::Table(av), toml::Value::Table(bv)) => {
                toml_conflicts(av, bv, &path, paths);
            }
            _ if av != bv => paths.push(path),
            _ => {}
        }
    }
}

fn yaml_conflicts(
    a: &serde_yaml::Mapping,
    b: &serde_yaml::Mapping,
    prefix: &str,
    paths: &mut Vec<String>,
) {
    for (key, av) in a {
        let Some(bv) = b.get(key) else { continue };
        let name = match key {
            serde_yaml::Value::String(name) => name.clone(),
            other => serde_yaml::to_string(other)
                .unwrap_or_default()
                .trim()
                .to_string(),
        };
        let path = child_path(prefix, &name);
        match (av, bv) {
            (serde_yaml::Value::Mapping(av), serde_yaml::Value::Mapping(bv)) => {
                yaml_conflicts(av, bv, &path, paths);
            }
            _ if av != bv => paths.push(path),
            _ => {}
        }
    }
}

fn is_comments_only(text: &str) -> bool {
    text.lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
}

fn json_subset(source: &Value, target: &Value) -> bool {
    match (source, target) {
        (Value::Object(s), Value::Object(t)) => s
            .iter()
            .all(|(k, sv)| t.get(k).is_some_and(|tv| json_subset(sv, tv))),
        _ => source == target,
    }
}

fn toml_subset(source: &toml::Table, target: &toml::Table) -> bool {
    source.iter().all(|(key, sv)| match (sv, target.get(key)) {
        (toml::Value::Table(s), Some(toml::Value::Table(t))) => toml_subset(s, t),
        (sv, Some(tv)) => sv == tv,
        (_, None) => false,
    })
}

fn merge_json(target: &str, source: &str) -> Result<String> {
    let Doc::Json(Value::Object(source)) = parse(Format::Json, source, "source")? else {
        unreachable!("parsed as a JSON object");
    };
    // Values the source does not touch stay as the exact text they were parsed
    // from: a number past f64 or an `1e3` spelling is not renormalized.
    let existing: IndexMap<String, Box<RawValue>> = if target.trim().is_empty() {
        IndexMap::new()
    } else {
        serde_json::from_str(target).map_err(|e| eyre!("failed to parse the JSON target: {e}"))?
    };
    let merged = merge_json_objects(existing, source);
    let indent = json_indent(target);
    let mut out = String::new();
    print_json_object(&merged, 0, &indent, &mut out)?;
    // Hand-edited JSON nearly always ends in a newline; keep that.
    if target.is_empty() || target.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// A JSON value on its way back to text: either the exact source text it came
/// from, an object some of whose entries changed, or a new value.
enum JsonNode {
    Raw(Box<RawValue>),
    Object(IndexMap<String, JsonNode>),
    Value(Value),
}

fn merge_json_objects(
    target: IndexMap<String, Box<RawValue>>,
    source: Map<String, Value>,
) -> IndexMap<String, JsonNode> {
    let mut merged: IndexMap<String, JsonNode> = target
        .into_iter()
        .map(|(key, raw)| (key, JsonNode::Raw(raw)))
        .collect();
    for (key, value) in source {
        match (merged.get_mut(&key), value) {
            (Some(node), Value::Object(source_child)) => {
                let existing = match node {
                    JsonNode::Raw(raw) => serde_json::from_str(raw.get()).ok(),
                    _ => None,
                };
                *node = match existing {
                    Some(existing) => JsonNode::Object(merge_json_objects(existing, source_child)),
                    None => JsonNode::Value(Value::Object(source_child)),
                };
            }
            (Some(node), value) => *node = JsonNode::Value(value),
            (None, value) => {
                merged.insert(key, JsonNode::Value(value));
            }
        }
    }
    merged
}

fn print_json_object(
    map: &IndexMap<String, JsonNode>,
    depth: usize,
    indent: &str,
    out: &mut String,
) -> Result<()> {
    if map.is_empty() {
        out.push_str("{}");
        return Ok(());
    }
    out.push_str("{\n");
    for (i, (key, node)) in map.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str(&indent.repeat(depth + 1));
        out.push_str(&serde_json::to_string(key)?);
        out.push_str(": ");
        match node {
            JsonNode::Raw(raw) => out.push_str(raw.get()),
            JsonNode::Object(child) => print_json_object(child, depth + 1, indent, out)?,
            JsonNode::Value(value) => {
                let mut buf = Vec::new();
                let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
                serde::Serialize::serialize(
                    value,
                    &mut serde_json::Serializer::with_formatter(&mut buf, formatter),
                )?;
                // pretty-printed from column zero; move it to this depth
                let nested = String::from_utf8(buf)?
                    .replace('\n', &format!("\n{}", indent.repeat(depth + 1)));
                out.push_str(&nested);
            }
        }
    }
    out.push('\n');
    out.push_str(&indent.repeat(depth));
    out.push('}');
    Ok(())
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

/// Equality of two TOML items by what they hold, not how they are written, so
/// an array or array of tables that already matches keeps its own spacing and
/// comments.
fn same_toml_item(a: &Item, b: &Item) -> bool {
    match (toml_value(a), toml_value(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// An item as a plain value, by rendering it under a key and parsing that.
fn toml_value(item: &Item) -> Option<toml::Value> {
    let mut doc = DocumentMut::new();
    doc.insert("v", item.clone());
    toml::from_str::<toml::Table>(&doc.to_string())
        .ok()?
        .remove("v")
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
    fn fill_missing_keeps_existing_values_and_adds_absent_keys() {
        let target = "# app\nmodel = \"mine\"\n\n[tui]\ntheme = \"dark\"\n";
        let source =
            "model = \"default\"\neffort = \"high\"\n\n[tui]\ntheme = \"light\"\nsize = 3\n";
        assert_eq!(
            fill_missing(Format::Toml, target, source).unwrap(),
            "# app\nmodel = \"mine\"\neffort = \"high\"\n\n[tui]\ntheme = \"dark\"\nsize = 3\n"
        );
        let json = fill_missing(
            Format::Json,
            r#"{"model": "mine"}"#,
            r#"{"model": "d", "e": 1}"#,
        )
        .unwrap();
        assert_eq!(json, "{\n  \"model\": \"mine\",\n  \"e\": 1\n}");
        assert_eq!(
            fill_missing(Format::Yaml, "a: 1\n", "a: 2\nb: 3\n").unwrap(),
            "a: 1\nb: 3\n"
        );
    }

    #[test]
    fn nothing_is_missing_when_every_key_has_a_value() {
        // a different value, or a non-table where the source has a table, is still present
        for (format, target, source) in [
            (
                Format::Json,
                r#"{"a": {"b": 2}, "c": 1}"#,
                r#"{"a": {"b": 1}, "c": {"d": 1}}"#,
            ),
            (Format::Toml, "a = 1\n", "a = 2\n"),
            (Format::Yaml, "a: 1\n", "a: {b: 1}\n"),
        ] {
            assert_eq!(missing(format, target, source).unwrap(), None, "{format:?}");
            assert_eq!(fill_missing(format, target, source).unwrap(), target);
        }
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
    fn an_equal_toml_array_keeps_its_spacing_and_comments() {
        let target = "list = [\n  1,   # one\n  2,\n] # keep\nmode = \"old\"\n";
        let merged = merge(Format::Toml, target, "list = [1, 2]\nmode = \"new\"\n").unwrap();
        assert_eq!(merged, target.replace("\"old\"", "\"new\""));
    }

    #[test]
    fn an_equal_toml_array_of_tables_keeps_its_formatting() {
        let target = "mode = \"old\"\n\n[[bin]]  # first\nname = \"a\"\n\n[[bin]]\nname = \"b\"\n";
        let merged = merge(
            Format::Toml,
            target,
            "mode = \"new\"\n[[bin]]\nname = \"a\"\n[[bin]]\nname = \"b\"\n",
        )
        .unwrap();
        assert_eq!(merged, target.replace("\"old\"", "\"new\""));
    }

    #[test]
    fn untouched_json_values_keep_their_exact_text() {
        let target = "{\n  \"big\": 18446744073709551617,\n  \"sci\": 1e3,\n  \"esc\": \"\\u00e9\",\n  \"model\": \"old\"\n}\n";
        let merged = merge(Format::Json, target, r#"{"model": "new"}"#).unwrap();
        assert_eq!(merged, target.replace("\"old\"", "\"new\""));
    }

    #[test]
    fn values_json_cannot_hold_are_compared_as_themselves() {
        // TOML: a datetime is not the string that prints like it, and inf is fine
        assert!(
            !contains(
                Format::Toml,
                "when = \"2026-01-02T03:04:05Z\"\n",
                "when = 2026-01-02T03:04:05Z\n"
            )
            .unwrap()
        );
        let merged = merge(Format::Toml, "big = inf\nx = 1\n", "x = 2\n").unwrap();
        assert_eq!(merged, "big = inf\nx = 2\n");
        // YAML: non-string keys elsewhere in the file are not an error
        let target = "# keep\n1: one\nenv:\n  2: two\nname: old\n";
        assert_eq!(
            merge(Format::Yaml, target, "name: new\n").unwrap(),
            target.replace("old", "new")
        );
        assert!(!contains(Format::Yaml, target, "env:\n  2: three\n").unwrap());
    }

    #[test]
    fn sources_conflict_only_on_different_values_for_one_key() {
        let conflicts = |a, b| conflicts(Format::Toml, a, b).unwrap();
        assert_eq!(
            conflicts("m = 1\n[t]\nx = 1\ny = 2\n", "m = 2\n[t]\nx = 9\ny = 2\n"),
            vec!["m".to_string(), "t.x".to_string()]
        );
        assert!(conflicts("m = 1\n[t]\nx = 1\n", "m = 1\n[t]\ny = 2\n").is_empty());
        // a table against a scalar is a conflict too
        assert_eq!(conflicts("t = 1\n", "[t]\nx = 1\n"), vec!["t".to_string()]);
    }

    #[test]
    fn conflicts_are_found_in_every_format() {
        assert_eq!(
            conflicts(Format::Json, r#"{"a": {"b": 1}}"#, r#"{"a": {"b": 2}}"#).unwrap(),
            vec!["a.b".to_string()]
        );
        assert_eq!(
            conflicts(Format::Yaml, "a:\n  b: 1\n1: x\n", "a:\n  b: 2\n1: y\n").unwrap(),
            vec!["a.b".to_string(), "1".to_string()]
        );
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
