//! The `[settings]` schema and `schema/miserc.json`, built from mise's `settings.toml`.
//!
//! Each leaf table of `settings.toml` (one with a `type`) describes a setting; every other table
//! is a group of settings. mise-settings generates the Rust types from the same file, so a setting
//! and its schema cannot drift apart.

use serde_json::{Map, Value, json};

/// The `properties` of `$defs.settings`, one per top-level setting or group.
pub(crate) fn properties(settings: &toml::Table) -> Map<String, Value> {
    group(settings, "")
}

/// `schema/miserc.json`: the top-level settings marked `rc`, which mise reads from
/// `.miserc.toml` before it loads any config file.
pub(crate) fn miserc(settings: &toml::Table) -> Value {
    let properties: Map<String, Value> = settings
        .iter()
        .filter_map(|(key, node)| {
            let props = leaf(node)?;
            (props.get("rc").and_then(toml::Value::as_bool) == Some(true))
                .then(|| (key.clone(), element(key, props)))
        })
        .collect();
    json!({
        "$schema": super::DRAFT,
        "title": "mise rc config",
        "description": "Early initialization settings for mise. These settings are loaded before the main config files.",
        "type": "object",
        "unevaluatedProperties": false,
        "properties": properties
    })
}

fn leaf(node: &toml::Value) -> Option<&toml::Table> {
    node.as_table().filter(|table| table.contains_key("type"))
}

fn group(table: &toml::Table, prefix: &str) -> Map<String, Value> {
    table
        .iter()
        .map(|(key, node)| {
            let name = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            let schema = match leaf(node) {
                Some(props) => element(&name, props),
                None => {
                    let table = node
                        .as_table()
                        .unwrap_or_else(|| panic!("settings.toml: {name} is not a table"));
                    json!({
                        "type": "object",
                        "unevaluatedProperties": false,
                        "properties": group(table, &name)
                    })
                }
            };
            (key.clone(), schema)
        })
        .collect()
}

/// One setting's schema. `name` is its dotted path, for error messages.
fn element(name: &str, props: &toml::Table) -> Value {
    let str_prop = |key: &str| props.get(key).and_then(toml::Value::as_str);
    let setting_type = str_prop("type").unwrap_or_default();
    let json_type = match setting_type {
        "String" | "Path" | "Url" | "Duration" => "string",
        "Bool" => "boolean",
        "Integer" => "integer",
        "ListString" | "ListPath" | "SetString" => "array",
        "IndexMap<String, String>" => "object",
        // Written as `oneOf` below, which AJV's strictTypes accepts where a list of types fails.
        "BoolOrString" | "BoolOrList" => "",
        other => panic!("settings.toml: unknown type {other:?} for {name}"),
    };
    let description = str_prop("description")
        .unwrap_or_else(|| panic!("settings.toml: missing description for {name}"));
    let choices: Option<Value> = props
        .get("enum")
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                // An entry is a bare value or `{ value, description }`.
                .map(|item| to_json(item.as_table().and_then(|t| t.get("value")).unwrap_or(item)))
                .collect()
        });
    let strings = || {
        let mut items = json!({"type": "string"});
        if let Some(choices) = &choices {
            items["enum"] = choices.clone();
        }
        items
    };

    let mut element = Map::new();
    if let Some(default) = props.get("default") {
        element.insert("default".into(), to_json(default));
    }
    element.insert("description".into(), description.into());
    if !json_type.is_empty() {
        element.insert("type".into(), json_type.into());
    }
    if props.contains_key("deprecated") {
        element.insert("deprecated".into(), true.into());
    }
    if json_type == "integer" && str_prop("rust_type").is_some_and(|t| t.starts_with('u')) {
        element.insert("minimum".into(), 0.into());
    }
    if json_type == "array" {
        element.insert("items".into(), strings());
    } else if setting_type != "BoolOrList"
        && let Some(choices) = &choices
    {
        element.insert("enum".into(), choices.clone());
    }
    if json_type == "object" {
        element.insert("additionalProperties".into(), json!({"type": "string"}));
    }
    match setting_type {
        // A list setting that also takes a single value.
        _ if json_type == "array" && str_prop("deserialize_with") == Some("path_or_paths") => {
            element.remove("type");
            let items = element.remove("items");
            element.insert(
                "oneOf".into(),
                json!([{"type": "string"}, {"type": "array", "items": items}]),
            );
        }
        "BoolOrString" => {
            element.insert(
                "oneOf".into(),
                json!([{"type": "boolean"}, {"type": "string"}]),
            );
        }
        // A boolean, or a list of the strings the setting's `enum` allows.
        "BoolOrList" => {
            element.insert(
                "oneOf".into(),
                json!([{"type": "boolean"}, {"type": "array", "items": strings()}]),
            );
        }
        _ => {}
    }
    element.into()
}

fn to_json(value: &toml::Value) -> Value {
    serde_json::to_value(value).expect("a settings.toml value converts to JSON")
}
