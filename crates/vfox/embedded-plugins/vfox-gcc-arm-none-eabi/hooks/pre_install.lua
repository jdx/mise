local versions = require("versions")
local strings = require("vfox.strings")

--- Returns some pre-installed information, such as version number, download address, local files, etc.
--- If checksum is provided, vfox will automatically check it for you.
--- @param ctx table
--- @field ctx.version string User-input version
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    -- Extract version number and architecture
    local version_parts = strings.split(ctx.version, "~")
    local version = version_parts[1]
    local arch = version_parts[2]
    return versions:GetPreInstall(version, arch)
end