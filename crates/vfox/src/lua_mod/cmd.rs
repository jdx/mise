use mlua::Table;
use mlua::prelude::*;
use std::path::Path;
use std::process::Command;

pub fn mod_cmd(lua: &Lua) -> LuaResult<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    let cmd = lua.create_table_from(vec![("exec", lua.create_function(exec)?)])?;
    loaded.set("cmd", cmd.clone())?;
    loaded.set("vfox.cmd", cmd)?;

    // Route Lua's `os.execute` through mise's sanitized env (registry `mise_env`),
    // matching cmd.exec. Stock `os.execute` inherits mise's raw process env, which
    // during a combined `mise install` can carry stale `tools = true` values
    // (e.g. a `CLOUDSDK_PYTHON` rendered before its python dependency was
    // installed). Plugins that shell out via `os.execute` (e.g. vfox-gcloud's
    // install.sh) would otherwise use the stale value and fail. When `mise_env`
    // is unset, `os.execute` behaves exactly as stock. (#10282)
    //
    // Reuse the existing `os` table so `os.time`/`os.date`/etc. are preserved; only
    // create one if `os` is absent (`nil`). A non-table `os` propagates the error
    // rather than being silently overwritten.
    let globals = lua.globals();
    let os: Table = match globals.get::<Option<Table>>("os")? {
        Some(os) => os,
        None => {
            let os = lua.create_table()?;
            globals.set("os", os.clone())?;
            os
        }
    };
    os.set("execute", lua.create_function(os_execute)?)?;
    Ok(())
}

