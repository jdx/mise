--- Returns all available versions of carthage from GitHub releases
--- @param ctx table Context object (unused for this plugin)
--- @return table Array of version objects with 'version' field
function PLUGIN:Available(ctx)
    local util = require("util")
    local versions = util.get_versions()

    -- Sort versions (newest first)
    table.sort(versions, util.version_compare)

    -- Convert to vfox format
    local result = {}
    for _, v in ipairs(versions) do
        table.insert(result, { version = v })
    end

    return result
end
