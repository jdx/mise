--- Returns all available versions of Leiningen
--- @param ctx table Context object
--- @return table Array of version objects with version and optional note fields
function PLUGIN:Available(ctx)
    local http = require("http")
    local json = require("json")

    local resp, err = http.get({
        url = "https://api.github.com/repos/technomancy/leiningen/releases",
    })

    if err ~= nil then
        error("Failed to fetch Leiningen releases: " .. err)
    end

    local releases, err = json.decode(resp.body)
    if err ~= nil then
        error("Failed to parse releases JSON: " .. err)
    end

    local results = {}

    for _, release in ipairs(releases) do
        local version = release.tag_name
        if version then
            table.insert(results, {
                version = version,
                note = release.prerelease and "prerelease" or "",
            })
        end
    end

    return results
end
