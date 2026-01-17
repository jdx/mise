--- Returns all available versions of Redis
--- @param ctx table Context object
--- @return table Array of version objects with version and optional note fields
function PLUGIN:Available(ctx)
    local http = require("http")

    local resp, err = http.get({
        url = "https://download.redis.io/releases/"
    })

    if err ~= nil then
        error("Failed to fetch Redis releases: " .. err)
    end

    local results = {}
    local seen = {}

    -- Parse HTML to extract version numbers from redis-X.Y.Z.tar.gz links
    for version in resp.body:gmatch("redis%-(%d+%.%d+%.%d+)%.tar%.gz") do
        if not seen[version] then
            seen[version] = true
            table.insert(results, {
                version = version,
                note = ""
            })
        end
    end

    -- Parse version string into numeric components
    local function parse_version(v)
        local major, minor, patch = v:match("^(%d+)%.(%d+)%.(%d+)$")
        return tonumber(major) or 0, tonumber(minor) or 0, tonumber(patch) or 0
    end

    -- Sort versions using semantic versioning (descending order - newest first)
    table.sort(results, function(a, b)
        local a_major, a_minor, a_patch = parse_version(a.version)
        local b_major, b_minor, b_patch = parse_version(b.version)

        if a_major ~= b_major then
            return a_major > b_major
        end
        if a_minor ~= b_minor then
            return a_minor > b_minor
        end
        return a_patch > b_patch
    end)

    return results
end
