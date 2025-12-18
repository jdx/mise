--- List all available versions

local function fetch_github_tags(repo_url)
    -- Use git ls-remote to get tags
    local cmd = 'git ls-remote --refs --tags "' .. repo_url .. '"'
    
    -- Detect Windows
    local is_windows = package.config:sub(1,1) == '\\'
    
    -- Redirect stderr appropriately for the platform
    if is_windows then
        cmd = cmd .. " 2>NUL"
    else
        cmd = cmd .. " 2>/dev/null"
    end
    
    local handle = io.popen(cmd)
    if not handle then
        return {}
    end
    
    local result = handle:read("*a")
    handle:close()
    
    -- If result is empty or nil, return empty table
    if not result or result == "" then
        return {}
    end
    
    local tags = {}
    for line in result:gmatch("[^\r\n]+") do
        -- Extract tag name from refs/tags/...
        local tag = line:match("refs/tags/(.+)$")
        if tag then
            table.insert(tags, tag)
        end
    end
    
    return tags
end

local function version_compare(a, b)
    -- Simple version comparison for sorting
    local function parse_version(v)
        local parts = {}
        for part in string.gmatch(v, "[^%.]+") do
            table.insert(parts, tonumber(part) or 0)
        end
        return parts
    end
    
    local a_parts = parse_version(a)
    local b_parts = parse_version(b)
    
    for i = 1, math.max(#a_parts, #b_parts) do
        local a_val = a_parts[i] or 0
        local b_val = b_parts[i] or 0
        if a_val ~= b_val then
            return a_val > b_val  -- Descending order
        end
    end
    
    return false
end

function PLUGIN:Available(ctx)
    local versions = {}
    
    -- Get Yarn Berry versions (v2.x+)
    local berry_tags = fetch_github_tags("https://github.com/yarnpkg/berry.git")
    local berry_versions = {}
    
    for _, tag in ipairs(berry_tags) do
        -- Extract version from @yarnpkg/cli/X.X.X format
        local version = tag:match("@yarnpkg/cli/(.+)$")
        if version then
            table.insert(berry_versions, version)
        end
    end
    
    -- Sort Berry versions in descending order
    table.sort(berry_versions, version_compare)
    
    -- Add Berry versions to the list
    for _, version in ipairs(berry_versions) do
        table.insert(versions, {
            version = version
        })
    end
    
    -- Get Yarn Classic versions (v1.x)
    local classic_tags = fetch_github_tags("https://github.com/yarnpkg/yarn.git")
    local classic_versions = {}
    
    for _, tag in ipairs(classic_tags) do
        -- Remove 'v' prefix if present
        local version = tag:match("^v(.+)$") or tag
        -- Only include 1.x versions (exclude 0.x)
        if version:match("^1%.") then
            table.insert(classic_versions, version)
        end
    end
    
    -- Sort Classic versions in descending order
    table.sort(classic_versions, version_compare)
    
    -- Add Classic versions to the list
    for _, version in ipairs(classic_versions) do
        table.insert(versions, {
            version = version
        })
    end
    
    return versions
end

return PLUGIN