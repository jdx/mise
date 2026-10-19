--- Utility functions for the clickhouse vfox plugin

local json = require("json")

local util = {}

--- Fetch content from a URL using curl
--- @param url string The URL to fetch
--- @return string|nil content The response body or nil on error
function util.fetch(url)
    local handle = io.popen(string.format('curl -sfL "%s"', url))
    if not handle then
        return nil
    end
    local content = handle:read("*a")
    handle:close()
    if content == "" then
        return nil
    end
    return content
end

--- Get all version tags from GitHub releases (only lts and stable)
--- @return table versions List of version strings
--- @return table dates Map of version string to RFC 3339 release date
function util.get_versions()
    local versions = {}
    local dates = {}
    local url = "https://api.github.com/repos/ClickHouse/ClickHouse/releases?per_page=100"
    local content = util.fetch(url)
    if not content then
        return versions, dates
    end

    -- Parse JSON to extract tag names (versions) and release dates
    local ok, releases = pcall(json.decode, content)
    if not ok or type(releases) ~= "table" then
        return versions, dates
    end
    for _, release in ipairs(releases) do
        local tag = type(release.tag_name) == "string" and release.tag_name:match("^v(.+)$")
        -- Only include lts and stable releases
        if tag and (tag:match("%-lts$") or tag:match("%-stable$")) then
            table.insert(versions, tag)
            if type(release.published_at) == "string" and dates[tag] == nil then
                dates[tag] = release.published_at
            end
        end
    end

    return versions, dates
end

--- Compare version strings for sorting (descending - newest first)
--- @param a string First version
--- @param b string Second version
--- @return boolean true if a > b
function util.version_compare(a, b)
    local function parse_version(v)
        local parts = {}
        for num in v:gmatch("(%d+)") do
            table.insert(parts, tonumber(num))
        end
        return parts
    end

    local pa = parse_version(a)
    local pb = parse_version(b)

    for i = 1, math.max(#pa, #pb) do
        local na = pa[i] or 0
        local nb = pb[i] or 0
        if na ~= nb then
            return na > nb
        end
    end
    return false
end

--- Get architecture string
--- @return string arch Architecture (amd64 or arm64)
function util.get_arch()
    local handle = io.popen("uname -m")
    if not handle then
        return "amd64"
    end
    local arch = handle:read("*l")
    handle:close()

    if arch == "x86_64" or arch == "amd64" then
        return "amd64"
    elseif arch == "aarch64" or arch == "arm64" then
        return "arm64"
    end
    return "amd64"
end

--- Trim version suffix (remove -lts, -stable, -prestable)
--- @param version string The version string
--- @return string trimmed The trimmed version
function util.trim_version_suffix(version)
    local trimmed = version:gsub("%-lts$", "")
    trimmed = trimmed:gsub("%-stable$", "")
    trimmed = trimmed:gsub("%-prestable$", "")
    return trimmed
end

return util
