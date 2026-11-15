use mlua::{Function, Lua, Result};

/// Replaces the global `pcall` with one that can yield.
///
/// The embedded Lua is 5.1, whose `pcall` is a C function that cannot be
/// yielded across. The async modules (`http`, `cmd`, ...) suspend the plugin's
/// coroutine while they wait, so a plugin that wraps one of them in `pcall`
/// fails with "attempt to yield across metamethod/C-call boundary", even when
/// the call would have succeeded. This version runs the function in its own
/// coroutine and passes its yields on to the caller, so the protected call can
/// wait like any other.
pub(crate) fn mod_pcall(lua: &Lua) -> Result<()> {
    let pcall: Function = lua
        .load(
            r#"
            local create, resume, status, yield =
                coroutine.create, coroutine.resume, coroutine.status, coroutine.yield

            -- `ok` is the result of the last resume. While the function is
            -- suspended, hand what it yielded to our caller and resume it with
            -- whatever the caller sends back.
            local function settle(co, ok, ...)
                if status(co) == "suspended" then
                    return settle(co, resume(co, yield(...)))
                end
                return ok, ...
            end

            return function(f, ...)
                -- coroutine.create rejects C functions in 5.1, and most of the
                -- plugin API is one, so call through a Lua function.
                local co = create(function(...)
                    return f(...)
                end)
                return settle(co, resume(co, ...))
            end
            "#,
        )
        .set_name("=pcall")
        .eval()?;
    lua.globals().set("pcall", pcall)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::MultiValue;

    /// An async function that suspends the coroutine once, as `http.get` does.
    fn async_double(lua: &Lua) -> Result<Function> {
        lua.create_async_function(|_, n: i64| async move {
            tokio::task::yield_now().await;
            Ok(n * 2)
        })
    }

    async fn eval(lua: &Lua, code: &str) -> Result<MultiValue> {
        lua.load(code).eval_async().await
    }

    #[tokio::test]
    async fn native_pcall_cannot_yield() {
        let lua = Lua::new();
        lua.globals()
            .set("double", async_double(&lua).unwrap())
            .unwrap();
        let err = eval(&lua, "return pcall(double, 21)").await.unwrap();
        // pcall reports the failure instead of raising it.
        assert_eq!(err.len(), 2);
        assert_eq!(err[0].as_boolean(), Some(false));
        assert!(
            err[1]
                .to_string()
                .unwrap()
                .contains("attempt to yield across metamethod/C-call boundary")
        );
    }

    #[tokio::test]
    async fn returns_results_of_an_async_call() {
        let lua = Lua::new();
        mod_pcall(&lua).unwrap();
        lua.globals()
            .set("double", async_double(&lua).unwrap())
            .unwrap();
        let out = eval(&lua, "return pcall(double, 21)").await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].as_boolean(), Some(true));
        assert_eq!(out[1].as_integer(), Some(42));
    }

    #[tokio::test]
    async fn yields_inside_a_lua_function_and_nested_pcall() {
        let lua = Lua::new();
        mod_pcall(&lua).unwrap();
        lua.globals()
            .set("double", async_double(&lua).unwrap())
            .unwrap();
        let out = eval(
            &lua,
            r#"
            local ok, value = pcall(function(n)
                local inner_ok, inner = pcall(double, n)
                return inner + double(inner)
            end, 5)
            return ok, value
            "#,
        )
        .await
        .unwrap();
        assert_eq!(out[0].as_boolean(), Some(true));
        assert_eq!(out[1].as_integer(), Some(30));
    }

    #[tokio::test]
    async fn catches_errors_after_a_yield() {
        let lua = Lua::new();
        mod_pcall(&lua).unwrap();
        lua.globals()
            .set("double", async_double(&lua).unwrap())
            .unwrap();
        let out = eval(
            &lua,
            r#"
            return pcall(function()
                double(1)
                error("boom")
            end)
            "#,
        )
        .await
        .unwrap();
        assert_eq!(out[0].as_boolean(), Some(false));
        assert!(out[1].to_string().unwrap().contains("boom"));
    }

    #[test]
    fn behaves_like_pcall_without_yielding() {
        let lua = Lua::new();
        mod_pcall(&lua).unwrap();
        let out: MultiValue = lua
            .load(
                r#"
                local a, b, c = pcall(function(...) return ... end, 1, nil, 3)
                local d, e = pcall(error, { code = 7 })
                local f, g = pcall(nil)
                local h, i = pcall(string.rep, "ab", 2)
                return a, b, c, d, e.code, f, g, h, i
                "#,
            )
            .eval()
            .unwrap();
        let out: Vec<String> = out.iter().map(|v| v.to_string().unwrap()).collect();
        assert_eq!(out[0], "true");
        assert_eq!(out[1], "1");
        assert_eq!(out[3], "false");
        assert_eq!(out[4], "7");
        assert_eq!(out[5], "false");
        assert!(out[6].contains("attempt to call"));
        assert_eq!(out[7], "true");
        assert_eq!(out[8], "abab");
    }
}
