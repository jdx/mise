function PLUGIN:BackendListVersions(ctx)
    local cmd = require("cmd")
    local json = require("json")

    local result = cmd.exec("npm view " .. ctx.tool .. " versions --json")
    local versions = json.decode(result)
    
    return {versions = versions}
end
