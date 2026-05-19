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
    let has_mise_env = if let Ok(mise_env) = lua.named_registry_value::<Table>("mise_env") {
        cmd.env_clear();
        for pair in mise_env.pairs::<String, String>() {
            let (key, value) = pair?;
            cmd.env(key, value);
        }
        true
    } else {
        false
    };
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
