local http = require("http")
local json = require("json")

--- Compare two semantic version strings
--- @param v1 string
--- @param v2 string
--- @return boolean true if v1 > v2
local function compare_versions(v1, v2)
    local function parse(v)
        local parts = {}
        -- Handle pre-release versions like 2020.4.1b2, 2020.4.1.dev1
        local main = v:match("^([%d%.]+)")
        if main then
            for num in main:gmatch("(%d+)") do
                table.insert(parts, tonumber(num))
            end
        end
        -- Pre-release versions should sort before release versions
        if v:match("[abrc]%d*$") or v:match("%.dev%d*$") then
            table.insert(parts, -1)
        else
            table.insert(parts, 0)
        end
        return parts
    end

    local p1, p2 = parse(v1), parse(v2)
    for i = 1, math.max(#p1, #p2) do
        local n1, n2 = p1[i] or 0, p2[i] or 0
        if n1 ~= n2 then
            return n1 > n2
        end
    end
    return false
end

--- Get the available version list from PyPI.
--- @param ctx table Empty table, no data provided. Always {}.
--- @return table Version list
function PLUGIN:Available(ctx)
    local resp, err = http.get({
        url = "https://pypi.org/pypi/pipenv/json",
    })

    if err ~= nil or resp.status_code ~= 200 then
        error("Failed to fetch pipenv versions from PyPI: " .. (err or ("HTTP " .. resp.status_code)))
    end

    local data = json.decode(resp.body)
    if not data or not data.releases then
        error("Invalid response from PyPI")
    end

    local result = {}
    for version, release_info in pairs(data.releases) do
        -- Skip versions with no files (yanked or broken releases)
        if release_info and #release_info > 0 then
            local note = ""
            -- Mark pre-release versions
            if version:match("[abrc]%d*$") or version:match("%.dev%d*$") then
                note = "pre-release"
            end
            table.insert(result, {
                version = version,
                note = note,
            })
        end
    end

    -- Sort versions (newest first)
    table.sort(result, function(a, b)
        return compare_versions(a.version, b.version)
    end)

    return result
end
