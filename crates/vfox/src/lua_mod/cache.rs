use mlua::Table;
use mlua::prelude::*;

use crate::hook_dependencies::HookDependencyRecorder;

/// `mise.cache`: lets a hook say what its result depends on, so mise knows
/// when a cached result no longer applies. Without a caller recording it
/// (`Vfox::hook_dependencies`), both functions do nothing.
pub(crate) fn mod_cache(lua: &Lua) -> LuaResult<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    let cache = lua.create_table_from(vec![
        ("watch_files", lua.create_function(watch_files)?),
        ("disable", lua.create_function(disable)?),
    ])?;
    loaded.set("mise.cache", cache)?;
    Ok(())
}

/// Record files or directories whose changes make the hook's result stale. A
/// path that doesn't exist yet counts too: creating it makes the result stale.
/// A relative path resolves from the current directory.
fn watch_files(lua: &Lua, paths: Vec<String>) -> LuaResult<()> {
    if let Some(recorder) = lua.app_data_ref::<HookDependencyRecorder>() {
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut deps = recorder.lock().unwrap();
        deps.watch_files
            .extend(paths.into_iter().map(|path| cwd.join(path)));
    }
    Ok(())
}

/// Ask for the hook's result not to be cached, so it runs every time.
fn disable(lua: &Lua, _: ()) -> LuaResult<()> {
    if let Some(recorder) = lua.app_data_ref::<HookDependencyRecorder>() {
        recorder.lock().unwrap().cacheable = false;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_cache_records_declarations() {
        let lua = Lua::new();
        mod_cache(&lua).unwrap();
        let recorder = HookDependencyRecorder::default();
        lua.set_app_data(recorder.clone());
        lua.load(mlua::chunk! {
            local cache = require("mise.cache")
            cache.watch_files({ "/project/Pipfile", "/project/Pipfile.lock" })
            cache.disable()
        })
        .exec()
        .unwrap();
        let deps = recorder.lock().unwrap();
        assert_eq!(
            deps.watch_files,
            vec![
                PathBuf::from("/project/Pipfile"),
                PathBuf::from("/project/Pipfile.lock")
            ]
        );
        assert!(!deps.cacheable);
    }

    #[test]
    fn test_cache_without_recorder_does_nothing() {
        let lua = Lua::new();
        mod_cache(&lua).unwrap();
        lua.load(mlua::chunk! {
            local cache = require("mise.cache")
            cache.watch_files({ "/project/Pipfile" })
            cache.disable()
        })
        .exec()
        .unwrap();
    }
}
