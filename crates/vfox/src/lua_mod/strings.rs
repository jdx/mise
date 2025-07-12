use mlua::prelude::*;
use mlua::{Table, Value};

pub fn mod_strings(lua: &Lua) -> LuaResult<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    let strings = lua.create_table_from(vec![
        ("split", lua.create_function(split)?),
        ("has_prefix", lua.create_function(has_prefix)?),
        ("has_suffix", lua.create_function(has_suffix)?),
        ("trim", lua.create_function(trim)?),
        ("trim_space", lua.create_function(trim_space)?),
        ("contains", lua.create_function(contains)?),
        ("join", lua.create_function(join)?),
    ])?;
    loaded.set("strings", strings.clone())?;
    loaded.set("vfox.strings", strings)?;
    Ok(())
}

fn split(_lua: &Lua, (s, sep): (String, String)) -> LuaResult<Vec<String>> {
    Ok(s.split(&sep).map(|s| s.to_string()).collect())
}

fn has_prefix(_lua: &Lua, (s, prefix): (String, String)) -> LuaResult<bool> {
    Ok(s.starts_with(&prefix))
}

fn has_suffix(_lua: &Lua, (s, suffix): (String, String)) -> LuaResult<bool> {
    Ok(s.ends_with(&suffix))
}

fn trim(_lua: &Lua, (s, suffix): (String, String)) -> LuaResult<String> {
    Ok(s.trim_end_matches(&suffix).to_string())
}

fn trim_space(_lua: &Lua, s: String) -> LuaResult<String> {
    Ok(s.trim().to_string())
}

fn contains(_lua: &Lua, (s, substr): (String, String)) -> LuaResult<bool> {
    Ok(s.contains(&substr))
}

fn join(_lua: &Lua, (arr, sep): (Vec<Value>, String)) -> LuaResult<String> {
    let mut res = String::new();
    for (i, v) in arr.iter().enumerate() {
        if i > 0 {
            res.push_str(&sep);
        }
        res.push_str(&v.to_string()?);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strings() {
        let lua = Lua::new();
        mod_strings(&lua).unwrap();
        lua.load(mlua::chunk! {
            local strings = require("strings")
            local str_parts = strings.split("hello world", " ")
            print(str_parts[1]) -- hello

            assert(strings.has_prefix("hello world", "hello"), [[not strings.has_prefix("hello")]])
            assert(strings.has_suffix("hello world", "world"), [[not strings.has_suffix("world")]])
            assert(strings.trim("hello world", "world") == "hello ", "strings.trim()")
            assert(strings.contains("hello world", "hello ") == true, "strings.contains()")

            // got = strings.trim_space(tt.input)
            //
            // local str = strings.join({"1",3,"4"},";")
            // assert(str == "1;3;4", "strings.join()")
        })
        .exec()
        .unwrap();
    }
}
