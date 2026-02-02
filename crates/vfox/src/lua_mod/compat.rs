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
    setup_package(lua)?;
    Ok(())
}

/// Extend the existing Luau `os` table with `getenv` and `execute`.
fn setup_os(lua: &Lua) -> mlua::Result<()> {
    let os_table: Table = lua.globals().get("os")?;

    os_table.set(
        "getenv",
        lua.create_function(|_lua, key: String| Ok(std::env::var(&key).ok()))?,
    )?;

    // os.remove returns true on success, or (nil, errmsg) on failure (Lua 5.1 semantics)
    os_table.set(
        "remove",
        lua.create_function(|lua, path: String| match std::fs::remove_file(&path) {
            Ok(()) => Ok((Value::Boolean(true), Value::Nil)),
            Err(e) => Ok((Value::Nil, Value::String(lua.create_string(e.to_string())?))),
        })?,
    )?;

    // os.rename returns true on success, or (nil, errmsg) on failure (Lua 5.1 semantics)
    os_table.set(
        "rename",
        lua.create_function(|lua, (old, new): (String, String)| {
            match std::fs::rename(&old, &new) {
                Ok(()) => Ok((Value::Boolean(true), Value::Nil)),
                Err(e) => Ok((Value::Nil, Value::String(lua.create_string(e.to_string())?))),
            }
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
    /// Whether the file was opened in write/append mode
    writable: bool,
    /// Accumulated write buffer (populated in write mode)
    write_buf: std::cell::RefCell<Option<String>>,
    /// Current read position for line-by-line reading
    read_pos: std::cell::RefCell<usize>,
}

impl UserData for FileHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read", |_lua, this, mode: String| {
            if let Some(content) = &this.content {
                if mode == "*a" || mode == "*all" {
                    Ok(Some(content.clone()))
                } else if mode == "*l" || mode == "*line" {
                    // Line-by-line reading with position tracking
                    let mut pos = this.read_pos.borrow_mut();
                    if *pos >= content.len() {
                        return Ok(None); // EOF
                    }
                    let remaining = &content[*pos..];
                    if let Some(newline_idx) = remaining.find('\n') {
                        let line = &remaining[..newline_idx];
                        *pos += newline_idx + 1; // Skip past the newline
                        Ok(Some(line.to_string()))
                    } else if !remaining.is_empty() {
                        // Last line without trailing newline
                        *pos = content.len();
                        Ok(Some(remaining.to_string()))
                    } else {
                        Ok(None)
                    }
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
            if !this.writable {
                return Err(mlua::Error::external("attempt to write to read-only file"));
            }
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
                // Write mode: create/truncate file immediately (Lua 5.1 semantics)
                // This validates the path upfront rather than deferring errors to close()
                match std::fs::File::create(&path) {
                    Ok(_) => {
                        let handle = FileHandle {
                            content: None,
                            path,
                            writable: true,
                            write_buf: std::cell::RefCell::new(Some(String::new())),
                            read_pos: std::cell::RefCell::new(0),
                        };
                        Ok((Value::UserData(lua.create_userdata(handle)?), Value::Nil))
                    }
                    Err(e) => Ok((Value::Nil, Value::String(lua.create_string(e.to_string())?))),
                }
            } else if mode.contains('a') {
                // Append mode: read existing content, buffer writes until close
                let existing = std::fs::read_to_string(&path).unwrap_or_default();
                let handle = FileHandle {
                    content: None,
                    path,
                    writable: true,
                    write_buf: std::cell::RefCell::new(Some(existing)),
                    read_pos: std::cell::RefCell::new(0),
                };
                Ok((Value::UserData(lua.create_userdata(handle)?), Value::Nil))
            } else {
                // Read mode
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        let handle = FileHandle {
                            content: Some(content),
                            path,
                            writable: false,
                            write_buf: std::cell::RefCell::new(None),
                            read_pos: std::cell::RefCell::new(0),
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

/// Create the `package` global table with `config` for platform detection.
/// Luau doesn't have a `package` library, but some existing plugins use
/// `package.config:sub(1,1) == '\\'` to detect Windows.
fn setup_package(lua: &Lua) -> mlua::Result<()> {
    let package_table = lua.create_table()?;

    // package.config format: dir_sep\npath_sep\ntemplate_char\nexec_dir\nignore_char
    // First char is the directory separator (/ on Unix, \ on Windows)
    let config = if cfg!(target_os = "windows") {
        "\\\n;\n?\n!\n-"
    } else {
        "/\n:\n?\n!\n-"
    };
    package_table.set("config", config)?;

    lua.globals().set("package", package_table)?;
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

    #[test]
    fn test_package_config() {
        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            assert(package ~= nil, "expected package table")
            assert(package.config ~= nil, "expected package.config")
            local sep = package.config:sub(1, 1)
            -- On Unix it should be /, on Windows it should be backslash
            assert(sep == "/" or sep == "\\", "expected / or backslash, got: " .. sep)
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_os_remove_success() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let filepath = temp_dir.path().join("to_delete.txt");
        let filepath_str = filepath.to_string_lossy().to_string();
        std::fs::write(&filepath, "test").unwrap();

        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local ok, err = os.remove($filepath_str)
            assert(ok == true, "expected true on success, got: " .. tostring(ok))
            assert(err == nil, "expected nil error on success")
        })
        .exec()
        .unwrap();

        assert!(!filepath.exists());
    }

    #[test]
    fn test_os_remove_error() {
        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local ok, err = os.remove("/nonexistent/path/file.txt")
            assert(ok == nil, "expected nil on error")
            assert(err ~= nil, "expected error message")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_os_rename_success() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let old_path = temp_dir.path().join("old.txt");
        let new_path = temp_dir.path().join("new.txt");
        let old_str = old_path.to_string_lossy().to_string();
        let new_str = new_path.to_string_lossy().to_string();
        std::fs::write(&old_path, "test").unwrap();

        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local ok, err = os.rename($old_str, $new_str)
            assert(ok == true, "expected true on success, got: " .. tostring(ok))
            assert(err == nil, "expected nil error on success")
        })
        .exec()
        .unwrap();

        assert!(!old_path.exists());
        assert!(new_path.exists());
    }

    #[test]
    fn test_os_rename_error() {
        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local ok, err = os.rename("/nonexistent/path/file.txt", "/nonexistent/path/new.txt")
            assert(ok == nil, "expected nil on error")
            assert(err ~= nil, "expected error message")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_io_write_to_readonly_file() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let filepath = temp_dir.path().join("readonly.txt");
        let filepath_str = filepath.to_string_lossy().to_string();
        std::fs::write(&filepath, "original content").unwrap();

        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        // Attempting to write to a read-only file handle should error
        let result = lua
            .load(mlua::chunk! {
                local f = io.open($filepath_str, "r")
                assert(f ~= nil, "expected file handle")
                f:write("should fail")
                f:close()
            })
            .exec();
        assert!(result.is_err());

        // Verify original content is preserved
        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "original content");
    }

    #[test]
    fn test_io_read_lines() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let filepath = temp_dir.path().join("multiline.txt");
        let filepath_str = filepath.to_string_lossy().to_string();
        std::fs::write(&filepath, "line1\nline2\nline3").unwrap();

        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local f = io.open($filepath_str, "r")
            assert(f ~= nil, "expected file handle")

            local l1 = f:read("*l")
            assert(l1 == "line1", "expected 'line1', got: " .. tostring(l1))

            local l2 = f:read("*l")
            assert(l2 == "line2", "expected 'line2', got: " .. tostring(l2))

            local l3 = f:read("*l")
            assert(l3 == "line3", "expected 'line3', got: " .. tostring(l3))

            local l4 = f:read("*l")
            assert(l4 == nil, "expected nil at EOF, got: " .. tostring(l4))

            f:close()
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_io_open_write_error() {
        let lua = Lua::new();
        mod_compat(&lua).unwrap();
        lua.load(mlua::chunk! {
            local f, err = io.open("/nonexistent/path/file.txt", "w")
            assert(f == nil, "expected nil for invalid write path")
            assert(err ~= nil, "expected error message for invalid write path")
        })
        .exec()
        .unwrap();
    }
}
