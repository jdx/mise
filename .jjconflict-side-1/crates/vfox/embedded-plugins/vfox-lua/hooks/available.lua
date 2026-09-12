--- Returns all available Lua versions from lua.org
--- @param ctx table Context provided by vfox
--- @return table Available versions
function PLUGIN:Available(ctx)
    local http = require("http")

    local result = {}
    local seen = {}

    -- Fetch the Lua FTP page
    local resp, err = http.get({
        url = "https://www.lua.org/ftp/",
    })

    if err ~= nil then
        error("Failed to fetch Lua versions: " .. err)
    end

    if resp.status_code ~= 200 then
        error("Failed to fetch Lua versions, status: " .. resp.status_code)
    end

    -- Parse HTML to extract lua-X.Y.Z.tar.gz filenames
    -- Pattern matches: lua-5.4.8.tar.gz, lua-5.1.tar.gz, etc.
    for version in string.gmatch(resp.body, "lua%-(%d+%.%d+[%.%d]*).tar.gz") do
        if not seen[version] then
            seen[version] = true
            table.insert(result, {
                version = version,
            })
        end
    end

    return result
end
