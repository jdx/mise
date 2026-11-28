//! `io.popen` for hooks that run detached from the user's working directory or
//! with an environment mise built for them.
//!
//! Stock `io.popen` starts its shell in mise's own working directory, which is
//! usually a project the user may not have trusted. A `git` there obeys the
//! project's repository config (`core.sshCommand` and `core.fsmonitor` are shell
//! commands), so a hook that only asks a remote for its tags would run the
//! project's commands. Hooks that have nothing to do with the project get a
//! detached directory (see `Plugin::detach_cmds_from_cwd`), and this replacement
//! starts their `io.popen` children there, like `os.execute` and `cmd.exec`.
//!
//! Stock `io.popen` also hands its children mise's own environment. A hook
//! that mise gives an environment (registry `mise_env`), as it does every
//! environment hook, runs its `io.popen` children with that environment
//! instead, as `os.execute` and `cmd.exec` do: without the project directories
//! mise withholds from the hook, and with the variables mise adds for it.

use mlua::prelude::*;
use mlua::{AnyUserData, Function, Table, UserData, UserDataMethods, Variadic};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

use super::cmd::{apply_mise_env, cmd_shell, command_from_shell, detached_cwd, start_in};

/// Replace `io.popen` so it starts children in the detached directory when the
/// hook has one and with the hook's environment when mise built one, and
/// behaves as stock `io.popen` otherwise.
pub(crate) fn mod_popen(lua: &Lua) -> LuaResult<()> {
    let Some(io) = lua.globals().get::<Option<Table>>("io")? else {
        return Ok(());
    };
    let stock: Option<Function> = io.get("popen")?;
    let popen = lua.create_function(
        move |lua, (command, mode): (String, Option<String>)| -> LuaResult<LuaMultiValue> {
            let cwd = detached_cwd(lua)?;
            let has_mise_env = lua
                .named_registry_value::<Option<Table>>("mise_env")?
                .is_some();
            if cwd.is_some() || has_mise_env {
                return popen(lua, &command, mode.as_deref(), cwd.as_deref());
            }
            match &stock {
                Some(stock) => stock.call((command, mode)),
                None => Err(LuaError::RuntimeError(
                    "io.popen is not available".to_string(),
                )),
            }
        },
    )?;
    io.set("popen", popen)?;
    // `io.type` tells a stock file from a closed one and returns nil for other
    // values; it answers the same for the handles above.
    let stock_type: Option<Function> = io.get("type")?;
    let io_type = lua.create_function(move |lua, value: LuaValue| -> LuaResult<LuaValue> {
        if let LuaValue::UserData(ud) = &value
            && let Ok(file) = ud.borrow::<PopenFile>()
        {
            let kind = if file.closed { "closed file" } else { "file" };
            return lua.create_string(kind).map(LuaValue::String);
        }
        match &stock_type {
            Some(stock) => stock.call(value),
            None => Ok(LuaNil),
        }
    })?;
    io.set("type", io_type)?;
    // `io.close(handle)` closes these handles as `handle:close()` does.
    let stock_close: Option<Function> = io.get("close")?;
    let io_close =
        lua.create_function(move |_, args: LuaMultiValue| -> LuaResult<LuaMultiValue> {
            if let Some(LuaValue::UserData(ud)) = args.front()
                && let Ok(mut file) = ud.borrow_mut::<PopenFile>()
            {
                file.open()?.close();
                return Ok(LuaMultiValue::from_iter([LuaValue::Boolean(true)]));
            }
            match &stock_close {
                Some(stock) => stock.call(args),
                None => Err(LuaError::RuntimeError(
                    "io.close is not available".to_string(),
                )),
            }
        })?;
    io.set("close", io_close)
}

fn popen(
    lua: &Lua,
    command: &str,
    mode: Option<&str>,
    cwd: Option<&str>,
) -> LuaResult<LuaMultiValue> {
    let (write, binary) = match mode.unwrap_or("r") {
        "r" => (false, false),
        "rb" => (false, true),
        "w" => (true, false),
        "wb" => (true, true),
        other => {
            return Err(LuaError::RuntimeError(format!(
                "io.popen: invalid mode {other:?}, expected \"r\" or \"w\""
            )));
        }
    };
    let mut cmd = command_from_shell(&cmd_shell(lua)?, command)?;
    apply_mise_env(lua, &mut cmd)?;
    if let Some(cwd) = cwd {
        start_in(&mut cmd, cwd);
    }
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
        stdout: child
            .stdout
            .take()
            .map(|stdout| text_reader(stdout, binary)),
        text_writes: cfg!(windows) && !binary,
        child: Some(child),
        closed: false,
    };
    lua.create_userdata(file)?.into_lua_multi(lua)
}

