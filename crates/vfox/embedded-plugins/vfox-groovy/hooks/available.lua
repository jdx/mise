local util = require("util")
local http = require("http")
local html = require("html")
--- Return all available versions provided by this plugin
--- @param ctx table Empty table used as context, for future extension
--- @return table Descriptions of available versions and accompanying tool descriptions
function PLUGIN:Available(ctx)
    local resp, err = http.get({
        url = util.GROOVY_URL
    })
    if err ~= nil or resp.status_code ~= 200 then
        error("paring release info failed." .. err)
    end
    local result = {}
    html.parse(resp.body):find("a"):each(function(i, selection)
        local href = selection:attr("href")
        local sn = string.match(href, "^%d")
        local es = string.match(href, "/$")
        if sn and es then
            table.insert(result, {
                version = string.sub(href, 1, -2),
                note = "",
            })
        end
    end)
    table.sort(result, function(a, b)
        return util:compare_versions(a,b)
    end)
    return result
end