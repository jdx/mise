use mlua::prelude::*;
use mlua::Table;
use std::path::Path;

pub fn mod_cmd(lua: &Lua) -> LuaResult<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    let cmd = lua.create_table_from(vec![("exec", lua.create_function(exec)?)])?;
    loaded.set("cmd", cmd.clone())?;
    loaded.set("vfox.cmd", cmd)?;
    Ok(())
}

fn exec(_lua: &Lua, args: mlua::MultiValue) -> LuaResult<String> {
    use std::process::Command;

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

    let mut cmd = if cfg!(target_os = "windows") {
        Command::new("cmd")
    } else {
        Command::new("sh")
    };

    if cfg!(target_os = "windows") {
        cmd.args(["/C", &command]);
    } else {
        cmd.args(["-c", &command]);
    }

    // Apply options if provided
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
        let lua = Lua::new();
        mod_cmd(&lua).unwrap();
        lua.load(mlua::chunk! {
            local cmd = require("cmd")
            -- Test with working directory
            local result = cmd.exec("pwd", {cwd = "/tmp"})
            assert(result:find("/tmp") ~= nil)
        })
        .exec()
        .unwrap();
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
}
