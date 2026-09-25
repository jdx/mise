//! Generate the settings types from mise's `settings.toml`.

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    codegen_settings();
}

/// `settings.toml` lives at the workspace root, where docs and schema tooling
/// read it too. The release copies it into this crate so the published package
/// can build on its own.
///
/// The workspace file wins whenever it exists, so a copy left behind by a local
/// release run can never shadow edits to it. Both paths are watched for the same
/// reason.
fn settings_toml_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir.join("../../settings.toml");
    let packaged = manifest_dir.join("settings.toml");
    println!("cargo:rerun-if-changed={}", workspace.display());
    println!("cargo:rerun-if-changed={}", packaged.display());
    if workspace.exists() {
        return workspace;
    }
    assert!(
        packaged.exists(),
        "settings.toml not found. Outside the mise workspace, copy its settings.toml into {} \
         before packaging (xtasks/release-plz does this when publishing).",
        manifest_dir.display()
    );
    packaged
}

/// Generate a raw string literal that safely contains the given content.
/// Dynamically determines the minimum number of '#' needed.
fn raw_string_literal(s: &str) -> String {
    // Find the longest sequence of '#' characters following a '"' in the string
    let mut max_hashes = 0;
    let mut current_hashes = 0;
    let mut after_quote = false;

    for c in s.chars() {
        if after_quote {
            if c == '#' {
                current_hashes += 1;
                max_hashes = max_hashes.max(current_hashes);
            } else {
                after_quote = false;
                current_hashes = 0;
            }
        }
        if c == '"' {
            after_quote = true;
            current_hashes = 0;
        }
    }

    // Use one more '#' than the longest sequence found
    let hashes = "#".repeat(max_hashes + 1);
    format!("r{hashes}\"{s}\"{hashes}")
}

