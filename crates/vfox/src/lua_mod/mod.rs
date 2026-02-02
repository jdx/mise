mod archiver;
mod cmd;
mod compat;
mod env;
mod file;
mod hooks;
mod html;
mod http;
mod json;
mod log;
mod semver;
mod strings;

pub use archiver::mod_archiver as archiver;
pub use cmd::mod_cmd as cmd;
pub use compat::mod_compat as compat;
pub use env::mod_env as env;
pub use file::mod_file as file;
pub use hooks::hooks_embedded;
pub use hooks::mod_hooks as hooks;
pub use html::mod_html as html;
pub use http::mod_http as http;
pub use json::mod_json as json;
pub use log::mod_log as log;
pub use semver::mod_semver as semver;
pub use strings::mod_strings as strings;

use mlua::{Lua, Table};

/// Get or create the `_LOADED` global table used for module caching.
/// This replaces `package.loaded` since Luau does not have a `package` library.
/// Self-bootstrapping: creates the table and sets up the custom `require` function
/// if they don't exist yet, so tests can call individual mod_* functions without
/// full plugin setup.
pub(crate) fn get_or_create_loaded(lua: &Lua) -> mlua::Result<Table> {
    match lua.globals().get::<Table>("_LOADED") {
        Ok(t) => Ok(t),
        Err(_) => {
            // Create _LOADED
            let t = lua.create_table()?;
            lua.globals().set("_LOADED", t.clone())?;
            // Create _PRELOAD
            let preload = lua.create_table()?;
            lua.globals().set("_PRELOAD", preload)?;
            // Set up custom require
            install_require(lua)?;
            Ok(t)
        }
    }
}

/// Install the custom `require` function into the Lua globals.
fn install_require(lua: &Lua) -> mlua::Result<()> {
    let require_fn = lua.create_function(|lua, name: String| {
        // Resolve @lib alias (used in .luaurc for Luau type checking)
        // @lib/foo -> foo (for preload) or lib/foo (for filesystem)
        let preload_name = if let Some(suffix) = name.strip_prefix("@lib/") {
            suffix.to_string()
        } else {
            name.clone()
        };
        let fs_name = if let Some(suffix) = name.strip_prefix("@lib/") {
            format!("lib/{}", suffix)
        } else {
            name.clone()
        };

        let loaded: Table = lua.globals().get("_LOADED")?;
        // 1. Check cache (use original name for cache key)
        if let Ok(module) = loaded.get::<mlua::Value>(&*name)
            && module != mlua::Value::Nil
        {
            return Ok(module);
        }
        // 2. Check preload (use preload_name for lookup, original name for cache)
        if let Ok(preload) = lua.globals().get::<Table>("_PRELOAD")
            && let Ok(loader) = preload.get::<mlua::Function>(&*preload_name)
        {
            // Set sentinel before calling loader to prevent circular dependency recursion
            loaded.set(name.as_str(), true)?;
            let result: mlua::Result<mlua::Value> = loader.call(());
            match result {
                Ok(module) => {
                    let store = if module == mlua::Value::Nil {
                        mlua::Value::Boolean(true)
                    } else {
                        module.clone()
                    };
                    loaded.set(name.as_str(), store.clone())?;
                    return Ok(store);
                }
                Err(e) => {
                    // Clean up sentinel on failure so subsequent requires can retry
                    loaded.set(name.as_str(), mlua::Value::Nil)?;
                    return Err(e);
                }
            }
        }
        // 3. Search filesystem paths (use fs_name for path, original name for cache)
        if let Ok(paths) = lua.named_registry_value::<String>("_REQUIRE_PATHS") {
            for template in paths.split(';') {
                let file_path = template.replace('?', &fs_name);
                if std::path::Path::new(&file_path).exists() {
                    let code = std::fs::read_to_string(&file_path)
                        .map_err(mlua::ExternalError::into_lua_err)?;
                    // Set sentinel before loading to prevent circular dependency recursion
                    loaded.set(name.as_str(), true)?;
                    let result: mlua::Result<mlua::Value> =
                        lua.load(&code).set_name(format!("={}", file_path)).eval();
                    match result {
                        Ok(module) => {
                            let store = if module == mlua::Value::Nil {
                                mlua::Value::Boolean(true)
                            } else {
                                module.clone()
                            };
                            loaded.set(name.as_str(), store.clone())?;
                            return Ok(store);
                        }
                        Err(e) => {
                            // Clean up sentinel on failure so subsequent requires can retry
                            loaded.set(name.as_str(), mlua::Value::Nil)?;
                            return Err(e);
                        }
                    }
                }
            }
        }
        Err(mlua::Error::external(format!(
            "module '{}' not found",
            name
        )))
    })?;
    lua.globals().set("require", require_fn)?;
    Ok(())
}

/// Set up the custom `require` system with filesystem search paths.
/// This replaces Lua 5.1's `package`-based require since Luau has no `package` library.
pub fn setup_require(lua: &Lua) -> mlua::Result<()> {
    get_or_create_loaded(lua)?;
    Ok(())
}
