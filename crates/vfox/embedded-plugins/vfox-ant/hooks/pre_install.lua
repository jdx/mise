local http = require("http")
local util = require("util")

--- Returns pre-installed information, such as version number, download address, etc.
--- If checksum is provided, vfox will automatically check it for you.
--- @param ctx {version: string}  (User-input version)
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    -- Try to get checksum (sha512 first, then sha1)
    local checksums = {
        { type = "sha512", url = util.CHECKSUM_URL:format(version, "sha512") },
        { type = "sha1", url = util.CHECKSUM_URL:format(version, "sha1") },
    }

    for _, checksum in ipairs(checksums) do
        local resp, err = http.get({
            url = checksum.url,
        })
        if err == nil and resp.status_code == 200 then
            local hash = string.match(resp.body, "^(%S+)")
            if hash then
                local result = {
                    version = version,
                    url = util.FILE_URL:format(version),
                }
                result[checksum.type] = hash
                return result
            end
        end
    end

    -- No checksum found, return without checksum
    return {
        version = version,
        url = util.FILE_URL:format(version),
    }
end
