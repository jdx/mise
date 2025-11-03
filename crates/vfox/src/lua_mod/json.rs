use mlua::{ExternalResult, Lua, LuaSerdeExt, Result, Table, Value};

pub fn mod_json(lua: &Lua) -> Result<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set(
        "json",
        lua.create_table_from(vec![
            ("encode", lua.create_function(encode)?),
            ("decode", lua.create_function(decode)?),
        ])?,
    )
}

fn encode(_lua: &Lua, value: Value) -> Result<String> {
    serde_json::to_string(&value).into_lua_err()
}

fn decode(lua: &Lua, value: String) -> Result<Value> {
    let value: serde_json::Value = serde_json::from_str(&value).into_lua_err()?;
    lua.to_value(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode() {
        let lua = Lua::new();
        mod_json(&lua).unwrap();
        lua.load(mlua::chunk! {
            local json = require("json")
            local obj = { "a", 1, "b", 2, "c", 3 }
            local jsonStr = json.encode(obj)
            assert(jsonStr == "[\"a\",1,\"b\",2,\"c\",3]")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_decode() {
        let lua = Lua::new();
        mod_json(&lua).unwrap();
        lua.load(mlua::chunk! {
            local json = require("json")
            local obj = json.decode("[\"a\",1,\"b\",2,\"c\",3]")
            assert(obj[1] == "a")
            assert(obj[2] == 1)
            assert(obj[3] == "b")
            assert(obj[4] == 2)
            assert(obj[5] == "c")
            assert(obj[6] == 3)
        })
        .exec()
        .unwrap();
    }
}
