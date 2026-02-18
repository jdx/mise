use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use std::path::Path;
use std::{env, fs};

fn main() {
    cfg_aliases::cfg_aliases! {
        asdf: { any(feature = "asdf", not(target_os = "windows")) },
        macos: { target_os = "macos" },
        linux: { target_os = "linux" },
        vfox: { any(feature = "vfox", target_os = "windows") },
    }
    built::write_built_file().expect("Failed to acquire build-time information");

    codegen_settings();
    codegen_registry();
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

/// Parse options from a TOML value into a Vec of (key, value) pairs
fn parse_options(opts: Option<&toml::Value>) -> Vec<(String, String)> {
    opts.map(|opts| {
        if let Some(table) = opts.as_table() {
            table
                .iter()
                .map(|(k, v)| {
                    let value = match v {
                        toml::Value::String(s) => s.clone(),
                        toml::Value::Table(t) => {
                            // Serialize nested tables back to TOML string
                            toml::to_string(t).unwrap_or_default()
                        }
                        _ => v.to_string(),
                    };
                    (k.clone(), value)
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        }
    })
    .unwrap_or_default()
}

fn load_registry_tools() -> toml::map::Map<String, toml::Value> {
    let mut tools = toml::map::Map::new();
    let registry_dir = Path::new("registry");

    println!("cargo:rerun-if-changed=registry");

    let mut files: Vec<_> = fs::read_dir(registry_dir)
        .expect("registry directory not found")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();

    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let tool_name = file
            .file_stem()
            .expect("file has no stem")
            .to_str()
            .expect("filename is not valid UTF-8")
            .to_string();
        let content = fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", file.display(), e));
        let tool_info: toml::Value = toml::de::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", file.display(), e));
        tools.insert(tool_name, tool_info);
    }
    tools
}

fn codegen_registry() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("registry.rs");
    let mut lines = vec![
        "{".to_string(),
        "    let mut m = std::collections::BTreeMap::new();".to_string(),
    ];

    let tools = load_registry_tools();
    for (short, info) in &tools {
        let info = info.as_table().unwrap();
        let aliases = info
            .get("aliases")
            .cloned()
            .unwrap_or(toml::Value::Array(vec![]))
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let test = info.get("test").map(|t| {
            let t = t
                .as_table()
                .unwrap_or_else(|| panic!("[{short}] 'test' field must be a table"));
            let cmd = t
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("[{short}] 'test.cmd' must be a string"))
                .to_string();
            let expected = t
                .get("expected")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("[{short}] 'test.expected' must be a string"))
                .to_string();
            (cmd, expected)
        });
        let mut backends = vec![];
        for backend in info.get("backends").unwrap().as_array().unwrap() {
            match backend {
                toml::Value::String(backend) => {
                    backends.push(format!(
                        r##"RegistryBackend{{
                            full: r#"{backend}"#,
                            platforms: &[],
                            options: &[],
                        }}"##
                    ));
                }
                toml::Value::Table(backend) => {
                    let full = backend.get("full").unwrap().as_str().unwrap();
                    let platforms = backend
                        .get("platforms")
                        .map(|p| {
                            p.as_array()
                                .unwrap()
                                .iter()
                                .map(|p| p.as_str().unwrap().to_string())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let backend_options = parse_options(backend.get("options"));
                    backends.push(format!(
                        r##"RegistryBackend{{
                            full: r#"{full}"#,
                            platforms: &[{platforms}],
                            options: &[{options}],
                        }}"##,
                        platforms = platforms
                            .into_iter()
                            .map(|p| format!("\"{p}\""))
                            .collect::<Vec<_>>()
                            .join(", "),
                        options = backend_options
                            .iter()
                            .map(|(k, v)| format!(
                                "({}, {})",
                                raw_string_literal(k),
                                raw_string_literal(v)
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                _ => panic!("Unknown backend type"),
            }
        }
        let os = info
            .get("os")
            .map(|os| {
                let os = os.as_array().unwrap();
                let mut os = os
                    .iter()
                    .map(|o| o.as_str().unwrap().to_string())
                    .collect::<Vec<_>>();
                os.sort();
                os
            })
            .unwrap_or_default();
        let description = info
            .get("description")
            .map(|d| d.as_str().unwrap().to_string());
        let depends = info
            .get("depends")
            .map(|depends| {
                let depends = depends.as_array().unwrap();
                let mut depends = depends
                    .iter()
                    .map(|d| d.as_str().unwrap().to_string())
                    .collect::<Vec<_>>();
                depends.sort();
                depends
            })
            .unwrap_or_default();
        let idiomatic_files = info
            .get("idiomatic_files")
            .map(|idiomatic_files| {
                idiomatic_files
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let detect = info
            .get("detect")
            .map(|detect| {
                detect
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let overrides = info
            .get("overrides")
            .map(|overrides| {
                overrides
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let rt = format!(
            r#"RegistryTool{{short: "{short}", description: {description}, backends: &[{backends}], aliases: &[{aliases}], test: &{test}, os: &[{os}], depends: &[{depends}], idiomatic_files: &[{idiomatic_files}], detect: &[{detect}], overrides: &[{overrides}]}}"#,
            description = description
                .map(|d| format!("Some({})", raw_string_literal(&d)))
                .unwrap_or("None".to_string()),
            backends = backends.into_iter().collect::<Vec<_>>().join(", "),
            aliases = aliases
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", "),
            test = test
                .map(|(t, v)| format!(
                    "Some(({}, {}))",
                    raw_string_literal(&t),
                    raw_string_literal(&v)
                ))
                .unwrap_or("None".to_string()),
            os = os
                .iter()
                .map(|o| format!("\"{o}\""))
                .collect::<Vec<_>>()
                .join(", "),
            depends = depends
                .iter()
                .map(|d| format!("\"{d}\""))
                .collect::<Vec<_>>()
                .join(", "),
            idiomatic_files = idiomatic_files
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", "),
            detect = detect
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", "),
            overrides = overrides
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", "),
        );
        lines.push(format!(r#"    m.insert("{short}", {rt});"#));
        for alias in aliases {
            lines.push(format!(r#"    m.insert("{alias}", {rt});"#));
        }
    }
    lines.push("    m".to_string());
    lines.push("}".to_string());

    fs::write(&dest_path, lines.join("\n")).unwrap();
}

fn codegen_settings() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("settings.rs");
    let mut lines = vec![
        r#"#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(partial_attr(derive(Clone, Serialize, Default)))]
pub struct Settings {"#
            .to_string(),
    ];

    println!("cargo:rerun-if-changed=settings.toml");
    let settings_toml = fs::read_to_string("settings.toml").expect("Failed to read settings.toml");
    let settings: toml::Table =
        toml::de::from_str(&settings_toml).expect("Failed to parse settings.toml");
    let props_to_code = |key: &str, props: &toml::Value| {
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
            lines.push(format!(
                "    pub {}: Settings{},",
                key,
                key.to_upper_camel_case()
            ));
        }
        lines.join("\n")
    };
    for (key, props) in &settings {
        lines.push(props_to_code(key, props));
    }
    lines.push("}".to_string());

    let nested_settings = settings
        .iter()
        .filter(|(_, v)| !v.as_table().unwrap().contains_key("type"))
        .collect::<Vec<_>>();
    for (child, props) in &nested_settings {
        lines.push(format!(
            r#"
#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(partial_attr(derive(Clone, Serialize, Default)))]
#[config(partial_attr(serde(deny_unknown_fields)))]
pub struct Settings{name} {{"#,
            name = child.to_upper_camel_case()
        ));

        for (key, props) in props.as_table().unwrap() {
            lines.push(props_to_code(key, props));
        }
        lines.push("}".to_string());
    }

    lines.push(
        r#"
pub static SETTINGS_META: Lazy<IndexMap<&'static str, SettingsMeta>> = Lazy::new(|| {
    indexmap!{"#
            .to_string(),
    );
    let push_deprecated_fields = |lines: &mut Vec<String>, props: &toml::Table| {
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
    };
    for (name, props) in &settings {
        let props = props.as_table().unwrap();
        if let Some(type_) = props.get("type").map(|v| v.as_str().unwrap()) {
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
            push_deprecated_fields(&mut lines, props);
            lines.push("    },".to_string());
        }
    }
    for (name, props) in &nested_settings {
        for (key, props) in props.as_table().unwrap() {
            let props = props.as_table().unwrap();
            if let Some(type_) = props.get("type").map(|v| v.as_str().unwrap()) {
                // We could shadow the 'type_' variable, but its a best practice to avoid shadowing.
                // Thus, we introduce 'meta_type' here.
                let meta_type = match type_ {
                    "IndexMap<String, String>" => "IndexMap",
                    other => other,
                };
                lines.push(format!(
                    r#"    "{name}.{key}" => SettingsMeta {{
        type_: SettingsType::{meta_type},"#,
                ));
            }
            if let Some(description) = props.get("description") {
                let description = description.as_str().unwrap().to_string();
                lines.push(format!(
                    "        description: {},",
                    raw_string_literal(&description)
                ));
            }
            push_deprecated_fields(&mut lines, props);
            lines.push("    },".to_string());
        }
    }
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