/// The handle `io.popen` returns: the child's stdout to read in `"r"` mode, or its
/// stdin to write in `"w"` mode. Closing it, or dropping it, waits for the child.
///
/// It answers as a Lua 5.1 popen file does, the version mlua is built with: `close`
/// returns `true` once the child is reaped, whatever its exit status (5.1's
/// `pclose` reports no status), and any use of a closed handle, a second `close`
/// included, raises "attempt to use a closed file".
struct PopenFile {
    stdin: Option<ChildStdin>,
    stdout: Option<Box<dyn BufRead + Send + Sync>>,
    /// Whether writes go through [`lf_as_crlf`]: a pipe opened with `"w"`, not
    /// `"wb"`, on Windows.
    text_writes: bool,
    child: Option<Child>,
    closed: bool,
}

/// `bytes` as the Windows C runtime writes them to a text-mode stream: every
/// `\n` goes out as `\r\n`, one that already follows a `\r` included.
fn lf_as_crlf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &byte in bytes {
        if byte == b'\n' {
            out.push(b'\r');
        }
        out.push(byte);
    }
    out
}

/// The child's stdout as stock `io.popen` reads it. On Windows a pipe opened
/// without `"b"` is in text mode, where `\r\n` reads as `\n` before any read
/// counts or splits the bytes.
fn text_reader(stdout: ChildStdout, binary: bool) -> Box<dyn BufRead + Send + Sync> {
    if cfg!(windows) && !binary {
        Box::new(CrlfAsLf::new(stdout))
    } else {
        Box::new(BufReader::new(stdout))
    }
}

/// Reads `\r\n` as `\n`, as the Windows C runtime reads a text-mode stream. A
/// `\r` that ends one read of the inner reader waits for the next one.
struct CrlfAsLf<R> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    held_cr: bool,
}

impl<R> CrlfAsLf<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            buf: Vec::new(),
            pos: 0,
            held_cr: false,
        }
    }
}

impl<R: Read> BufRead for CrlfAsLf<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        while self.pos == self.buf.len() {
            let mut raw = [0; 8192];
            let n = self.inner.read(&mut raw)?;
            self.buf.clear();
            self.pos = 0;
            if n == 0 {
                if std::mem::take(&mut self.held_cr) {
                    self.buf.push(b'\r');
                }
                break;
            }
            for &byte in &raw[..n] {
                if std::mem::take(&mut self.held_cr) && byte != b'\n' {
                    self.buf.push(b'\r');
                }
                if byte == b'\r' {
                    self.held_cr = true;
                } else {
                    self.buf.push(byte);
                }
            }
        }
        Ok(&self.buf[self.pos..])
    }

    fn consume(&mut self, amt: usize) {
        self.pos = (self.pos + amt).min(self.buf.len());
    }
}

