use crate::error::Result;
use mlua::{ExternalResult, Lua, MultiValue, Table};

use super::get_or_create_loaded;
#[cfg(unix)]
use std::os::unix::fs::symlink as _symlink;
#[cfg(windows)]
use std::os::windows::fs::symlink_dir;
#[cfg(windows)]
use std::os::windows::fs::symlink_file;
use std::path::Path;

fn join_path(_lua: &Lua, args: MultiValue) -> mlua::Result<String> {
    let sep = std::path::MAIN_SEPARATOR;
    let mut parts = vec![];
    for v in args {
        let s = v.to_string()?;
        if !s.is_empty() {
            parts.push(s);
        }
    }
    Ok(parts.join(&sep.to_string()))
}

pub fn mod_file(lua: &Lua) -> Result<()> {
    let loaded: Table = get_or_create_loaded(lua)?;
    Ok(loaded.set(
        "file",
        lua.create_table_from(vec![
            (
                "read",
                lua.create_async_function(|_lua: mlua::Lua, input| async move {
                    read(&_lua, input).await
                })?,
            ),
            (
                "symlink",
                lua.create_async_function(|_lua: mlua::Lua, input| async move {
                    symlink(&_lua, input).await
                })?,
            ),
            ("join_path", lua.create_function(join_path)?),
            (
                "exists",
                lua.create_async_function(|_lua: mlua::Lua, input| async move {
                    exists(&_lua, input).await
                })?,
            ),
        ])?,
    )?)
}

async fn read(_lua: &Lua, input: MultiValue) -> mlua::Result<String> {
    let args: Vec<String> = input
        .into_iter()
        .map(|v| v.to_string())
        .collect::<mlua::Result<_>>()?;
    let path = Path::new(&args[0]);
    std::fs::read_to_string(path).into_lua_err()
}

async fn symlink(_lua: &Lua, input: MultiValue) -> mlua::Result<()> {
    let input: Vec<String> = input
        .into_iter()
        .map(|v| v.to_string())
        .collect::<mlua::Result<_>>()?;
    let src = Path::new(&input[0]);
    let dst = Path::new(&input[1]);
    #[cfg(windows)]
    {
        if src.is_dir() {
            symlink_dir(src, dst).into_lua_err()?;
        } else {
            symlink_file(src, dst).into_lua_err()?;
        }
    }
    #[cfg(unix)]
    _symlink(src, dst).into_lua_err()?;
    Ok(())
}

async fn exists(_lua: &Lua, input: MultiValue) -> mlua::Result<bool> {
    let args: Vec<String> = input
        .into_iter()
        .map(|v| v.to_string())
        .collect::<mlua::Result<_>>()?;
    let path = Path::new(&args[0]);
    std::fs::exists(path).into_lua_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_read() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let filepath = temp_dir.path().join("file-read.txt");
        let filepath_str = filepath.to_string_lossy().to_string();
        fs::write(&filepath, "hello world").unwrap();
        let lua = Lua::new();
        mod_file(&lua).unwrap();
        lua.load(mlua::chunk! {
            local file = require("file")
            local success, contents = pcall(file.read, $filepath_str)
            if not success then
                error("Failed to read: " .. contents)
            end
            if contents == nil then
                error("contents should not be nil")
            elseif contents ~= "hello world" then
                error("contents expected to be 'hello world', was actually:" .. contents)
            end
        })
        .exec()
        .unwrap();
        // TempDir automatically cleans up when dropped
    }

    #[test]
    fn test_symlink() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let src_path = temp_dir.path().join("symlink_src");
        let dst_path = temp_dir.path().join("symlink_dst");
        let src_path_str = src_path.to_string_lossy().to_string();
        let dst_path_str = dst_path.to_string_lossy().to_string();
        let lua = Lua::new();
        mod_file(&lua).unwrap();
        lua.load(mlua::chunk! {
            local file = require("file")
            file.symlink($src_path_str, $dst_path_str)
        })
        .exec()
        .unwrap();
        assert_eq!(fs::read_link(&dst_path).unwrap(), src_path);
        // TempDir automatically cleans up when dropped
    }

    #[test]
    fn test_exists() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let existing_file = temp_dir.path().join("exists.txt");
        let existing_file_str = existing_file.to_string_lossy().to_string();
        let nonexistent_file_str = temp_dir
            .path()
            .join("nonexistent.txt")
            .to_string_lossy()
            .to_string();

        fs::write(&existing_file, "test content").unwrap();
        let lua = Lua::new();
        mod_file(&lua).unwrap();

        lua.load(mlua::chunk! {
            local file = require("file")
            local existing_exists = file.exists($existing_file_str)
            local nonexistent_exists = file.exists($nonexistent_file_str)

            if not existing_exists then
                error("Expected existing file to exist")
            end
            if nonexistent_exists then
                error("Expected nonexistent file to not exist")
            end
        })
        .exec()
        .unwrap();
    }
}
