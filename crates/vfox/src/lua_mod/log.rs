use mlua::{Lua, Result, Table, Value, Variadic};

fn values_to_string(lua: &Lua, args: Variadic<Value>) -> Result<String> {
    let tostring: mlua::Function = lua.globals().get("tostring")?;
    let parts: Vec<String> = args
        .iter()
        .map(|v| {
            tostring
                .call::<String>(v)
                .unwrap_or_else(|_| "?".to_string())
        })
        .collect();
    Ok(parts.join("\t"))
}

fn get_plugin_name(lua: &Lua) -> Option<String> {
    lua.named_registry_value::<String>("plugin_name").ok()
}

fn format_msg(plugin_name: Option<&str>, msg: &str) -> String {
    match plugin_name {
        Some(name) => format!("[{}] {}", name, msg),
        None => msg.to_string(),
    }
}

macro_rules! create_log_fn {
    ($lua:expr, $level:expr) => {
        $lua.create_function(|lua, args: Variadic<Value>| {
            if log::log_enabled!($level) {
                let msg = values_to_string(lua, args)?;
                let name = get_plugin_name(lua);
                log::log!($level, "{}", format_msg(name.as_deref(), &msg));
            }
            Ok(())
        })?
    };
}

pub fn mod_log(lua: &Lua) -> Result<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;

    let log_table = lua.create_table()?;

    log_table.set("trace", create_log_fn!(lua, log::Level::Trace))?;
    log_table.set("debug", create_log_fn!(lua, log::Level::Debug))?;

    let info_fn = create_log_fn!(lua, log::Level::Info);
    log_table.set("info", info_fn.clone())?;

    log_table.set("warn", create_log_fn!(lua, log::Level::Warn))?;
    log_table.set("error", create_log_fn!(lua, log::Level::Error))?;

    loaded.set("log", log_table.clone())?;

    // Also register as vfox.log
    let vfox_table: Table = match loaded.get::<Option<Table>>("vfox")? {
        Some(t) => t,
        None => {
            let t = lua.create_table()?;
            loaded.set("vfox", t.clone())?;
            t
        }
    };
    vfox_table.set("log", log_table)?;

    // Override print() to route through info!()
    lua.globals().set("print", info_fn)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mod_log_registration() {
        let lua = Lua::new();
        mod_log(&lua).unwrap();

        lua.load(mlua::chunk! {
            local log = require("log")
            assert(type(log) == "table")
            assert(type(log.trace) == "function")
            assert(type(log.debug) == "function")
            assert(type(log.info) == "function")
            assert(type(log.warn) == "function")
            assert(type(log.error) == "function")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_vfox_log_namespace() {
        let lua = Lua::new();
        mod_log(&lua).unwrap();

        lua.load(mlua::chunk! {
            local vfox_log = require("vfox").log
            assert(type(vfox_log) == "table")
            assert(type(vfox_log.info) == "function")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_log_functions_execute() {
        let lua = Lua::new();
        mod_log(&lua).unwrap();

        // Should not panic with no plugin_name set
        lua.load(mlua::chunk! {
            local log = require("log")
            log.trace("trace msg")
            log.debug("debug msg")
            log.info("info msg")
            log.warn("warn msg")
            log.error("error msg")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_log_with_plugin_name() {
        let lua = Lua::new();
        lua.set_named_registry_value("plugin_name", "test-plugin")
            .unwrap();
        mod_log(&lua).unwrap();

        lua.load(mlua::chunk! {
            local log = require("log")
            log.info("hello from plugin")
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_log_multiple_args() {
        let lua = Lua::new();
        mod_log(&lua).unwrap();

        lua.load(mlua::chunk! {
            local log = require("log")
            log.info("multi", "arg", 123, true, nil)
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_print_override() {
        let lua = Lua::new();
        mod_log(&lua).unwrap();

        lua.load(mlua::chunk! {
            print("hello", "world", 42)
        })
        .exec()
        .unwrap();
    }

    #[test]
    fn test_print_override_with_plugin_name() {
        let lua = Lua::new();
        lua.set_named_registry_value("plugin_name", "my-plugin")
            .unwrap();
        mod_log(&lua).unwrap();

        lua.load(mlua::chunk! {
            print("test message")
        })
        .exec()
        .unwrap();
    }
}
