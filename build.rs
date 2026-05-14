use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use serde::Serialize as _;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::{env, fs};

use aqua_registry::encode_package_rkyv;
use aqua_registry::types::{AquaPackage, RegistryPackageRow, RegistryYaml};
use eyre::{Result, eyre};
use serde_yaml::Value;

fn main() -> Result<()> {
    cfg_aliases::cfg_aliases! {
        asdf: { any(feature = "asdf", not(target_os = "windows")) },
        macos: { target_os = "macos" },
        linux: { target_os = "linux" },
        vfox: { any(feature = "vfox", target_os = "windows") },
    }
    built::write_built_file()?;

    codegen_settings();
    codegen_registry();
    codegen_aqua_standard_registry()?;
    Ok(())
}

#[derive(Debug)]
struct AquaPackageRegistry {
    id: String,
    content: Vec<u8>,
    aliases: Vec<String>,
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
                    let mut value = String::new();
                    v.serialize(toml::ser::ValueSerializer::new(&mut value))
                        .unwrap_or_else(|e| panic!("failed to serialize registry option {k}: {e}"));
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
    let mut generated_entries = BTreeMap::new();

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
            let tools = t
                .get("tools")
                .map(|tools| {
                    let mut tools = tools
                        .as_array()
                        .unwrap_or_else(|| panic!("[{short}] 'test.tools' must be an array"))
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .unwrap_or_else(|| {
                                    panic!("[{short}] 'test.tools' must contain only strings")
                                })
                                .to_string()
                        })
                        .collect::<Vec<_>>();
                    tools.sort();
                    tools
                })
                .unwrap_or_default();
            (cmd, expected, tools)
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
            r#"RegistryTool{{short: "{short}", description: {description}, backends: &[{backends}], aliases: &[{aliases}], test: &{test}, os: &[{os}], idiomatic_files: &[{idiomatic_files}], detect: &[{detect}], overrides: &[{overrides}]}}"#,
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
                .map(|(cmd, expected, tools)| format!(
                    "Some(RegistryToolTest{{ cmd: {}, expected: {}, tools: &[{}] }})",
                    raw_string_literal(&cmd),
                    raw_string_literal(&expected),
                    tools
                        .iter()
                        .map(|tool| format!("\"{tool}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .unwrap_or("None".to_string()),
            os = os
                .iter()
                .map(|o| format!("\"{o}\""))
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
        generated_entries.insert(short.clone(), rt.clone());
        for alias in aliases {
            generated_entries.insert(alias, rt.clone());
        }
    }

    let entries = generated_entries.into_iter().collect::<Vec<_>>();
    fs::write(&dest_path, registry_code(&entries)).unwrap();
}

fn registry_code(entries: &[(String, String)]) -> String {
    let mut code = String::from("Registry {\n    entries: &[\n");
    for (key, tool) in entries {
        code.push_str(&format!("        ({key:?}, {tool}),\n"));
    }
    code.push_str("    ],\n    lookup: ");
    code.push_str(&phf_usize_map_code(
        entries
            .iter()
            .enumerate()
            .map(|(index, (key, _))| (key.clone(), index.to_string()))
            .collect::<Vec<_>>(),
    ));
    code.push_str(",\n}");
    code
}

fn phf_usize_map_code(entries: Vec<(String, String)>) -> String {
    let mut map = phf_codegen::Map::new();
    for (key, value) in &entries {
        map.entry(key, value);
    }
    map.build().to_string()
}

fn codegen_aqua_standard_registry() -> Result<()> {
    let out_dir = env::var("OUT_DIR")?;
    let files_dest_path = Path::new(&out_dir).join("aqua_standard_registry_files.rs");
    let aliases_dest_path = Path::new(&out_dir).join("aqua_standard_registry_aliases.rs");
    let metadata_dest_path = Path::new(&out_dir).join("aqua_standard_registry_metadata.rs");
    let packages_dir = Path::new(&out_dir).join("aqua_standard_registry_packages");

    let registry_file = Path::new("vendor/aqua-registry/registry.yml");
    let metadata_file = Path::new("vendor/aqua-registry/metadata.json");

    println!("cargo:rerun-if-changed={}", registry_file.display());
    println!("cargo:rerun-if-changed={}", metadata_file.display());

    let registry_yaml = serde_yaml::from_str::<RegistryYaml>(&fs::read_to_string(registry_file)?)?;

    let registries = aqua_package_registries(&registry_yaml.packages)?;
    if registries.is_empty() {
        return Err(eyre!(
            "Aqua registry file {} contains no packages",
            registry_file.display()
        ));
    }

    fs::create_dir_all(&packages_dir)?;
    fs::write(
        files_dest_path,
        aqua_registry_files_code(&registries, &packages_dir)?,
    )?;
    fs::write(aliases_dest_path, aqua_registry_aliases_code(&registries)?)?;

    let metadata = serde_yaml::from_str::<Value>(&fs::read_to_string(metadata_file)?)?;
    let repository = yaml_string_field(&metadata, "repository").ok_or_else(|| {
        eyre!(
            "Aqua registry metadata file {} does not contain a repository",
            metadata_file.display()
        )
    })?;
    let tag = yaml_string_field(&metadata, "tag").ok_or_else(|| {
        eyre!(
            "Aqua registry metadata file {} does not contain a tag",
            metadata_file.display()
        )
    })?;

    fs::write(
        metadata_dest_path,
        format!("AquaRegistryMetadata {{ repository: {repository:?}, tag: {tag:?} }}"),
    )?;

    Ok(())
}

fn aqua_package_registries(rows: &[RegistryPackageRow]) -> Result<Vec<AquaPackageRegistry>> {
    let mut registries = Vec::new();
    let mut canonical_ids = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        let package = &row.package;
        let Some(id) = aqua_canonical_package_id(package) else {
            println!(
                "cargo:warning=skipping aqua registry package row {index}: missing name, repo_owner/repo_name, and path"
            );
            continue;
        };
        if let Some(existing) = canonical_ids.insert(id.clone(), index) {
            return Err(eyre!(
                "baked aqua registry package id collision for {id:?}: rows {existing} and {index}"
            ));
        }
        let content = encode_package_rkyv(package)?;
        registries.push(AquaPackageRegistry {
            id,
            content,
            aliases: row.aliases.clone(),
        });
    }
    Ok(registries)
}

fn aqua_registry_files_code(
    registries: &[AquaPackageRegistry],
    packages_dir: &Path,
) -> Result<String> {
    let mut used_stems = HashMap::new();
    let mut entries = registries
        .iter()
        .map(|registry| {
            let stem = aqua_package_file_stem(&registry.id);
            if let Some(other_id) = used_stems.insert(stem.clone(), registry.id.as_str()) {
                return Err(eyre!(
                    "baked aqua registry package filename collision for {other_id:?} and {:?}: {stem}",
                    registry.id
                ));
            }
            let filename = format!("{stem}.rkyv");
            let path = packages_dir.join(filename);
            fs::write(&path, &registry.content)?;
            Ok((registry.id.clone(), path))
        })
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(aqua_registry_bytes_map_code(&entries))
}

fn aqua_registry_bytes_map_code(entries: &[(String, PathBuf)]) -> String {
    let mut map = phf_codegen::Map::new();
    let mut values = Vec::new();
    for (key, path) in entries {
        values.push((
            key.clone(),
            format!(
                "include_bytes!({:?}).as_slice()",
                path.display().to_string()
            ),
        ));
    }
    for (key, value) in &values {
        map.entry(key, value);
    }
    map.build().to_string()
}

/// Hashes the canonical package ID with FNV-1a 64-bit to generate compact,
/// deterministic baked package filenames without leaking path separators.
fn aqua_package_file_stem(id: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn aqua_registry_aliases_code(registries: &[AquaPackageRegistry]) -> Result<String> {
    let canonical_ids = registries
        .iter()
        .map(|registry| registry.id.as_str())
        .collect::<HashSet<_>>();
    let mut aliases = BTreeMap::new();

    for registry in registries {
        for alias in &registry.aliases {
            if alias != &registry.id
                && !canonical_ids.contains(alias.as_str())
                && let Some(existing) = aliases.insert(alias.clone(), registry.id.clone())
                && existing != registry.id
            {
                return Err(eyre!(
                    "baked aqua registry alias collision for {alias:?}: {existing:?} and {:?}",
                    registry.id
                ));
            }
        }
    }

    let entries = aliases.into_iter().collect::<Vec<_>>();

    Ok(aqua_registry_string_map_code(&entries))
}

fn aqua_registry_string_map_code(entries: &[(String, String)]) -> String {
    let mut map = phf_codegen::Map::new();
    let mut values = Vec::new();
    for (key, value) in entries {
        values.push((key.clone(), format!("{value:?}")));
    }
    for (key, value) in &values {
        map.entry(key, value);
    }
    map.build().to_string()
}

fn aqua_canonical_package_id(package: &AquaPackage) -> Option<String> {
    package
        .name
        .clone()
        .or_else(|| {
            if package.repo_owner.is_empty() || package.repo_name.is_empty() {
                None
            } else {
                Some(format!("{}/{}", package.repo_owner, package.repo_name))
            }
        })
        .or_else(|| package.path.clone())
}

fn yaml_string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn codegen_settings() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("settings.rs");
    let mut lines = vec![
        r#"#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(layer_attr(derive(Clone, Serialize, Default)))]
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
#[config(layer_attr(derive(Clone, Serialize, Default)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
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
