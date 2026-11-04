--- Utility functions for the carthage vfox plugin

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

--- Get all version tags from GitHub releases
--- @return table versions List of version strings
--- @return table dates Map of version string to RFC 3339 release date
function util.get_versions()
    local versions = {}
    local dates = {}
    local url = "https://api.github.com/repos/Carthage/Carthage/releases?per_page=100"
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
        local tag = release.tag_name
        -- Carthage uses tags like "0.40.0", "0.39.1", etc.
        if type(tag) == "string" and tag:match("^%d+%.%d+") then
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

return util
