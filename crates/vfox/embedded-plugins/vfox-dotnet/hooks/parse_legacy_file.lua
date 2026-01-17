--- Parses global.json to extract .NET SDK version
--- @param ctx table Context provided by vfox
--- @return table Version info
function PLUGIN:ParseLegacyFile(ctx)
    local json = require("json")
    local filename = ctx.filename
    local filepath = ctx.filepath

    -- Only handle global.json
    if filename ~= "global.json" then
        return {}
    end

    -- Read the file
    local f = io.open(filepath, "r")
    if f == nil then
        return {}
    end
    local content = f:read("*all")
    f:close()

    -- Parse JSON
    local data = json.decode(content)
    if data == nil then
        return {}
    end

    -- Extract SDK version from global.json format:
    -- { "sdk": { "version": "8.0.100" } }
    if data["sdk"] ~= nil and data["sdk"]["version"] ~= nil then
        return {
            version = data["sdk"]["version"],
        }
    end

    return {}
end
