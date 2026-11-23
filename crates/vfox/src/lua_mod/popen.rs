//! `io.popen` for hooks that run detached from the user's working directory.
//!
//! Stock `io.popen` starts its shell in mise's own working directory, which is
//! usually a project the user may not have trusted. A `git` there obeys the
//! project's repository config (`core.sshCommand` and `core.fsmonitor` are shell
//! commands), so a hook that only asks a remote for its tags would run the
//! project's commands. Hooks that have nothing to do with the project get a
//! detached directory (see `Plugin::detach_cmds_from_cwd`), and this replacement
//! starts their `io.popen` children there, like `os.execute` and `cmd.exec`.

use mlua::prelude::*;
use mlua::{AnyUserData, Function, Table, UserData, UserDataMethods, Variadic};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

use super::cmd::{cmd_shell, command_from_shell, detached_cwd, start_in};

/// Replace `io.popen` so it starts children in the detached directory when the
/// hook has one, and behaves as stock `io.popen` otherwise.
pub(crate) fn mod_popen(lua: &Lua) -> LuaResult<()> {
    let Some(io) = lua.globals().get::<Option<Table>>("io")? else {
        return Ok(());
    };
    let stock: Option<Function> = io.get("popen")?;
    let popen = lua.create_function(
        move |lua, (command, mode): (String, Option<String>)| -> LuaResult<LuaMultiValue> {
            match detached_cwd(lua)? {
                Some(cwd) => popen(lua, &command, mode.as_deref(), &cwd),
                None => match &stock {
                    Some(stock) => stock.call((command, mode)),
                    None => Err(LuaError::RuntimeError(
                        "io.popen is not available".to_string(),
                    )),
                },
            }
        },
    )?;
    io.set("popen", popen)
}

fn popen(lua: &Lua, command: &str, mode: Option<&str>, cwd: &str) -> LuaResult<LuaMultiValue> {
    let write = match mode.unwrap_or("r") {
        "r" | "rb" => false,
        "w" | "wb" => true,
        other => {
            return Err(LuaError::RuntimeError(format!(
                "io.popen: invalid mode {other:?}, expected \"r\" or \"w\""
            )));
        }
    };
    let mut cmd = command_from_shell(&cmd_shell(lua)?, command)?;
    start_in(&mut cmd, cwd);
    if write {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdout(Stdio::piped());
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return (LuaNil, format!("{command}: {err}")).into_lua_multi(lua),
    };
    let file = PopenFile {
        stdin: child.stdin.take(),
        stdout: child.stdout.take().map(BufReader::new),
        child: Some(child),
    };
    lua.create_userdata(file)?.into_lua_multi(lua)
}

/// The handle `io.popen` returns: the child's stdout to read in `"r"` mode, or its
/// stdin to write in `"w"` mode. Closing it, or dropping it, waits for the child.
struct PopenFile {
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    child: Option<Child>,
}

impl PopenFile {
    fn reader(&mut self) -> LuaResult<&mut BufReader<ChildStdout>> {
        self.stdout
            .as_mut()
            .ok_or_else(|| LuaError::RuntimeError("file is not readable".to_string()))
    }

    fn close(&mut self) {
        // Closing the pipes first lets a child blocked on a full pipe see EOF or
        // EPIPE and exit, as `pclose` does.
        self.stdin = None;
        self.stdout = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }

