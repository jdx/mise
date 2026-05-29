local util = require("util")
local http = require("http")
--- Return all available versions provided by this plugin
--- @param ctx table Empty table used as context, for future extension
--- @return table Descriptions of available versions and accompanying tool descriptions
function PLUGIN:Available(ctx)
    local resp, err = http.get({
        url = util.SEARCH_URL,
    })
    print(err)
    if err ~= nil or resp.status_code ~= 200 then
        return {}
    end
    local htmlBody = resp.body
    local htmlContent = [[]] .. htmlBody .. [[]]
    local result = {}

    for match in htmlContent:gmatch('<div class="download%-elem">.-<a href="[^"]-">([^<]-)</a>.-</div>') do
        table.insert(result, { version = match:gsub("^Scala ", ""), note = "" })
    end

    return result
end
