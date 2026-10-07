--- List all available versions

local http = require("http")
local json = require("json")

local function fetch_github_tags(repo_url)
    -- Use git ls-remote to get tags
    local cmd = 'git ls-remote --refs --tags "' .. repo_url .. '"'

    -- Detect Windows
    local is_windows = package.config:sub(1, 1) == "\\"

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
            return a_val > b_val -- Descending order
        end
    end

    return false
end

-- Descending semver order where a prerelease sorts below its release
-- (6.0.0-rc.22 < 6.0.0). version_compare cannot do this.
local function semver_compare(a, b)
    local function parse(v)
        local core, pre = v:match("^([^-+]+)-?([^+]*)")
        local nums = {}
        for n in core:gmatch("%d+") do
            table.insert(nums, tonumber(n))
        end
        local ids = {}
        for id in pre:gmatch("[^%.]+") do
            table.insert(ids, id)
        end
        return nums, ids
    end

    local a_nums, a_pre = parse(a)
    local b_nums, b_pre = parse(b)
    for i = 1, math.max(#a_nums, #b_nums) do
        local x, y = a_nums[i] or 0, b_nums[i] or 0
        if x ~= y then
            return x > y
        end
    end
    if #a_pre == 0 or #b_pre == 0 then
        return #a_pre == 0 and #b_pre > 0
    end
    for i = 1, math.max(#a_pre, #b_pre) do
        local x, y = a_pre[i], b_pre[i]
        if x == nil or y == nil then
            return x ~= nil -- more identifiers is higher (rc < rc.1)
        end
        if x ~= y then
            local xn, yn = tonumber(x), tonumber(y)
            if xn and yn then
                return xn > yn
            elseif xn or yn then
                return yn ~= nil -- numeric identifiers are lower than alphanumeric
            end
            return x > y
        end
    end
    return false
end

function PLUGIN:Available(ctx)
    local versions = {}

    -- Get Yarn ZPM versions (v6+). Only versions published to npm are
    -- installable (some git tags never were), so list the npm package. Every
    -- platform package carries the same versions, so any one will do.
    local resp, err = http.try_get({ url = "https://registry.npmjs.org/@yarnpkg/yarn-x86_64-unknown-linux-musl" })
    if err == nil and resp.status_code == 200 then
        local zpm_versions = {}
        for version in pairs(json.decode(resp.body).versions or {}) do
            local major = tonumber(version:match("^(%d+)%.%d+%.%d+"))
            if major and major >= 6 then
                table.insert(zpm_versions, version)
            end
        end
        table.sort(zpm_versions, semver_compare)
        for _, version in ipairs(zpm_versions) do
            table.insert(versions, { version = version })
        end
    end

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
            version = version,
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
            version = version,
        })
    end

    return versions
end

return PLUGIN
