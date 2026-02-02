use mlua::Table;
use mlua::prelude::*;

use super::get_or_create_loaded;

pub fn mod_env(lua: &Lua) -> LuaResult<()> {
    let loaded: Table = get_or_create_loaded(lua)?;
    let env = lua.create_table_from(vec![
        ("setenv", lua.create_function(setenv)?),
        ("getenv", lua.create_function(getenv)?),
    ])?;
    loaded.set("env", env.clone())?;
    loaded.set("vfox.env", env)?;
    Ok(())
}

fn setenv(_lua: &Lua, (key, val): (String, String)) -> LuaResult<()> {
    unsafe {
        std::env::set_var(key, val);
    }
    Ok(())
}

fn getenv(_lua: &Lua, key: String) -> LuaResult<Option<String>> {
    Ok(std::env::var(&key).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env() {
        let lua = Lua::new();
        mod_env(&lua).unwrap();
        lua.load(mlua::chunk! {
            local env = require("env")
            env.setenv("TEST_ENV", "myvar")
            local val = env.getenv("TEST_ENV")
            assert(val == "myvar", "expected 'myvar', got: " .. tostring(val))
        })
        .exec()
        .unwrap();
    }
}