fn exec(lua: &Lua, args: mlua::MultiValue) -> LuaResult<String> {
    let (command, options) = match args.len() {
        1 => {
            let command: String = args.into_iter().next().unwrap().to_string()?;
            (command, None)
        }
        2 => {
            let mut iter = args.into_iter();
            let command: String = iter.next().unwrap().to_string()?;
            let options: Table = iter.next().unwrap().as_table().unwrap().clone();
            (command, Some(options))
        }
        _ => {
            return Err(mlua::Error::RuntimeError(
                "cmd.exec takes 1 or 2 arguments: (command) or (command, options)".to_string(),
            ));
        }
    };

    let shell = cmd_shell(lua)?;
    let mut cmd = command_from_shell(&shell, &command)?;

    // Apply mise-constructed environment if available in Lua registry.
    // This ensures mise-managed tools are on PATH when called from env module hooks.
    let has_mise_env = apply_mise_env(lua, &mut cmd)?;
    debug!("[cmd.exec] command={command:?} shell={shell:?} has_mise_env={has_mise_env}");

    // Apply options if provided (explicit env vars override mise env)
    if let Some(options) = options {
        // Set working directory if specified
        if let Ok(cwd) = options.get::<String>("cwd") {
            cmd.current_dir(Path::new(&cwd));
        }

        // Set environment variables if specified
        if let Ok(env) = options.get::<Table>("env") {
            for pair in env.pairs::<String, String>() {
                let (key, value) = pair?;
                cmd.env(key, value);
            }
        }

        // Set timeout if specified (future feature)
        if let Ok(_timeout) = options.get::<u64>("timeout") {
            // TODO: Implement timeout functionality
            // For now, just ignore the timeout option
        }
    }

    let output = cmd
        .output()
        .map_err(|e| mlua::Error::RuntimeError(format!("Failed to execute command: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        Ok(stdout.to_string())
    } else {
        Err(mlua::Error::RuntimeError(format!(
            "Command failed with status {}: {}",
            output.status, stderr
        )))
    }
}

/// Apply the mise-constructed environment (Lua registry `mise_env`) to `cmd`,
/// clearing the inherited environment first so the child sees exactly the env
/// mise built. Returns true if it was applied; when `mise_env` is absent the
/// command's inherited environment is left untouched (stock behavior).
fn apply_mise_env(lua: &Lua, cmd: &mut Command) -> LuaResult<bool> {
    if let Ok(mise_env) = lua.named_registry_value::<Table>("mise_env") {
        cmd.env_clear();
        for pair in mise_env.pairs::<String, String>() {
            let (key, value) = pair?;
            cmd.env(key, value);
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Drop-in replacement for Lua's `os.execute` that applies mise's sanitized env
/// (see [`apply_mise_env`]) and runs through the same shell as cmd.exec, while
/// keeping `os.execute`'s streaming stdio (output goes to the terminal rather
/// than being captured). Returns the process exit code (Lua 5.1 convention:
/// `0` on success); `os.execute()` with no argument reports shell availability.
/// (#10282)
fn os_execute(lua: &Lua, command: Option<String>) -> LuaResult<i64> {
    let Some(command) = command else {
        // `os.execute()` with no argument: report that a shell is available.
        return Ok(1);
    };
    let shell = cmd_shell(lua)?;
    let mut cmd = command_from_shell(&shell, &command)?;
    let has_mise_env = apply_mise_env(lua, &mut cmd)?;
    debug!("[os.execute] command={command:?} shell={shell:?} has_mise_env={has_mise_env}");
    let status = cmd
        .status()
        .map_err(|e| mlua::Error::RuntimeError(format!("Failed to execute command: {e}")))?;
    Ok(status.code().unwrap_or(-1) as i64)
}

fn cmd_shell(lua: &Lua) -> LuaResult<Vec<String>> {
    if let Ok(shell) = lua.named_registry_value::<Table>("mise_cmd_shell") {
        return shell.sequence_values::<String>().collect();
    }
    Ok(default_cmd_shell())
}

fn default_cmd_shell() -> Vec<String> {
    if cfg!(target_os = "windows") {
        vec!["cmd".to_string(), "/C".to_string()]
    } else {
        vec!["sh".to_string(), "-c".to_string()]
    }
}

fn command_from_shell(shell: &[String], command: &str) -> LuaResult<Command> {
    let (program, args) = shell.split_first().ok_or_else(|| {
        mlua::Error::RuntimeError("cmd.exec shell command cannot be empty".to_string())
    })?;
    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.arg(command);
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd() {
        let lua = Lua::new();
        mod_cmd(&lua).unwrap();
        lua.load(mlua::chunk! {
            local cmd = require("cmd")
            local result = cmd.exec("echo hello world")
            assert(result == "hello world\n")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_cmd_with_cwd() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let temp_path = temp_dir.path();
        // Canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
        let temp_path_canonical = temp_path
            .canonicalize()
            .unwrap_or_else(|_| temp_path.to_path_buf());
        let temp_dir_str = temp_path_canonical.to_string_lossy().to_string();
        let expected_path = temp_dir_str.trim_end_matches('/').to_string();
        let lua = Lua::new();
        mod_cmd(&lua).unwrap();
        lua.load(mlua::chunk! {
            local cmd = require("cmd")
            -- Test with working directory
            local result = cmd.exec("pwd", {cwd = $temp_dir_str})
            -- Check that result contains the expected path (handles trailing slashes/newlines)
            assert(result:find($expected_path) ~= nil, "Expected result to contain: " .. $expected_path .. " but got: " .. result)
        })
        .exec()
        .unwrap();
        // TempDir automatically cleans up when dropped
    }

    #[test]
    fn test_cmd_with_env() {
        let lua = Lua::new();
        mod_cmd(&lua).unwrap();
        lua.load(mlua::chunk! {
            local cmd = require("cmd")
            -- Test with environment variables
            local result = cmd.exec("echo $TEST_VAR", {env = {TEST_VAR = "hello"}})
            assert(result:find("hello") ~= nil)
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_cmd_windows_compatibility() {
        let lua = Lua::new();
        mod_cmd(&lua).unwrap();

        let test_command = "echo hello world";

        lua.load(format!(
            r#"
            local cmd = require("cmd")
            local result = cmd.exec("{test_command}")
            assert(result:find("hello world") ~= nil)
        "#
        ))
        .exec()
        .unwrap();
    }

    // os.execute must honor the mise_env registry (env_clear + mise_env) so
    // plugins shelling out via os.execute get mise's sanitized env, not the raw
    // process env (which may carry stale tools=true values). (#10282)
    #[test]
    #[cfg(unix)]
    fn test_os_execute_applies_mise_env() {
        let lua = Lua::new();
        mod_cmd(&lua).unwrap();
        let env = lua.create_table().unwrap();
        // env_clear wipes PATH, so re-supply it for `sh` to resolve.
        env.set("PATH", std::env::var("PATH").unwrap_or_default())
            .unwrap();
        env.set("MISE_OS_EXEC_MARKER", "yes").unwrap();
        lua.set_named_registry_value("mise_env", env).unwrap();
        lua.load(
            r#"
            local ok = os.execute('[ "$MISE_OS_EXEC_MARKER" = yes ]')
            assert(ok == 0, "mise_env not applied to os.execute: " .. tostring(ok))
            local bad = os.execute('[ "$MISE_OS_EXEC_MARKER" = no ]')
            assert(bad ~= 0, "expected non-zero exit on false test")
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_cmd_shell_from_registry() {
        let lua = Lua::new();
        let shell = lua
            .create_sequence_from(["custom-shell", "-custom-arg"])
            .unwrap();
        lua.set_named_registry_value("mise_cmd_shell", shell)
            .unwrap();

        assert_eq!(
            cmd_shell(&lua).unwrap(),
            vec!["custom-shell".to_string(), "-custom-arg".to_string()]
        );
    }

    #[test]
    fn test_command_from_shell_appends_command() {
        let shell = vec!["custom-shell".to_string(), "-custom-arg".to_string()];
        let command = command_from_shell(&shell, "echo hello").unwrap();

        assert_eq!(command.get_program(), "custom-shell");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().to_string())
                .collect::<Vec<_>>(),
            vec!["-custom-arg".to_string(), "echo hello".to_string()]
        );
    }
}
