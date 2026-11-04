use mlua::Table;
use mlua::prelude::*;
use std::path::PathBuf;

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
        let mut deps = recorder.lock().unwrap();
        deps.watch(paths.into_iter().map(PathBuf::from));
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

    #[test]
    fn test_cache_records_declarations() {
        let project = tempfile::tempdir().unwrap();
        let pipfile = project.path().join("Pipfile");
        let lock = project.path().join("Pipfile.lock");
        let paths = vec![
            pipfile.to_string_lossy().to_string(),
            lock.to_string_lossy().to_string(),
        ];
        let lua = Lua::new();
        mod_cache(&lua).unwrap();
        let recorder = HookDependencyRecorder::default();
        lua.set_app_data(recorder.clone());
        lua.load(mlua::chunk! {
            local cache = require("mise.cache")
            cache.watch_files($paths)
            cache.disable()
        })
        .exec()
        .unwrap();
        let deps = recorder.lock().unwrap();
        assert_eq!(deps.watch_files, vec![pipfile, lock]);
        assert_eq!(deps.cwd, None);
        assert!(!deps.cacheable);
    }

    #[test]
    fn test_cache_records_the_directory_of_relative_paths() {
        let lua = Lua::new();
        mod_cache(&lua).unwrap();
        let recorder = HookDependencyRecorder::default();
        lua.set_app_data(recorder.clone());
        lua.load(mlua::chunk! {
            require("mise.cache").watch_files({ "Pipfile" })
        })
        .exec()
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        let deps = recorder.lock().unwrap();
        assert_eq!(deps.watch_files, vec![cwd.join("Pipfile")]);
        assert_eq!(deps.cwd, Some(cwd));
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
