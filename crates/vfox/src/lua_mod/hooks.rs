use crate::embedded_plugins::EmbeddedPlugin;
use crate::error::Result;
use mlua::Lua;
use std::collections::BTreeSet;
use std::path::Path;

pub struct HookFunc {
    _name: &'static str,
    pub filename: &'static str,
}

#[rustfmt::skip]
pub const HOOK_FUNCS: [HookFunc; 12] = [
    HookFunc { _name: "Available", filename: "available" },
    HookFunc { _name: "PreInstall", filename: "pre_install" },
    HookFunc { _name: "EnvKeys", filename: "env_keys" },
    HookFunc { _name: "PostInstall", filename: "post_install" },
    HookFunc { _name: "PreUse", filename: "pre_use" },
    HookFunc { _name: "ParseLegacyFile", filename: "parse_legacy_file" },
    HookFunc { _name: "PreUninstall", filename: "pre_uninstall" },

    // backend
    HookFunc { _name: "BackendListVersions", filename: "backend_list_versions" },
    HookFunc { _name: "BackendInstall", filename: "backend_install" },
    HookFunc { _name: "BackendExecEnv", filename: "backend_exec_env" },

    // mise
    HookFunc { _name: "MiseEnv", filename: "mise_env" },
    HookFunc { _name: "MisePath", filename: "mise_path" },
];

pub fn mod_hooks(lua: &Lua, root: &Path) -> Result<BTreeSet<&'static str>> {
    let mut hooks = BTreeSet::new();
    for hook in &HOOK_FUNCS {
        let hook_path = root.join("hooks").join(format!("{}.lua", hook.filename));
        if hook_path.exists() {
            lua.load(hook_path).exec()?;
            hooks.insert(hook.filename);
        }
    }
    Ok(hooks)
}

pub fn hooks_embedded(lua: &Lua, embedded: &EmbeddedPlugin) -> Result<BTreeSet<&'static str>> {
    let mut hooks = BTreeSet::new();

    // Get package.loaded table to preload hooks
    let package: mlua::Table = lua.globals().get("package")?;
    let loaded: mlua::Table = package.get("loaded")?;

    for (hook_name, hook_code) in embedded.hooks {
        // Execute the hook code to define the function
        lua.load(*hook_code).exec()?;

        // Also preload into package.loaded so require("hooks/<name>") works
        // The hook code typically defines a function on the PLUGIN table
        // We need to register a module that can be required
        let module_name = format!("hooks/{}", hook_name);
        // Create a simple module that returns true (the hook code has already been executed)
        loaded.set(module_name, true)?;

        // Find the matching hook filename from HOOK_FUNCS
        for hook in &HOOK_FUNCS {
            if hook.filename == *hook_name {
                hooks.insert(hook.filename);
                break;
            }
        }
    }
    Ok(hooks)
}
