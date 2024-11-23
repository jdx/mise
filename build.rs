use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    cfg_aliases::cfg_aliases! {
        asdf: { any(feature = "asdf", not(target_os = "windows")) },
        macos: { target_os = "macos" },
        vfox: { any(feature = "vfox", target_os = "windows") },
    }
    built::write_built_file().expect("Failed to acquire build-time information");

    codegen_settings();
    codegen_registry();
    codegen_aqua();
}

fn codegen_registry() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("registry.rs");
    let mut lines = vec!["[".to_string()];

    let registry: toml::Table = fs::read_to_string("registry.toml")
        .unwrap()
        .parse()
        .unwrap();

    let tools = registry.get("tools").unwrap().as_table().unwrap();
    let mut trusted_ids = HashSet::new();
    for (short, info) in tools {
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
            let t = t.as_array().unwrap();
            (
                t[0].as_str().unwrap().to_string(),
                t[1].as_str().unwrap().to_string(),
            )
        });
        let backends = info.get("backends").unwrap().as_array().unwrap();
        let mut fulls = vec![];
        for backend in backends {
            match backend {
                toml::Value::String(backend) => {
                    fulls.push(backend.to_string());
                }
                toml::Value::Table(backend) => {
                    fulls.push(backend.get("full").unwrap().as_str().unwrap().to_string());
                    if let Some(trust) = backend.get("trust") {
                        if trust.as_bool().unwrap() {
                            trusted_ids.insert(short);
                        }
                    }
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
        let rt = format!(
            r#"RegistryTool{{short: "{short}", backends: vec!["{backends}"], aliases: &[{aliases}], test: &{test}, os: &[{os}], depends: &[{depends}]}}"#,
            backends = fulls.join("\", \""),
            aliases = aliases
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", "),
            test = test
                .map(|(t, v)| format!("Some((\"{t}\", \"{v}\"))", t = t, v = v))
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
        );
        lines.push(format!(r#"    ("{short}", {rt}),"#));
        for alias in aliases {
            lines.push(format!(r#"    ("{alias}", {rt}),"#));
        }
    }
    lines.push(r#"].into()"#.to_string());

    fs::write(&dest_path, lines.join("\n")).unwrap();
}

fn codegen_settings() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("settings.rs");
    let mut lines = vec![r#"
#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(partial_attr(derive(Clone, Serialize, Default)))]
pub struct Settings {"#
        .to_string()];

    let settings: toml::Table = fs::read_to_string("settings.toml")
        .unwrap()
        .parse()
        .unwrap();
    let props_to_code = |key: &str, props: &toml::Value| {
        let mut lines = vec![];
        let props = props.as_table().unwrap();
        if let Some(description) = props.get("description") {
            lines.push(format!("    /// {}", description.as_str().unwrap()));
        }
        let type_ = props
            .get("rust_type")
            .map(|rt| rt.as_str().unwrap())
            .or(props.get("type").map(|t| match t.as_str().unwrap() {
                "Bool" => "bool",
                "String" => "String",
                "Integer" => "i64",
                "Url" => "String",
                "Path" => "PathBuf",
                "Duration" => "String",
                "ListString" => "Vec<String>",
                "ListPath" => "Vec<PathBuf>",
                t => panic!("Unknown type: {}", t),
            }));
        if let Some(type_) = type_ {
            let type_ = if props.get("optional").is_some_and(|v| v.as_bool().unwrap()) {
                format!("Option<{}>", type_)
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
            lines.push(format!(
                "    #[config({})]",
                opts.iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            lines.push(format!("    pub {}: {},", key, type_));
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
            r#"#[derive(Config, Default, Debug, Clone, Serialize)]
#[config(partial_attr(derive(Clone, Serialize, Default)))]
#[config(partial_attr(serde(deny_unknown_fields)))]
pub struct Settings{name} {{
"#,
            name = child.to_upper_camel_case()
        ));

        for (key, props) in props.as_table().unwrap() {
            lines.push(props_to_code(key, props));
        }
        lines.push("}".to_string());
    }

    lines.push(
        r#"
pub static SETTINGS_META: Lazy<IndexMap<String, SettingsMeta>> = Lazy::new(|| {
    indexmap!{
    "#
        .to_string(),
    );
    for (name, props) in &settings {
        let props = props.as_table().unwrap();
        if let Some(type_) = props.get("type").map(|v| v.as_str().unwrap()) {
            lines.push(format!(
                r#"    "{name}".to_string() => SettingsMeta {{
        type_: SettingsType::{type_},
    }},"#,
            ));
        }
    }
    for (name, props) in &nested_settings {
        for (key, props) in props.as_table().unwrap() {
            let props = props.as_table().unwrap();
            if let Some(type_) = props.get("type").map(|v| v.as_str().unwrap()) {
                lines.push(format!(
                    r#"    "{name}.{key}".to_string() => SettingsMeta {{
        type_: SettingsType::{type_},
    }},"#,
                ));
            }
        }
    }
    lines.push(
        r#"    }
});
    "#
        .to_string(),
    );

    fs::write(&dest_path, lines.join("\n")).unwrap();
}

// pub static AQUA_STANDARD_REGISTRY_FILES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
//     include!(concat!(env!("OUT_DIR"), "/aqua_standard_registry.rs"));
// });

fn codegen_aqua() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("aqua_standard_registry.rs");
    let mut lines = vec!["[".to_string()];
    for (k, v) in aqua_registries(&registry_dir()).unwrap_or_default() {
        lines.push(format!(r####"    ("{k}", r###"{v}"###),"####));
    }
    lines.push("].into()".to_string());
    fs::write(&dest_path, lines.join("\n")).unwrap();
}

fn ls(path: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    fs::read_dir(path)?
        .map(|entry| entry.map(|e| e.path()))
        .collect()
}

fn aqua_registries(d: &Path) -> Result<Vec<(String, String)>, std::io::Error> {
    let mut registries = vec![];
    for f in ls(d)? {
        if f.is_dir() {
            registries.extend(aqua_registries(&f)?);
        } else if f.file_name() == Some("registry.yaml".as_ref()) {
            registries.push((
                f.parent()
                    .unwrap()
                    .strip_prefix(registry_dir())
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                fs::read_to_string(&f).unwrap(),
            ));
        }
    }
    Ok(registries)
}

fn registry_dir() -> PathBuf {
    PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("aqua-registry")
        .join("pkgs")
}
