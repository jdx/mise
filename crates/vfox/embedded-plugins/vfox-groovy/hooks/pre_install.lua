local util = require("util")
local http = require("http")
--- Returns some pre-installed information, such as version number, download address, local files, etc.
--- If checksum is provided, vfox will automatically check it for you.
--- @param ctx table
--- @field ctx.version string User-input version
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    local version = ctx.version
    local downloadUrl = util.DOWNLOAD_URL:format(version, version)

    --local resp, err = http.get({
    --    url = downloadUrl..".sha256"
    --})
    --if err ~= nil then
    --    error(err)
    --end
    --
    --if resp.status_code ~= 200 then
    --    return nil
    --end
    --
    --local sha256 = resp.body

    return {
        version = version,
        --sha256 = sha256,
        url = downloadUrl,
    }
end