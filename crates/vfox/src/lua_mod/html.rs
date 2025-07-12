use mlua::{Lua, Table};

pub fn mod_html(lua: &Lua) -> mlua::Result<()> {
    let package: Table = lua.globals().get("package")?;
    let loaded: Table = package.get("loaded")?;
    loaded.set(
        "htmlparser.voidelements",
        lua.load(include_str!("../../lua/htmlparser/voidelements.lua"))
            .eval::<Table>()?,
    )?;
    loaded.set(
        "htmlparser.ElementNode",
        lua.load(include_str!("../../lua/htmlparser/ElementNode.lua"))
            .eval::<Table>()?,
    )?;
    loaded.set(
        "htmlparser",
        lua.load(include_str!("../../lua/htmlparser.lua"))
            .eval::<Table>()?,
    )?;
    loaded.set(
        "html",
        lua.load(mlua::chunk! {
            local htmlparser = require("htmlparser")
            return {
                parse = function(s)
                    Node = {
                        find = function(self, tag)
                            local nodes = self.node:select(tag)
                            return Node.new(nodes)
                        end,
                        first = function(self)
                            return Node.new({self.nodes[1]})
                        end,
                        eq = function(self, idx)
                            local node = self.nodes[idx + 1]
                            return Node.new({node})
                        end,
                        each = function(self, f)
                            for i, node in ipairs(self.nodes) do
                                f(i - 1, Node.new({node}))
                            end
                        end,
                        text = function(self)
                            if self.node == nil then
                                return ""
                            end
                            return self.node:getcontent()
                        end,
                        attr = function(self, key)
                            if self.node == nil then
                                return ""
                            end
                            return self.node.attributes[key]
                        end,
                    }
                    Node.new = function(nodes)
                        return setmetatable({nodes = nodes, node = nodes[1]}, {__index = Node})
                    end
                    local root = htmlparser.parse(s, 100000)
                    return Node.new({root})
                end
            }
        })
        .eval::<Table>()?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua_mod::http::mod_http;

    #[test]
    fn test_html() {
        let lua = Lua::new();
        mod_html(&lua).unwrap();
        lua.load(mlua::chunk! {
            local html = require("html")
            local doc = html.parse("<html><body><div id='t2' name='123'>456</div><DIV foo='bar'>222</DIV></body></html>")
            local f = doc:find("div"):eq(0)
            local s = doc:find("div"):eq(1)
            assert(s:text() == "222")
            assert(f:text() == "456")

            assert(s:attr("foo") == "bar")

            doc:find("div"):each(function(i, e)
                if i == 0 then
                    assert(e:text() == "456")
                else
                    assert(e:text() == "222")
                end
            end)
        })
            .exec()
            .unwrap();
    }

    #[tokio::test]
    #[ignore] // TODO: make this actually work
    async fn test_html_go() {
        let lua = Lua::new();
        mod_html(&lua).unwrap();
        mod_http(&lua).unwrap();
        lua.load(mlua::chunk! {
            local http = require("http")
            local html = require("html")

            table = {}

            resp, err = http.get({
                url = "https://go.dev/dl/"
            })
            if err ~= nil or resp.status_code ~= 200 then
                error("parsing release info failed." .. err)
            end
            local doc = html.parse(resp.body)
            local listDoc = doc:find("div#archive")
            listDoc:find(".toggle"):each(function(i, selection)
                local versionStr = selection:attr("id")
                if versionStr ~= nil then
                    selection:find("table.downloadtable tr"):each(function(ti, ts)
                        local td = ts:find("td")
                        local filename = td:eq(0):text()
                        local kind = td:eq(1):text()
                        local os = td:eq(2):text()
                        local arch = td:eq(3):text()
                        local checksum = td:eq(5):text()
                        if kind == "Archive" and os == "Windows" and arch == "x86-64" then
                            table.insert(result, {
                                version = string.sub(versionStr, 3),
                                url = "https://go.dev/dl/" .. filename,
                                note = "",
                                sha256 = checksum,
                            })
                        end
                    end)
                end
            end)
            print(table)
            // TODO: check results
        })
        .exec_async()
        .await
        .unwrap();
    }
}
