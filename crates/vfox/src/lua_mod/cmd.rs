use mlua::prelude::*;
use mlua::Table;

pub fn mod_cmd(lua: &Lua) -> LuaResult<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    let cmd = lua.create_table_from(vec![("exec", lua.create_function(exec)?)])?;
    loaded.set("cmd", cmd.clone())?;
    loaded.set("vfox.cmd", cmd)?;
    Ok(())
}

fn exec(_lua: &Lua, (command,): (String,)) -> LuaResult<String> {
    use std::process::Command;

    let output = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", &command]).output()
    } else {
        Command::new("sh").args(["-c", &command]).output()
    }
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
    fn test_cmd_windows_compatibility() {
        let lua = Lua::new();
        mod_cmd(&lua).unwrap();

        // Test that the command execution works on both Unix and Windows
        let test_command = if cfg!(target_os = "windows") {
            "echo hello world"
        } else {
            "echo hello world"
        };

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
