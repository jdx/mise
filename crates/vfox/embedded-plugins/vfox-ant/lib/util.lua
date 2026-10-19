local http = require("http")
local html = require("html")

local util = {}

util.ARCHIVE_URL = "https://archive.apache.org/dist/ant/binaries/"
util.FILE_URL = "https://archive.apache.org/dist/ant/binaries/apache-ant-%s-bin.tar.gz"
util.CHECKSUM_URL = "https://archive.apache.org/dist/ant/binaries/apache-ant-%s-bin.tar.gz.%s"

function util.parseVersions()
    local resp, err = http.get({
        url = util.ARCHIVE_URL,
    })
    if err ~= nil or resp.status_code ~= 200 then
        error("Failed to fetch version list: " .. (err or "HTTP " .. resp.status_code))
    end

    local result = {}
    html.parse(resp.body):find("a"):each(function(i, selection)
        local href = selection:attr("href")
        -- Match apache-ant-X.Y.Z-bin.tar.gz
        local version = string.match(href, "^apache%-ant%-([%d%.]+)%-bin%.tar%.gz$")
        if version then
            table.insert(result, {
                version = version,
                note = "",
            })
        end
    end)

    return result
end

function util.compareVersions(a, b)
    local function parseVersion(v)
        local parts = {}
        for part in string.gmatch(v, "([^.]+)") do
            table.insert(parts, tonumber(part) or 0)
        end
        return parts
    end

    local pa = parseVersion(a)
    local pb = parseVersion(b)

    for i = 1, math.max(#pa, #pb) do
        local va = pa[i] or 0
        local vb = pb[i] or 0
        if va ~= vb then
            return va > vb
        end
    end
    return false
end

return util
