--- Returns available PHP versions from GitHub php-src tags
--- @param ctx table Context provided by vfox
--- @return table Available versions
function PLUGIN:Available(ctx)
    local http = require("http")
    local result = {}
    local seen = {}

    -- Fetch tags from php-src GitHub repository
    -- We use the GitHub API to get all tags
    local page = 1
    local per_page = 100

    while true do
        local resp, err = http.get({
            url = "https://api.github.com/repos/php/php-src/tags?per_page=" .. per_page .. "&page=" .. page,
            headers = {
                ["Accept"] = "application/vnd.github.v3+json",
                ["User-Agent"] = "vfox-php",
            },
        })

        if err ~= nil or resp.status_code ~= 200 then
            -- Fallback: try git ls-remote approach via web
            break
        end

        local json = require("json")
        local tags = json.decode(resp.body)

        if tags == nil or #tags == 0 then
            break
        end

        for _, tag in ipairs(tags) do
            local name = tag.name
            -- Match tags like "php-8.4.17", "php-8.3.0RC1", etc.
            local version = string.match(name, "^php%-([0-9]+%.[0-9]+%.%d+[%w%-]*)$")
            if version ~= nil and not seen[version] then
                seen[version] = true
                table.insert(result, { version = version })
            end
        end

        page = page + 1

        -- Stop if we got less than a full page
        if #tags < per_page then
            break
        end

        -- Safety limit
        if page > 20 then
            break
        end
    end

    -- Sort versions (newest first)
    table.sort(result, function(a, b)
        return compare_versions(a.version, b.version) > 0
    end)

    return result
end

--- Compare two version strings
--- Returns positive if a > b, negative if a < b, 0 if equal
function compare_versions(a, b)
    -- Extract numeric parts
    local a_major, a_minor, a_patch = string.match(a, "^(%d+)%.(%d+)%.(%d+)")
    local b_major, b_minor, b_patch = string.match(b, "^(%d+)%.(%d+)%.(%d+)")

    if a_major == nil or b_major == nil then
        return 0
    end

    a_major, a_minor, a_patch = tonumber(a_major), tonumber(a_minor), tonumber(a_patch)
    b_major, b_minor, b_patch = tonumber(b_major), tonumber(b_minor), tonumber(b_patch)

    if a_major ~= b_major then
        return a_major - b_major
    end
    if a_minor ~= b_minor then
        return a_minor - b_minor
    end
    if a_patch ~= b_patch then
        return a_patch - b_patch
    end

    -- Check for RC/alpha/beta suffix (stable versions come after pre-releases)
    local a_has_suffix = string.match(a, "%d+%.%d+%.%d+[%-]?%a")
    local b_has_suffix = string.match(b, "%d+%.%d+%.%d+[%-]?%a")

    if a_has_suffix and not b_has_suffix then
        return -1 -- a is pre-release, b is stable
    elseif not a_has_suffix and b_has_suffix then
        return 1 -- a is stable, b is pre-release
    end

    return 0
end