impl<R: Read> Read for CrlfAsLf<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let buf = self.fill_buf()?;
        let n = buf.len().min(out.len());
        out[..n].copy_from_slice(&buf[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl PopenFile {
    fn open(&mut self) -> LuaResult<&mut Self> {
        if self.closed {
            return Err(LuaError::RuntimeError(
                "attempt to use a closed file".to_string(),
            ));
        }
        Ok(self)
    }

    fn reader(&mut self) -> LuaResult<&mut Box<dyn BufRead + Send + Sync>> {
        self.open()?
            .stdout
            .as_mut()
            .ok_or_else(|| LuaError::RuntimeError("file is not readable".to_string()))
    }

    fn close(&mut self) {
        self.closed = true;
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
        }
        Ok(Some(line))
    }

    fn read_all(&mut self) -> LuaResult<Vec<u8>> {
        let mut all = Vec::new();
        self.reader()?.read_to_end(&mut all)?;
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

    /// Reads a number as Lua 5.1's `fscanf("%lf")` does: skips whitespace, then
    /// consumes only the longest prefix that can start a decimal number (a sign,
    /// digits, one `.`, an exponent), leaving whatever follows unread.
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
        let (mut seen_digit, mut seen_dot, mut seen_exp) = (false, false, false);
        while let Some(&byte) = reader.fill_buf()?.first() {
            let accept = match byte {
                b'0'..=b'9' => {
                    seen_digit = true;
                    true
                }
                b'+' | b'-' => token.is_empty() || matches!(token.last(), Some(b'e' | b'E')),
                b'.' if !seen_dot && !seen_exp => {
                    seen_dot = true;
                    true
                }
                b'e' | b'E' if seen_digit && !seen_exp => {
                    seen_exp = true;
                    true
                }
                _ => false,
            };
            if !accept {
                break;
            }
            token.push(byte);
            reader.consume(1);
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
            // Stock Lua rejects a closed file here, not on the first call.
            file.borrow_mut::<PopenFile>()?.open()?;
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
                    let this = this.open()?;
                    let text_writes = this.text_writes;
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
                        if text_writes {
                            stdin.write_all(&lf_as_crlf(&bytes))?;
                        } else {
                            stdin.write_all(&bytes)?;
                        }
                    }
                }
                Ok(file)
            },
        );
        methods.add_method_mut("flush", |_, this, ()| {
            if let Some(stdin) = this.open()?.stdin.as_mut() {
                stdin.flush()?;
            }
            Ok(true)
        });
        methods.add_method_mut("close", |_, this, ()| {
            this.open()?.close();
            Ok(true)
        });
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::CrlfAsLf;
    use crate::lua_mod::cmd;
    use mlua::Lua;
    use std::io::{BufRead, Read};
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

    /// Runs the same handle checks against stock `io.popen` and the detached one.
    fn close_and_type_results(lua: &Lua) -> Vec<String> {
        lua.load(
            r##"
            local seen = {}
            local function note(...)
                local parts = {}
                for i = 1, select("#", ...) do
                    parts[#parts + 1] = tostring((select(i, ...)))
                end
                seen[#seen + 1] = table.concat(parts, ",")
            end
            local function closed_error(f)
                local ok, err = pcall(f)
                return ok, string.find(tostring(err), "attempt to use a closed file", 1, true) ~= nil
            end
            for _, command in ipairs({ "true", "exit 3", "kill -9 $$" }) do
                local h = io.popen(command)
                note(command, io.type(h), h:read("*a"))
                note(h:close())
                note(io.type(h))
                note(closed_error(function() return h:close() end))
                note(closed_error(function() return h:read("*a") end))
            end
            local w = io.popen("cat >/dev/null", "w")
            note(io.type(w), w:close(), io.type(w))
            note(closed_error(function() return w:write("x") end))
            local c = io.popen("exit 5")
            note(io.close(c), io.type(c))
            note(closed_error(function() return io.close(c) end))
            -- lines() on a closed handle fails at once, not on the first call
            local l = io.popen("true")
            l:close()
            note(closed_error(function() return l:lines() end))
            note(io.type("not a file"), io.type(io.stdout))
            -- a handle dropped without close is reaped by the collector
            io.popen("exit 4")
            collectgarbage()
            collectgarbage()
            return seen
            "##,
        )
        .eval()
        .unwrap()
    }

    #[test]
    fn popen_handles_answer_like_stock_ones() {
        let stock = Lua::new();
        let expected = close_and_type_results(&stock);
        let dir = tempfile::tempdir().unwrap();
        let detached = detached_lua(dir.path());
        assert_eq!(close_and_type_results(&detached), expected);
        // Lua 5.1's pclose reports no exit status: a failed or killed child
        // still closes to true.
        assert_eq!(expected[1], "true");
        assert_eq!(expected[6], "true");
        assert_eq!(expected[11], "true");
        assert_eq!(expected[3], "false,true");
        assert_eq!(expected[2], "closed file");
    }

    /// What `read("*n")` returns, then what is left unread, for each output. An
    /// exponent with no digits (`4e`) is left out: stock Lua leaves it to the C
    /// library's `fscanf`, which reads it as `4` on glibc and as nothing elsewhere.
    fn number_reads(lua: &Lua) -> Vec<String> {
        lua.load(
            r#"
            local seen = {}
            for _, out in ipairs({ "12foo", "  -3.5e2x", "0.5.5", "abc", "+7 8", "1e+2-3" }) do
                local h = io.popen("printf '%s' '" .. out .. "'")
                local n = h:read("*n")
                seen[#seen + 1] = tostring(n) .. "|" .. h:read("*a")
                h:close()
            end
            return seen
            "#,
        )
        .eval()
        .unwrap()
    }

    /// Hands out one byte per read, so a `\r\n` is split between reads.
    struct ByteAtATime<'a>(&'a [u8]);

    impl Read for ByteAtATime<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let Some((&first, rest)) = self.0.split_first() else {
                return Ok(0);
            };
            if out.is_empty() {
                return Ok(0);
            }
            out[0] = first;
            self.0 = rest;
            Ok(1)
        }
    }

    #[test]
    fn text_mode_reads_crlf_as_lf_before_counting() {
        let raw = b"x\r\ny\r\r\nz\rw\r";
        let mut all = Vec::new();
        CrlfAsLf::new(ByteAtATime(raw))
            .read_to_end(&mut all)
            .unwrap();
        assert_eq!(all, b"x\ny\r\nz\rw\r");

        let mut reader = CrlfAsLf::new(ByteAtATime(raw));
        let mut three = Vec::new();
        (&mut reader).take(3).read_to_end(&mut three).unwrap();
        assert_eq!(three, b"x\ny");
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line).unwrap();
        assert_eq!(line, b"\r\n");
    }

    #[test]
    fn popen_reads_a_number_like_stock_popen() {
        let expected = number_reads(&Lua::new());
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(number_reads(&detached_lua(dir.path())), expected);
        assert_eq!(expected[0], "12|foo");
    }

    /// Sets the environment mise builds for a hook, as `Plugin::set_cmd_env` does.
    fn set_mise_env(lua: &Lua, vars: &[(&str, &str)]) {
        let env = lua.create_table().unwrap();
        for (key, value) in vars {
            env.set(*key, *value).unwrap();
        }
        lua.set_named_registry_value("mise_env", env).unwrap();
    }

    #[test]
    fn popen_children_get_the_hook_environment() {
        let path = std::env::var("PATH").unwrap();
        let home = std::env::var("HOME").unwrap();
        let run = |lua: &Lua| -> (String, String) {
            lua.load(
                r#"
                local h = io.popen("printf '%s|%s' \"${HOOK_VAR-unset}\" \"${HOME-unset}\"")
                local seen = h:read("*a")
                h:close()
                local p = io.popen("pwd")
                local cwd = p:read("*l")
                p:close()
                return seen, cwd
                "#,
            )
            .eval()
            .unwrap()
        };

        let attached = Lua::new();
        cmd(&attached).unwrap();
        set_mise_env(&attached, &[("PATH", &path), ("HOOK_VAR", "set")]);
        let (seen, cwd) = run(&attached);
        assert_eq!(seen, "set|unset");
        assert_eq!(
            canon(&cwd),
            std::env::current_dir().unwrap().canonicalize().unwrap()
        );

        let dir = tempfile::tempdir().unwrap();
        let detached = detached_lua(dir.path());
        set_mise_env(&detached, &[("PATH", &path), ("HOOK_VAR", "set")]);
        let (seen, cwd) = run(&detached);
        assert_eq!(seen, "set|unset");
        assert_eq!(canon(&cwd), dir.path().canonicalize().unwrap());

        // with no hook environment, children inherit mise's own
        let stock = Lua::new();
        cmd(&stock).unwrap();
        let (seen, _) = run(&stock);
        assert_eq!(seen, format!("unset|{home}"));
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

    #[test]
    fn a_hook_cwd_option_moves_pwd_with_it() {
        let detached = tempfile::tempdir().unwrap();
        let chosen = tempfile::tempdir().unwrap();
        let lua = detached_lua(detached.path());
        // `printenv` instead of a shell, which would repair a stale PWD itself
        let shell = lua.create_sequence_from(["printenv"]).unwrap();
        lua.set_named_registry_value("mise_cmd_shell", shell)
            .unwrap();
        let chosen_str = chosen.path().to_string_lossy().to_string();
        let (default_pwd, chosen_pwd): (String, String) = lua
            .load(mlua::chunk! {
                local cmd = require("cmd")
                return cmd.exec("PWD"), cmd.exec("PWD", { cwd = $chosen_str })
            })
            .eval()
            .unwrap();
        assert_eq!(default_pwd.trim(), detached.path().to_string_lossy());
        assert_eq!(chosen_pwd.trim(), chosen.path().to_string_lossy());
    }
}

#[cfg(test)]
mod text_mode_tests {
    use super::lf_as_crlf;

    #[test]
    fn text_mode_writes_every_lf_as_crlf() {
        assert_eq!(lf_as_crlf(b""), b"");
        assert_eq!(lf_as_crlf(b"no newline"), b"no newline");
        assert_eq!(lf_as_crlf(b"a\nb\n"), b"a\r\nb\r\n");
        assert_eq!(lf_as_crlf(b"\n\n"), b"\r\n\r\n");
        assert_eq!(lf_as_crlf(b"x\r\ny\rz"), b"x\r\r\ny\rz");
    }
}
