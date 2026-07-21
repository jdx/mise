local util = require("util")
local http = require("http")

--- Return all available versions provided by this plugin
--- @param ctx table Empty table used as context, for future extension
--- @return table Descriptions of available versions and accompanying tool descriptions
function PLUGIN:Available(ctx)
    local resp, err = http.get({
        url = util.SEARCH_URL,
    })
    if err ~= nil or resp.status_code ~= 200 then
        return {}
    end

    local htmlBody = resp.body
    local htmlContent = [[]] .. htmlBody .. [[]]
    local versions = {}

    for version in htmlContent:gmatch('<a href="/download/([^"]-)%.html">Scala%s+[^<]-</a>') do
        if util:isVersion(version) then
            table.insert(versions, version)
        end
    end

    local result = {}
    for _, version in ipairs(versions) do
        table.insert(result, { version = version, note = "" })
    end

    return result
end