/// Generate Rust setting types, metadata, and file-layer merge behavior from settings.toml.
fn codegen_settings() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("settings.rs");
    let mut lines = vec![
        r#"#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(layer_attr(derive(Clone, Serialize, Default)))]
pub struct Settings {"#
            .to_string(),
    ];

    let settings_path = settings_toml_path();
    let settings_toml = fs::read_to_string(&settings_path).expect("Failed to read settings.toml");
    let settings: toml::Table =
        toml::de::from_str(&settings_toml).expect("Failed to parse settings.toml");
    /// Build a collision-resistant generated Rust type name for a settings path.
    fn settings_struct_name(path: &[&str]) -> String {
        if let [part] = path {
            return format!("Settings{}", part.to_upper_camel_case());
        }

        let mut name = "SettingsNested".to_string();
        for part in path {
            // Encode both component boundaries and the original bytes so distinct
            // TOML paths cannot collapse to the same generated Rust type name.
            name.push('P');
            name.push_str(&part.len().to_string());
            name.push('X');
            for byte in part.as_bytes() {
                name.push_str(&format!("{byte:02X}"));
            }
        }
        name
    }

    /// Render one settings.toml entry as a field in a generated settings struct.
    fn props_to_code(key: &str, props: &toml::Value, parent_path: &[&str]) -> String {
        let mut lines = vec![];
        let props = props.as_table().unwrap();
        if let Some(description) = props.get("description") {
            lines.push(format!("    /// {}", description.as_str().unwrap()));
        }
        let type_ = props
            .get("rust_type")
            .map(|rt| rt.as_str().unwrap())
            .or_else(|| {
                props.get("type").map(|t| match t.as_str().unwrap() {
                    "Bool" => "bool",
                    "String" => "String",
                    "Integer" => "i64",
                    "Url" => "String",
                    "Path" => "PathBuf",
                    "Duration" => "String",
                    "ListString" => "Vec<String>",
                    "ListPath" => "Vec<PathBuf>",
                    "SetString" => "BTreeSet<String>",
                    "IndexMap<String, String>" => "IndexMap<String, String>",
                    "BoolOrString" => {
                        panic!(r#"type \"BoolOrString\" requires a `rust_type` to be specified"#)
                    }
                    t => panic!("Unknown type: {t}"),
                })
            });
        if let Some(type_) = type_ {
            let type_ = if props.get("optional").is_some_and(|v| v.as_bool().unwrap()) {
                format!("Option<{type_}>")
            } else {
                type_.to_string()
            };
            let mut opts = IndexMap::new();
            if let Some(env) = props.get("env") {
                opts.insert("env".to_string(), env.to_string());
            }
            if let Some(default) = props.get("default") {
                opts.insert("default".to_string(), default.to_string());
            } else if type_ == "bool" {
                opts.insert("default".to_string(), "false".to_string());
            }
            if let Some(parse_env) = props.get("parse_env") {
                opts.insert(
                    "parse_env".to_string(),
                    parse_env.as_str().unwrap().to_string(),
                );
            }
            if let Some(deserialize_with) = props.get("deserialize_with") {
                opts.insert(
                    "deserialize_with".to_string(),
                    deserialize_with.as_str().unwrap().to_string(),
                );
            }
            lines.push(format!(
                "    #[config({})]",
                opts.iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            lines.push(format!("    pub {key}: {type_},"));
        } else {
            lines.push("    #[config(nested)]".to_string());
            let mut path = parent_path.to_vec();
            path.push(key);
            lines.push(format!("    pub {key}: {},", settings_struct_name(&path)));
        }
        lines.join("\n")
    }
    for (key, props) in &settings {
        lines.push(props_to_code(key, props, &[]));
    }
    lines.push("}".to_string());

    /// Emit generated settings structs for every nested settings.toml table.
    fn emit_nested_settings(lines: &mut Vec<String>, table: &toml::Table, parent_path: &[&str]) {
        for (child, props) in table
            .iter()
            .filter(|(_, value)| !value.as_table().unwrap().contains_key("type"))
        {
            let mut path = parent_path.to_vec();
            path.push(child);
            lines.push(format!(
                r#"
#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(layer_attr(derive(Clone, Serialize, Default)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct {name} {{"#,
                name = settings_struct_name(&path)
            ));

            for (key, props) in props.as_table().unwrap() {
                lines.push(props_to_code(key, props, &path));
            }
            lines.push("}".to_string());
            emit_nested_settings(lines, props.as_table().unwrap(), &path);
        }
    }
    emit_nested_settings(&mut lines, &settings, &[]);

    lines.push(
        r#"
/// Validate collection values constrained by `enum` in settings.toml.
pub fn validate_settings_enum_values(settings: &Settings) -> Result<()> {"#
            .to_string(),
    );
    /// Emit runtime validators for constrained string collections.
    fn emit_collection_enum_validators(
        lines: &mut Vec<String>,
        table: &toml::Table,
        path: &[&str],
    ) {
        for (key, value) in table {
            let props = value.as_table().unwrap();
            let mut field_path = path.to_vec();
            field_path.push(key);
            let Some(type_) = props.get("type").and_then(toml::Value::as_str) else {
                emit_collection_enum_validators(lines, props, &field_path);
                continue;
            };
            if !matches!(type_, "ListString" | "SetString") {
                continue;
            }
            let Some(allowed) = props.get("enum").and_then(toml::Value::as_array) else {
                continue;
            };
            let allowed = allowed
                .iter()
                .map(|value| {
                    value.as_str().unwrap_or_else(|| {
                        panic!("enum values for {} must be strings", field_path.join("."))
                    })
                })
                .collect::<Vec<_>>();
            let allowed_code = allowed
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            let name = field_path.join(".");
            let field = format!("settings.{name}");
            let values = if props.get("optional").is_some_and(|v| v.as_bool().unwrap()) {
                format!("{field}.as_ref().into_iter().flatten().map(String::as_str)")
            } else {
                format!("{field}.iter().map(String::as_str)")
            };
            lines.push(format!(
                "    validate_setting_enum_values({name:?}, {values}, &[{allowed_code}])?;"
            ));
        }
    }
    emit_collection_enum_validators(&mut lines, &settings, &[]);
    lines.push("    Ok(())".to_string());
    lines.push("}".to_string());

    lines.push(
        r#"
/// Apply the merge strategies declared in settings.toml to config-file layers.
pub fn merge_settings_file_layers(layers: &mut [SettingsPartial]) {"#
            .to_string(),
    );
    for (key, props) in &settings {
        let props = props.as_table().unwrap();
        let Some(strategy) = props.get("merge") else {
            continue;
        };
        let strategy = strategy
            .as_str()
            .expect("setting merge strategy must be a string");
        match (strategy, props.get("type").and_then(toml::Value::as_str)) {
            ("append_unique", Some("ListString")) => lines.push(format!(
                r#"    {{
        let mut found = false;
        let values = layers
            .iter_mut()
            .rev()
            .filter_map(|layer| {{
                let values = layer.{key}.take();
                found |= values.is_some();
                values
            }})
            .flatten()
            .unique()
            .collect();
        if found {{
            layers[0].{key} = Some(values);
        }}
    }}"#
            )),
            _ => panic!(
                "unsupported merge strategy {strategy:?} for setting {key:?}; append_unique requires ListString"
            ),
        }
    }
    lines.push("}".to_string());

    lines.push(
        r#"
pub static SETTINGS_META: Lazy<IndexMap<&'static str, SettingsMeta>> = Lazy::new(|| {
    indexmap!{"#
            .to_string(),
    );
    /// Emit deprecation metadata shared by each generated settings metadata entry.
    fn push_deprecated_fields(lines: &mut Vec<String>, props: &toml::Table) {
        let deprecated = props
            .get("deprecated")
            .map(|v| v.as_str().unwrap().to_string());
        let warn_at = props
            .get("deprecated_warn_at")
            .map(|v| v.as_str().unwrap().to_string());
        let remove_at = props
            .get("deprecated_remove_at")
            .map(|v| v.as_str().unwrap().to_string());
        match deprecated {
            Some(msg) => lines.push(format!(
                "        deprecated: Some({}),",
                raw_string_literal(&msg)
            )),
            None => lines.push("        deprecated: None,".to_string()),
        }
        match warn_at {
            Some(v) => lines.push(format!("        deprecated_warn_at: Some({v:?}),")),
            None => lines.push("        deprecated_warn_at: None,".to_string()),
        }
        match remove_at {
            Some(v) => lines.push(format!("        deprecated_remove_at: Some({v:?}),")),
            None => lines.push("        deprecated_remove_at: None,".to_string()),
        }
        lines.push(format!(
            "        global_only: {},",
            props
                .get("global_only")
                .is_some_and(|v| v.as_bool().unwrap())
        ));
        lines.push(format!(
            "        env_only: {},",
            props.get("env_only").is_some_and(|v| v.as_bool().unwrap())
        ));
    }
    /// Emit flattened runtime metadata for settings and nested settings tables.
    fn emit_settings_meta(lines: &mut Vec<String>, table: &toml::Table, prefix: &str) {
        for (key, value) in table {
            let name = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            let props = value.as_table().unwrap();
            if let Some(type_) = props.get("type").map(|value| value.as_str().unwrap()) {
                // We could shadow the 'type_' variable, but its a best practice to avoid shadowing.
                // Thus, we introduce 'meta_type' here.
                let meta_type = match type_ {
                    "IndexMap<String, String>" => "IndexMap",
                    other => other,
                };
                lines.push(format!(
                    r#"    "{name}" => SettingsMeta {{
        type_: SettingsType::{meta_type},"#,
                ));
                if let Some(description) = props.get("description") {
                    let description = description.as_str().unwrap().to_string();
                    lines.push(format!(
                        "        description: {},",
                        raw_string_literal(&description)
                    ));
                }
                match props.get("env").and_then(|value| value.as_str()) {
                    Some(env) => lines.push(format!("        env: Some({env:?}),")),
                    None => lines.push("        env: None,".to_string()),
                }
                push_deprecated_fields(lines, props);
                lines.push("    },".to_string());
            } else {
                emit_settings_meta(lines, props, &name);
            }
        }
    }
    emit_settings_meta(&mut lines, &settings, "");
    lines.push(
        r#"    }
});
    "#
        .to_string(),
    );

    // Generate MisercSettings struct for early initialization settings
    lines.push(
        r#"
/// Settings that can be set in .miserc.toml for early initialization.
/// These settings affect config file discovery and must be loaded before
/// the main config files are parsed.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct MisercSettings {"#
            .to_string(),
    );

    for (key, props) in &settings {
        let props = props.as_table().unwrap();
        // Only include settings with rc = true
        if props
            .get("rc")
            .is_some_and(|v| v.as_bool().unwrap_or(false))
        {
            if let Some(description) = props.get("description") {
                lines.push(format!("    /// {}", description.as_str().unwrap()));
            }
            let type_ = props
                .get("rust_type")
                .map(|rt| rt.as_str().unwrap())
                .or_else(|| {
                    props.get("type").map(|t| match t.as_str().unwrap() {
                        "Bool" => "bool",
                        "String" => "String",
                        "Integer" => "i64",
                        "Url" => "String",
                        "Path" => "PathBuf",
                        "Duration" => "String",
                        "ListString" => "Vec<String>",
                        "ListPath" => "Vec<PathBuf>",
                        "SetString" => "BTreeSet<String>",
                        "IndexMap<String, String>" => "IndexMap<String, String>",
                        "BoolOrString" => panic!(
                            r#"type \"BoolOrString\" requires a `rust_type` to be specified"#
                        ),
                        t => panic!("Unknown type: {t}"),
                    })
                });
            if let Some(type_) = type_ {
                // All miserc settings are optional
                let type_ = format!("Option<{type_}>");
                lines.push(format!("    pub {key}: {type_},"));
            }
        }
    }
    lines.push("}".to_string());

    fs::write(&dest_path, lines.join("\n")).unwrap();
}
