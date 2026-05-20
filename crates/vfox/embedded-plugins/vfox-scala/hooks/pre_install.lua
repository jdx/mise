local util = require("util")
--- Returns some pre-installed information, such as version number, download address, local files, etc.
--- If checksum is provided, vfox will automatically check it for you.
--- @param ctx table
--- @field ctx.version string User-input version
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    local version = ctx.version
    local downloadUrl = util:getDownloadUrl(version)
    -- TODO: get scala 3+ sha256sum
    return {
        version = version,
        url = downloadUrl,
    }
end
