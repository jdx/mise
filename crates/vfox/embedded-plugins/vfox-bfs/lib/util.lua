--- Utility functions for the bfs vfox plugin

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
function util.get_versions()
    local versions = {}
    local url = "https://api.github.com/repos/tavianator/bfs/tags?per_page=100"
    local content = util.fetch(url)
    if not content then
        return versions
    end

    -- Parse JSON to extract tag names (versions)
    for tag in content:gmatch('"name"%s*:%s*"([^"]+)"') do
        -- bfs uses tags like "4.0.4", "4.1", etc. (no 'v' prefix)
        if tag:match("^%d+%.%d+") then
            table.insert(versions, tag)
        end
    end

    return versions
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
