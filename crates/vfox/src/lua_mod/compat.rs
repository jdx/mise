//! Compatibility shims for Lua 5.1 stdlib functions missing in Luau.
//!
//! Luau does not include `io` or full `os` (no `os.getenv`, `os.execute`).
//! Many existing vfox plugins rely on these, so we provide Rust-backed
//! implementations injected into the Lua globals.

use mlua::{ExternalError, ExternalResult, Lua, Table, UserData, UserDataMethods, Value};
use std::process::Command;

/// Set up compatibility shims for `os` and `io` globals.
pub fn mod_compat(lua: &Lua) -> mlua::Result<()> {
    setup_os(lua)?;
    setup_io(lua)?;
    Ok(())
}

/// Extend the existing Luau `os` table with `getenv` and `execute`.
fn setup_os(lua: &Lua) -> mlua::Result<()> {
    let os_table: Table = lua.globals().get("os")?;

    os_table.set(
        "getenv",
        lua.create_function(|_lua, key: String| Ok(std::env::var(&key).ok()))?,
    )?;

    os_table.set(
        "remove",
        lua.create_function(|_lua, path: String| {
            std::fs::remove_file(&path).into_lua_err()?;
            Ok(true)
        })?,
    )?;

    os_table.set(
        "rename",
        lua.create_function(|_lua, (old, new): (String, String)| {
            std::fs::rename(&old, &new).into_lua_err()?;
            Ok(true)
        })?,
    )?;

    os_table.set(
        "execute",
        lua.create_function(|_lua, cmd_str: String| {
            let output = if cfg!(target_os = "windows") {
                Command::new("cmd").args(["/C", &cmd_str]).status()
            } else {
                Command::new("sh").args(["-c", &cmd_str]).status()
            };
            match output {
                Ok(status) => {
                    if status.success() {
                        // Lua 5.1 returns 0 on success
                        Ok(Value::Integer(0))
                    } else {
                        Ok(Value::Integer(status.code().unwrap_or(1) as i64))
                    }
                }
                Err(e) => Err(e.into_lua_err()),
            }
        })?,
    )?;

    Ok(())
}

/// File handle userdata for io.open
struct FileHandle {
    content: Option<String>,
    path: String,
    /// Accumulated write buffer (populated in write mode)
    write_buf: std::cell::RefCell<Option<String>>,
}

impl UserData for FileHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read", |_lua, this, mode: String| {
            if let Some(content) = &this.content {
                if mode == "*a" || mode == "*all" {
                    Ok(Some(content.clone()))
                } else if mode == "*l" || mode == "*line" {
                    Ok(content.lines().next().map(|s| s.to_string()))
                } else {
                    Ok(Some(content.clone()))
                }
            } else {
                Ok(None)
            }
        });
        methods.add_method("close", |_lua, this, ()| {
            // Flush write buffer to disk on close
            if let Some(buf) = this.write_buf.borrow().as_ref() {
                std::fs::write(&this.path, buf).into_lua_err()?;
            }
            Ok(())
        });
        methods.add_method("write", |_lua, this, data: String| {
            let mut wb = this.write_buf.borrow_mut();
            if let Some(buf) = wb.as_mut() {
                buf.push_str(&data);
            } else {
                *wb = Some(data);
            }
            Ok(())
        });
    }
}

/// Popen handle for io.popen
struct PopenHandle {
    output: String,
}

impl UserData for PopenHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read", |_lua, this, mode: String| {
            if mode == "*a" || mode == "*all" {
                Ok(Some(this.output.clone()))
            } else if mode == "*l" || mode == "*line" {
                Ok(this.output.lines().next().map(|s| s.to_string()))
            } else {
                Ok(Some(this.output.clone()))
            }
        });
        methods.add_method("close", |_lua, _this, ()| Ok(()));
    }
}

/// Stderr writer userdata
struct StderrWriter;

impl UserData for StderrWriter {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("write", |_lua, _this, data: String| {
            eprint!("{}", data);
            Ok(())
        });
    }
}

