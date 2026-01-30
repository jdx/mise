--- Returns all available PostgreSQL versions from the official FTP server
--- @param ctx table Context provided by vfox
--- @return table Available versions
function PLUGIN:Available(ctx)
    local http = require("http")

    local result = {}
    local seen = {}

    -- Fetch the PostgreSQL source directory listing
    local resp, err = http.get({
        url = "https://ftp.postgresql.org/pub/source/",
    })

    if err ~= nil then
        error("Failed to fetch PostgreSQL versions: " .. err)
    end

    if resp.status_code ~= 200 then
        error("Failed to fetch PostgreSQL versions, status: " .. resp.status_code)
    end

    -- Parse HTML to extract version directories
    -- Format: >v17.2/<, >v16.6/<, >v9.6.24/<
    for version in string.gmatch(resp.body, '>v([0-9]+%.[0-9]+[%.0-9]*)/<') do
        if not seen[version] then
            seen[version] = true
            table.insert(result, {
                version = version,
            })
        end
    end

    -- Sort versions semantically (descending order - newest first)
    table.sort(result, function(a, b)
        return compare_versions(b.version, a.version)
    end)

    return result
end

--- Compare two version strings semantically
--- Returns true if v1 < v2 (for ascending sort)
function compare_versions(v1, v2)
    local parts1 = split_version(v1)
    local parts2 = split_version(v2)

    local max_len = math.max(#parts1, #parts2)
    for i = 1, max_len do
        local p1 = parts1[i] or 0
        local p2 = parts2[i] or 0
        if p1 ~= p2 then
            return p1 < p2
        end
    end
    return false
end

--- Split a version string into numeric parts
function split_version(version)
    local parts = {}
    for part in string.gmatch(version, "([0-9]+)") do
        table.insert(parts, tonumber(part))
    end
    return parts
end
