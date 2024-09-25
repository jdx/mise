use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use std::path::Path;
use std::{env, fs};

fn main() {
    cfg_aliases::cfg_aliases! {
        vfox: { any(feature = "vfox", target_os = "windows") },
        asdf: { any(feature = "asdf", not(target_os = "windows")) },
    }
    built::write_built_file().expect("Failed to acquire build-time information");

    codegen_settings();
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
        if let Some(type_) = props.get("type") {
            let mut opts = IndexMap::new();
            if let Some(env) = props.get("env") {
                opts.insert("env".to_string(), env.to_string());
            }
            if let Some(default) = props.get("default") {
                opts.insert("default".to_string(), default.to_string());
            } else if type_.as_str().unwrap() == "bool" {
                opts.insert("default".to_string(), "false".to_string());
            }
            if let Some(parse_env) = props.get("parse_env") {
                opts.insert(
                    "parse_env".to_string(),
                    parse_env.as_str().unwrap().to_string(),
                );
            }
            dbg!(&opts);
            lines.push(format!(
                "    #[config({})]",
                opts.iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            lines.push(format!("    pub {}: {},", key, type_.as_str().unwrap()));
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
    for (child, props) in nested_settings {
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

    fs::write(&dest_path, lines.join("\n")).unwrap();
}