/// Create the `io` global table with `open`, `popen`, and `stderr`.
fn setup_io(lua: &Lua) -> mlua::Result<()> {
    let io_table = lua.create_table()?;

    // io.open(path, mode) -> handle, nil | nil, errmsg
    io_table.set(
        "open",
        lua.create_function(|lua, (path, mode): (String, Option<String>)| {
            let mode = mode.unwrap_or_else(|| "r".to_string());
            if mode.contains('w') {
                // Write mode: create/truncate file, buffer writes until close
                let handle = FileHandle {
                    content: None,
                    path,
                    write_buf: std::cell::RefCell::new(Some(String::new())),
                };
                Ok((Value::UserData(lua.create_userdata(handle)?), Value::Nil))
            } else if mode.contains('a') {
                // Append mode: read existing content, buffer writes until close
                let existing = std::fs::read_to_string(&path).unwrap_or_default();
                let handle = FileHandle {
                    content: None,
                    path,
                    write_buf: std::cell::RefCell::new(Some(existing)),
                };
                Ok((Value::UserData(lua.create_userdata(handle)?), Value::Nil))
            } else {
                // Read mode
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        let handle = FileHandle {
                            content: Some(content),
                            path,
                            write_buf: std::cell::RefCell::new(None),
                        };
                        Ok((Value::UserData(lua.create_userdata(handle)?), Value::Nil))
                    }
                    Err(e) => Ok((Value::Nil, Value::String(lua.create_string(e.to_string())?))),
                }
            }
        })?,
    )?;

    // io.popen(cmd) -> handle
    io_table.set(
        "popen",
        lua.create_function(|lua, cmd_str: String| {
            let output = if cfg!(target_os = "windows") {
                Command::new("cmd")
                    .args(["/C", &cmd_str])
                    .output()
                    .into_lua_err()?
            } else {
                Command::new("sh")
                    .args(["-c", &cmd_str])
                    .output()
                    .into_lua_err()?
            };
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let handle = PopenHandle { output: stdout };
            lua.create_userdata(handle)
        })?,
    )?;

    // io.stderr
    io_table.set("stderr", lua.create_userdata(StderrWriter)?)?;

    lua.globals().set("io", io_table)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_getenv() {
        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        unsafe {
            std::env::set_var("VFOX_TEST_COMPAT", "hello");
        }
        lua.load(mlua::chunk! {
            local val = os.getenv("VFOX_TEST_COMPAT")
            assert(val == "hello", "expected hello, got: " .. tostring(val))
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_os_execute() {
        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local result = os.execute("true")
            assert(result == 0, "expected 0, got: " .. tostring(result))
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_io_open() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let filepath = temp_dir.path().join("test.txt");
        let filepath_str = filepath.to_string_lossy().to_string();
        std::fs::write(&filepath, "hello world").unwrap();

        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local f = io.open($filepath_str, "r")
            assert(f ~= nil, "expected file handle")
            local content = f:read("*a")
            f:close()
            assert(content == "hello world", "expected 'hello world', got: " .. tostring(content))
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_io_open_write() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let filepath = temp_dir.path().join("write_test.txt");
        let filepath_str = filepath.to_string_lossy().to_string();

        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local f = io.open($filepath_str, "w")
            assert(f ~= nil, "expected file handle for write mode")
            f:write("hello ")
            f:write("world")
            f:close()
        })
        .exec()
        .unwrap();

        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_io_open_read_error() {
        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local f, err = io.open("/nonexistent/path/file.txt", "r")
            assert(f == nil, "expected nil for nonexistent file")
            assert(err ~= nil, "expected error message")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_io_popen() {
        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local handle = io.popen("echo hello")
            local result = handle:read("*a")
            handle:close()
            assert(result:find("hello") ~= nil, "expected hello in output, got: " .. tostring(result))
        })
        .exec()
        .unwrap();
    }
}
