--- Returns all available versions of chromedriver from Google's API
--- @param ctx table Context provided by vfox
--- @return table Available versions
function PLUGIN:Available(ctx)
    local http = require("http")
    local json = require("json")

    local result = {}

    local resp, err = http.get({
        url = "https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json",
        headers = {
            ["Accept"] = "application/json",
        },
    })

    if err ~= nil then
        error("Failed to fetch versions: " .. err)
    end

    if resp.status_code ~= 200 then
        error("Failed to fetch versions, status: " .. resp.status_code)
    end

    local data = json.decode(resp.body)
    if data == nil or data.versions == nil then
        return result
    end

    -- Collect versions that have chromedriver downloads
    for _, v in ipairs(data.versions) do
        if v.downloads and v.downloads.chromedriver then
            table.insert(result, {
                version = v.version,
            })
        end
    end

    return result
end
