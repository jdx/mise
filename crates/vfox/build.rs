#![allow(unknown_lints)]
#![deny(dead_code_pub_in_binary, unreachable_pub)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    codegen_embedded_plugins();
}

/// `include_str!` for a file under `embedded-plugins/`, resolved from the crate's manifest
/// directory when rustc compiles the generated code. The generated file must not contain the
/// checkout path: build script output is reused from a shared cache by jobs that check the
/// repository out somewhere else, where an absolute path no longer exists.
fn include_embedded(relative: &str) -> String {
    format!("include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/embedded-plugins/{relative}\"))")
}

fn codegen_embedded_plugins() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("embedded_plugins.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let embedded_dir = Path::new(&manifest_dir).join("embedded-plugins");

    // Tell Cargo to re-run if any embedded plugin files change
    println!("cargo:rerun-if-changed=embedded-plugins");

    if !embedded_dir.exists() {
        // Generate empty implementation if no embedded plugins
        let code = r#"
#[derive(Debug)]
pub struct EmbeddedPlugin {
    pub metadata: &'static str,
    pub hooks: &'static [(&'static str, &'static str)],
    pub lib: &'static [(&'static str, &'static str)],
}

pub fn get_embedded_plugin(_name: &str) -> Option<&'static EmbeddedPlugin> {
    None
}

pub fn list_embedded_plugins() -> &'static [&'static str] {
    &[]
}
"#;
        fs::write(&dest_path, code).unwrap();
        return;
    }

    let mut plugins: BTreeMap<String, PluginFiles> = BTreeMap::new();

    // Scan for plugin directories
    for entry in fs::read_dir(&embedded_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = path.file_name().unwrap().to_string_lossy().to_string();
        if !dir_name.starts_with("vfox-") {
            continue;
        }

        // Tell Cargo to re-run if this plugin directory or any Lua files change
        println!("cargo:rerun-if-changed={}", path.display());

        // Also track subdirectories and individual Lua files
        let hooks_dir = path.join("hooks");
        if hooks_dir.exists() {
            println!("cargo:rerun-if-changed={}", hooks_dir.display());
            for entry in fs::read_dir(&hooks_dir).unwrap().flatten() {
                if entry.path().extension().is_some_and(|ext| ext == "lua") {
                    println!("cargo:rerun-if-changed={}", entry.path().display());
                }
            }
        }
        let lib_dir = path.join("lib");
        if lib_dir.exists() {
            println!("cargo:rerun-if-changed={}", lib_dir.display());
            for entry in fs::read_dir(&lib_dir).unwrap().flatten() {
                if entry.path().extension().is_some_and(|ext| ext == "lua") {
                    println!("cargo:rerun-if-changed={}", entry.path().display());
                }
            }
        }
        let metadata_file = path.join("metadata.lua");
        if metadata_file.exists() {
            println!("cargo:rerun-if-changed={}", metadata_file.display());
        }

        let plugin = collect_plugin_files(&path);
        plugins.insert(dir_name, plugin);
    }

    // Generate Rust code
    let mut code = String::new();

    // Struct definition
    code.push_str(
        r#"
#[derive(Debug)]
pub struct EmbeddedPlugin {
    pub metadata: &'static str,
    pub hooks: &'static [(&'static str, &'static str)],
    pub lib: &'static [(&'static str, &'static str)],
}

"#,
    );

    // Generate static instances for each plugin
    for (name, files) in &plugins {
        let var_name = name.replace('-', "_").to_uppercase();
        code.push_str(&format!(
            "static {var_name}: EmbeddedPlugin = EmbeddedPlugin {{\n"
        ));

        // Metadata
        code.push_str(&format!(
            "    metadata: {},\n",
            include_embedded(&format!("{name}/metadata.lua"))
        ));

        // Hooks
        code.push_str("    hooks: &[\n");
        for hook in &files.hooks {
            code.push_str(&format!(
                "        (\"{}\", {}),\n",
                hook,
                include_embedded(&format!("{name}/hooks/{hook}.lua"))
            ));
        }
        code.push_str("    ],\n");

        // Lib files
        code.push_str("    lib: &[\n");
        for lib in &files.lib {
            code.push_str(&format!(
                "        (\"{}\", {}),\n",
                lib,
                include_embedded(&format!("{name}/lib/{lib}.lua"))
            ));
        }
        code.push_str("    ],\n");

        code.push_str("};\n\n");
    }

    // Generate lookup function
    code.push_str("pub fn get_embedded_plugin(name: &str) -> Option<&'static EmbeddedPlugin> {\n");
    code.push_str("    match name {\n");
    for name in plugins.keys() {
        let var_name = name.replace('-', "_").to_uppercase();
        let short_name = name.strip_prefix("vfox-").unwrap_or(name);
        code.push_str(&format!(
            "        \"{}\" | \"{}\" => Some(&{}),\n",
            name, short_name, var_name
        ));
    }
    code.push_str("        _ => None,\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");

    // Generate list function
    code.push_str("pub fn list_embedded_plugins() -> &'static [&'static str] {\n");
    code.push_str("    &[\n");
    for name in plugins.keys() {
        code.push_str(&format!("        \"{}\",\n", name));
    }
    code.push_str("    ]\n");
    code.push_str("}\n");

    fs::write(&dest_path, code).unwrap();
}

struct PluginFiles {
    hooks: Vec<String>,
    lib: Vec<String>,
}

fn collect_plugin_files(plugin_dir: &Path) -> PluginFiles {
    let mut hooks = Vec::new();
    let mut lib = Vec::new();

    // Collect hooks
    let hooks_dir = plugin_dir.join("hooks");
    if hooks_dir.exists() {
        for entry in fs::read_dir(&hooks_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "lua") {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                hooks.push(name);
            }
        }
    }
    hooks.sort();

    // Collect lib files
    let lib_dir = plugin_dir.join("lib");
    if lib_dir.exists() {
        for entry in fs::read_dir(&lib_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "lua") {
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                lib.push(name);
            }
        }
    }
    lib.sort();

    PluginFiles { hooks, lib }
}