    fn read_line(&mut self, keep_newline: bool) -> LuaResult<Option<Vec<u8>>> {
        let mut line = Vec::new();
        if self.reader()?.read_until(b'\n', &mut line)? == 0 {
            return Ok(None);
        }
        if !keep_newline && line.last() == Some(&b'\n') {
            line.pop();
            // Stock `io.popen` opens the pipe in text mode on Windows.
            if cfg!(windows) && line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }

    fn read_all(&mut self) -> LuaResult<Vec<u8>> {
        let mut all = Vec::new();
        self.reader()?.read_to_end(&mut all)?;
        if cfg!(windows) {
            let mut text = Vec::with_capacity(all.len());
            let mut bytes = all.iter().peekable();
            while let Some(&byte) = bytes.next() {
                if byte != b'\r' || bytes.peek() != Some(&&b'\n') {
                    text.push(byte);
                }
            }
            all = text;
        }
        Ok(all)
    }

    fn read_bytes(&mut self, count: usize) -> LuaResult<Option<Vec<u8>>> {
        let mut buf = Vec::new();
        self.reader()?.take(count as u64).read_to_end(&mut buf)?;
        if buf.is_empty() && count > 0 {
            return Ok(None);
        }
        if count == 0 && self.reader()?.fill_buf()?.is_empty() {
            return Ok(None);
        }
        Ok(Some(buf))
    }

    fn read_number(&mut self) -> LuaResult<Option<f64>> {
        let reader = self.reader()?;
        loop {
            let buf = reader.fill_buf()?;
            let Some(&byte) = buf.first() else {
                return Ok(None);
            };
            if !byte.is_ascii_whitespace() {
                break;
            }
            reader.consume(1);
        }
        let mut token = Vec::new();
        loop {
            let buf = reader.fill_buf()?;
            match buf.first() {
                Some(&byte) if byte.is_ascii_alphanumeric() || b"+-.".contains(&byte) => {
                    token.push(byte);
                    reader.consume(1);
                }
                _ => break,
            }
        }
        Ok(std::str::from_utf8(&token)
            .ok()
            .and_then(|token| token.parse().ok()))
    }

    fn read(&mut self, lua: &Lua, formats: LuaMultiValue) -> LuaResult<LuaMultiValue> {
        let formats: Vec<LuaValue> = if formats.is_empty() {
            vec![LuaValue::String(lua.create_string("l")?)]
        } else {
            formats.into_iter().collect()
        };
        let mut results = Vec::with_capacity(formats.len());
        for format in formats {
            let value = match format {
                LuaValue::Integer(count) => self
                    .read_bytes(usize::try_from(count).unwrap_or(0))?
                    .map(|bytes| lua.create_string(bytes))
                    .transpose()?
                    .map_or(LuaNil, LuaValue::String),
                LuaValue::Number(count) => self
                    .read_bytes(count.max(0.0) as usize)?
                    .map(|bytes| lua.create_string(bytes))
                    .transpose()?
                    .map_or(LuaNil, LuaValue::String),
                LuaValue::String(format) => {
                    let format = format.to_str()?.to_string();
                    match format.trim_start_matches('*').chars().next() {
                        Some('a') => LuaValue::String(lua.create_string(self.read_all()?)?),
                        Some('l') => self
                            .read_line(false)?
                            .map(|line| lua.create_string(line))
                            .transpose()?
                            .map_or(LuaNil, LuaValue::String),
                        Some('L') => self
                            .read_line(true)?
                            .map(|line| lua.create_string(line))
                            .transpose()?
                            .map_or(LuaNil, LuaValue::String),
                        Some('n') => self.read_number()?.map_or(LuaNil, LuaValue::Number),
                        _ => {
                            return Err(LuaError::RuntimeError(format!(
                                "bad argument to 'read' (invalid format {format:?})"
                            )));
                        }
                    }
                }
                other => {
                    return Err(LuaError::RuntimeError(format!(
                        "bad argument to 'read' (invalid format of type {})",
                        other.type_name()
                    )));
                }
            };
            let done = value.is_nil();
            results.push(value);
            if done {
                break;
            }
        }
        Ok(LuaMultiValue::from_iter(results))
    }
}

impl Drop for PopenFile {
    fn drop(&mut self) {
        self.close();
    }
}

impl UserData for PopenFile {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("read", |lua, this, formats: LuaMultiValue| {
            this.read(lua, formats)
        });
        methods.add_function("lines", |lua, file: AnyUserData| {
            lua.create_function_mut(move |lua, ()| {
                let mut this = file.borrow_mut::<PopenFile>()?;
                this.read_line(false)?
                    .map(|line| lua.create_string(line))
                    .transpose()
            })
        });
        methods.add_function(
            "write",
            |_, (file, values): (AnyUserData, Variadic<LuaValue>)| {
                {
                    let mut this = file.borrow_mut::<PopenFile>()?;
                    let stdin = this.stdin.as_mut().ok_or_else(|| {
                        LuaError::RuntimeError("file is not writable".to_string())
                    })?;
                    for value in values {
                        let bytes = match value {
                            LuaValue::String(s) => s.as_bytes().to_vec(),
                            LuaValue::Integer(n) => n.to_string().into_bytes(),
                            LuaValue::Number(n) => n.to_string().into_bytes(),
                            other => {
                                return Err(LuaError::RuntimeError(format!(
                                    "bad argument to 'write' (string expected, got {})",
                                    other.type_name()
                                )));
                            }
                        };
                        stdin.write_all(&bytes)?;
                    }
                }
                Ok(file)
            },
        );
        methods.add_method_mut("flush", |_, this, ()| {
            if let Some(stdin) = this.stdin.as_mut() {
                stdin.flush()?;
            }
            Ok(true)
        });
        methods.add_method_mut("close", |_, this, ()| {
            this.close();
            Ok(true)
        });
    }
}

