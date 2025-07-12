use mlua::prelude::*;
use mlua::Table;

pub fn mod_env(lua: &Lua) -> LuaResult<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    let env = lua.create_table_from(vec![("setenv", lua.create_function(setenv)?)])?;
    loaded.set("env", env.clone())?;
    loaded.set("vfox.env", env)?;
    Ok(())
}

fn setenv(_lua: &Lua, (key, val): (String, String)) -> LuaResult<()> {
    std::env::set_var(key, val);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env() {
        let lua = Lua::new();
        mod_env(&lua).unwrap();
        if cfg!(windows) {
            lua.load(mlua::chunk! {
                local env = require("env")
                env.setenv("TEST_ENV", "myvar")
                handle = io.popen("pwsh -Command \"echo $env:TEST_ENV\"")
                result = handle:read("*a")
                handle:close()
                assert(result == "myvar\n")
            })
            .exec()
            .unwrap();
        } else {
            lua.load(mlua::chunk! {
                local env = require("env")
                env.setenv("TEST_ENV", "myvar")
                handle = io.popen("echo $TEST_ENV")
                result = handle:read("*a")
                handle:close()
                assert(result == "myvar\n")
            })
            .exec()
            .unwrap();
        }
    }
}
