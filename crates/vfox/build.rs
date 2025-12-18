use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    codegen_embedded_plugins();
}

/// Convert a path to a string with forward slashes (required for include_str! on Windows)
fn path_to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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

        // Metadata - use absolute path with forward slashes for cross-platform include_str!
        let metadata_path = embedded_dir.join(name).join("metadata.lua");
        code.push_str(&format!(
            "    metadata: include_str!(\"{}\"),\n",
            path_to_forward_slashes(&metadata_path)
        ));

        // Hooks
        code.push_str("    hooks: &[\n");
        for hook in &files.hooks {
            let hook_path = embedded_dir
                .join(name)
                .join("hooks")
                .join(format!("{}.lua", hook));
            code.push_str(&format!(
                "        (\"{}\", include_str!(\"{}\")),\n",
                hook,
                path_to_forward_slashes(&hook_path)
            ));
        }
        code.push_str("    ],\n");

        // Lib files
        code.push_str("    lib: &[\n");
        for lib in &files.lib {
            let lib_path = embedded_dir
                .join(name)
                .join("lib")
                .join(format!("{}.lua", lib));
            code.push_str(&format!(
                "        (\"{}\", include_str!(\"{}\")),\n",
                lib,
                path_to_forward_slashes(&lib_path)
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
