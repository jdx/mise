use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use std::collections::HashSet;
use std::path::Path;
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
}

fn codegen_registry() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("registry.rs");
    let mut lines = vec![r#"
const _REGISTRY: &[(&str, &[&str], &[&str])] = &["#
        .to_string()];

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
        lines.push(format!(
            r#"    ("{short}", &["{fulls}"], &[{aliases}]),"#,
            fulls = fulls.join("\", \""),
            aliases = aliases
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    lines.push(r#"];"#.to_string());

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