#[cfg(all(test, unix))]
mod tests {
    use crate::lua_mod::cmd;
    use mlua::Lua;
    use std::path::Path;

    fn detached_lua(dir: &Path) -> Lua {
        let lua = Lua::new();
        cmd(&lua).unwrap();
        lua.set_named_registry_value("mise_cmd_cwd", dir.to_string_lossy().to_string())
            .unwrap();
        lua
    }

    fn canon(path: &str) -> std::path::PathBuf {
        Path::new(path.trim()).canonicalize().unwrap()
    }

    #[test]
    fn popen_starts_in_the_detached_directory() {
        let dir = tempfile::tempdir().unwrap();
        let lua = detached_lua(dir.path());
        let seen: String = lua
            .load(r#"local h = io.popen("pwd"); local cwd = h:read("*l"); h:close(); return cwd"#)
            .eval()
            .unwrap();
        assert_eq!(canon(&seen), dir.path().canonicalize().unwrap());
    }

    #[test]
    fn popen_reads_like_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let lua = detached_lua(dir.path());
        lua.load(
            r#"
            local h = io.popen("printf 'one\ntwo\n 42 rest\nfour'")
            assert(h:read("*l") == "one")
            assert(h:read("L") == "two\n")
            assert(h:read("*n") == 42)
            assert(h:read(5) == " rest")
            assert(h:read("*a") == "\nfour")
            assert(h:read("*l") == nil)
            assert(h:read("*a") == "")
            assert(h:close() == true)

            local lines = {}
            for line in io.popen("printf 'a\nb\n'"):lines() do
                table.insert(lines, line)
            end
            assert(#lines == 2 and lines[1] == "a" and lines[2] == "b")
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn popen_writes_to_the_child() {
        let dir = tempfile::tempdir().unwrap();
        let lua = detached_lua(dir.path());
        lua.load(
            r#"
            local h = io.popen("cat > written", "w")
            h:write("hello ", 1, "\n"):write("world\n")
            assert(h:close() == true)
            "#,
        )
        .exec()
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("written")).unwrap(),
            "hello 1\nworld\n"
        );
    }

    #[test]
    fn popen_without_a_detached_directory_is_stock() {
        let lua = Lua::new();
        cmd(&lua).unwrap();
        let seen: String = lua
            .load(r#"local h = io.popen("pwd"); local cwd = h:read("*a"); h:close(); return cwd"#)
            .eval()
            .unwrap();
        assert_eq!(
            canon(&seen),
            std::env::current_dir().unwrap().canonicalize().unwrap()
        );
        let kind: String = lua
            .load(r#"return io.type(io.popen("true"))"#)
            .eval()
            .unwrap();
        assert_eq!(kind, "file");
    }

    #[test]
    fn a_hook_cwd_option_overrides_the_detached_directory() {
        let detached = tempfile::tempdir().unwrap();
        let chosen = tempfile::tempdir().unwrap();
        let lua = detached_lua(detached.path());
        let chosen_str = chosen.path().to_string_lossy().to_string();
        let (default_cwd, chosen_cwd): (String, String) = lua
            .load(mlua::chunk! {
                local cmd = require("cmd")
                return cmd.exec("pwd"), cmd.exec("pwd", { cwd = $chosen_str })
            })
            .eval()
            .unwrap();
        assert_eq!(canon(&default_cwd), detached.path().canonicalize().unwrap());
        assert_eq!(canon(&chosen_cwd), chosen.path().canonicalize().unwrap());
    }
}
