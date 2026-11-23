use crate::error::Result;
use mlua::{ExternalResult, Lua, MultiValue, Table, Value};
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink as _symlink};
#[cfg(windows)]
use std::os::windows::fs::symlink_dir;
#[cfg(windows)]
use std::os::windows::fs::symlink_file;
use std::path::{Path, PathBuf};

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

pub(crate) fn mod_file(lua: &Lua) -> Result<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
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
            (
                "stat",
                lua.create_async_function(|_lua: mlua::Lua, path: String| async move {
                    stat(&_lua, path).await
                })?,
            ),
            ("list", lua.create_function(list)?),
            ("glob", lua.create_function(glob)?),
            ("move", lua.create_function(move_path)?),
            ("mkdir", lua.create_function(mkdir)?),
            ("remove", lua.create_function(remove)?),
        ])?,
    )?)
}

fn paths_to_strings(paths: Vec<PathBuf>) -> Vec<String> {
    let mut paths = paths
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn list(_lua: &Lua, path: String) -> mlua::Result<Vec<String>> {
    xx::file::ls(path).map(paths_to_strings).into_lua_err()
}

fn glob(_lua: &Lua, pattern: String) -> mlua::Result<Vec<String>> {
    let paths = glob::glob(&pattern)
        .map_err(mlua::Error::external)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(mlua::Error::external)?;
    Ok(paths_to_strings(paths))
}

fn move_path(_lua: &Lua, (source, destination): (String, String)) -> mlua::Result<()> {
    xx::file::mv(source, destination).into_lua_err()
}

/// Create `path` and any missing parents; an existing directory is not an error.
fn mkdir(_lua: &Lua, path: String) -> mlua::Result<()> {
    std::fs::create_dir_all(path).into_lua_err()
}

/// Remove a file or symlink, or a directory tree when `{ recursive = true }`.
///
/// A symlink is removed itself and never followed, so a link to a directory
/// leaves its target alone.
fn remove(_lua: &Lua, (path, options): (String, Option<Table>)) -> mlua::Result<()> {
    let recursive = match options {
        Some(options) => options.get::<Option<bool>>("recursive")?.unwrap_or(false),
        None => false,
    };
    // A trailing separator makes the OS resolve a symlink to its target, which
    // would delete through the link. `components` drops it and keeps a root.
    let path: PathBuf = Path::new(&path).components().collect();
    let path = path.as_path();
    let meta = std::fs::symlink_metadata(path).into_lua_err()?;
    if meta.is_symlink() {
        // A Windows symlink to a directory is removed with remove_dir.
        return std::fs::remove_file(path)
            .or_else(|_| std::fs::remove_dir(path))
            .into_lua_err();
    }
    if meta.is_dir() {
        if !recursive {
            return Err(mlua::Error::external(format!(
                "{} is a directory: pass {{ recursive = true }} to remove it",
                path.display()
            )));
        }
        return std::fs::remove_dir_all(path).into_lua_err();
    }
    std::fs::remove_file(path).into_lua_err()
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

async fn stat(lua: &Lua, path: String) -> mlua::Result<Value> {
    let path = Path::new(&path);
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Nil),
        Err(e) => return Err(mlua::Error::external(e)),
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let accessed = meta
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let created = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let table = lua.create_table()?;
    table.set("size", meta.len())?;
    table.set("is_file", meta.is_file())?;
    table.set("is_dir", meta.is_dir())?;
    table.set("is_symlink", meta.is_symlink())?;
    table.set("modified", modified)?;
    table.set("accessed", accessed)?;
    table.set("created", created)?;
    #[cfg(unix)]
    {
        table.set("mode", format!("{:o}", meta.permissions().mode() & 0o7777))?;
    }
    #[cfg(not(unix))]
    {
        table.set("mode", Value::Nil)?;
    }
    Ok(Value::Table(table))
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

    #[test]
    fn test_stat() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let file_path = temp_dir.path().join("stat-file.txt");
        let file_path_str = file_path.to_string_lossy().to_string();
        // only read by the `#[cfg(unix)]` permissions assertion further down
        #[cfg(unix)]
        let file_path_str_clone = file_path_str.clone();
        let dir_path = temp_dir.path().join("stat-dir");
        let dir_path_str = dir_path.to_string_lossy().to_string();
        let nonexistent_path_str = temp_dir
            .path()
            .join("nonexistent.txt")
            .to_string_lossy()
            .to_string();

        fs::write(&file_path, "test content").unwrap();
        fs::create_dir(&dir_path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o644);
            fs::set_permissions(&file_path, perms).unwrap();
        }
        let lua = Lua::new();
        mod_file(&lua).unwrap();

        lua.load(mlua::chunk! {
            local file = require("file")

            local st = file.stat($file_path_str)
            assert(st ~= nil, "stat should return a table for existing file")
            assert(st.is_file == true, "should be a file")
            assert(st.is_dir == false, "should not be a directory")
            assert(st.is_symlink == false, "should not be a symlink")
            assert(st.size == string.len("test content"), "size should match content length")
            assert(type(st.modified) == "number", "modified should be a number")

            local st2 = file.stat($dir_path_str)
            assert(st2.is_dir == true, "should be a directory")
            assert(st2.is_file == false, "should not be a file")

            local st3 = file.stat($nonexistent_path_str)
            assert(st3 == nil, "stat should return nil for nonexistent")
        })
        .exec()
        .unwrap();

        #[cfg(unix)]
        {
            lua.load(mlua::chunk! {
                local file = require("file")
                local st = file.stat($file_path_str_clone)
                assert(st.mode == "644", "mode should be permission bits only, got: " .. (st.mode or "nil"))
            })
            .exec()
            .unwrap();
        }
    }

    #[test]
    fn test_list_glob_and_move() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let source_dir = temp_dir.path().join("source");
        let nested_dir = source_dir.join("nested");
        let destination_dir = temp_dir.path().join("destination");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(source_dir.join("a.txt"), "a").unwrap();
        fs::write(source_dir.join("b.log"), "b").unwrap();
        fs::write(nested_dir.join("c.txt"), "c").unwrap();

        let source_dir_str = source_dir.to_string_lossy().to_string();
        let glob_pattern = source_dir.join("*.txt").to_string_lossy().to_string();
        let nested_dir_str = nested_dir.to_string_lossy().to_string();
        let destination_dir_str = destination_dir.to_string_lossy().to_string();
        let lua = Lua::new();
        mod_file(&lua).unwrap();

        lua.load(mlua::chunk! {
            local file = require("file")
            local entries = file.list($source_dir_str)
            assert(#entries == 3, "list should return immediate directory entries")
            assert(entries[1] < entries[2] and entries[2] < entries[3], "list should be sorted")

            local matches = file.glob($glob_pattern)
            assert(#matches == 1, "glob should return matching files")
            assert(matches[1]:match("a%.txt$"), "glob should return the matching path")

            file.move($nested_dir_str, $destination_dir_str)
        })
        .exec()
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination_dir.join("c.txt")).unwrap(),
            "c"
        );
        assert!(!nested_dir.exists());
    }

    #[test]
    fn test_mkdir_creates_parents_and_accepts_an_existing_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let nested = temp_dir.path().join("a").join("b").join("c");
        let nested_str = nested.to_string_lossy().to_string();
        let file_path = temp_dir.path().join("plain.txt");
        let file_path_str = file_path.to_string_lossy().to_string();
        fs::write(&file_path, "x").unwrap();
        let lua = Lua::new();
        mod_file(&lua).unwrap();

        lua.load(mlua::chunk! {
            local file = require("file")
            file.mkdir($nested_str)
            file.mkdir($nested_str) -- already there
            local ok = pcall(file.mkdir, $file_path_str)
            assert(not ok, "mkdir over a file must raise")
        })
        .exec()
        .unwrap();

        assert!(nested.is_dir());
    }

    #[test]
    fn test_remove_deletes_files_and_requires_recursive_for_directories() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let file_path = temp_dir.path().join("gone.txt");
        let dir_path = temp_dir.path().join("tree");
        fs::write(&file_path, "x").unwrap();
        fs::create_dir_all(dir_path.join("inner")).unwrap();
        fs::write(dir_path.join("inner").join("f.txt"), "x").unwrap();
        let missing_str = temp_dir
            .path()
            .join("missing")
            .to_string_lossy()
            .to_string();
        let file_path_str = file_path.to_string_lossy().to_string();
        let dir_path_str = dir_path.to_string_lossy().to_string();
        let dir_path_str_again = dir_path_str.clone();
        let lua = Lua::new();
        mod_file(&lua).unwrap();

        lua.load(mlua::chunk! {
            local file = require("file")
            file.remove($file_path_str)

            local ok, err = pcall(file.remove, $dir_path_str)
            assert(not ok and tostring(err):find("recursive", 1, true), "a directory needs recursive = true")
            ok = pcall(file.remove, $dir_path_str, { recursive = false })
            assert(not ok, "recursive = false behaves like the default")

            ok = pcall(file.remove, $missing_str)
            assert(not ok, "a missing path raises")
        })
        .exec()
        .unwrap();
        assert!(!file_path.exists());
        assert!(dir_path.join("inner").join("f.txt").exists());

        lua.load(mlua::chunk! {
            require("file").remove($dir_path_str_again, { recursive = true })
        })
        .exec()
        .unwrap();
        assert!(!dir_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_deletes_a_symlink_without_following_it() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let target = temp_dir.path().join("target");
        let link = temp_dir.path().join("link");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), "x").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let link_str = link.to_string_lossy().to_string();
        let lua = Lua::new();
        mod_file(&lua).unwrap();

        // Even with recursive = true, only the link goes.
        lua.load(mlua::chunk! {
            require("file").remove($link_str, { recursive = true })
        })
        .exec()
        .unwrap();

        assert!(fs::symlink_metadata(&link).is_err());
        assert!(target.join("keep.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_ignores_a_trailing_separator_on_a_symlink() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let target = temp_dir.path().join("target");
        let link = temp_dir.path().join("link");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), "x").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        // "link/" names the link's target to the OS, so it must not be used as is.
        let link_str = format!("{}/", link.display());
        let lua = Lua::new();
        mod_file(&lua).unwrap();

        lua.load(mlua::chunk! {
            require("file").remove($link_str, { recursive = true })
        })
        .exec()
        .unwrap();

        assert!(fs::symlink_metadata(&link).is_err());
        assert!(target.join("keep.txt").exists());
    }
}
