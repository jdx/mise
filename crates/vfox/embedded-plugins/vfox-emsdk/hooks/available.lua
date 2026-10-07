local http = require("http")
local json = require("json")

local available_result = nil

function PLUGIN:Available(ctx)
    if available_result then
        return available_result
    end

    local resp, err = http.get({
        url = "https://raw.githubusercontent.com/emscripten-core/emsdk/main/emscripten-releases-tags.json"
    })

    if err == nil and resp.status_code == 200 then
        local result = {}
        local seen = {}
        for version in resp.body:gmatch('"(%d+%.%d+%.%d+)"%s*:') do
            if not seen[version] then
                seen[version] = true
                table.insert(result, {
                    version = version,
                    note = ""
                })
            end
        end
        table.sort(result, compare_versions)
        if #result > 0 then
            available_result = result
            return result
        end
    end

    resp, err = http.get({
        url = "https://api.github.com/repos/emscripten-core/emsdk/releases?per_page=100"
    })

    if err ~= nil or resp.status_code ~= 200 then
        return {}
    end

    local body = json.decode(resp.body)
    local result = {}
    local seen = {}

    for _, release in ipairs(body) do
        local tag = release.tag_name
        if string.match(tag, "^%d+%.%d+%.%d+$") and not seen[tag] then
            seen[tag] = true
            table.insert(result, {
                version = tag,
                note = release.prerelease and "pre-release" or ""
            })
        end
    end

    table.sort(result, compare_versions)

    available_result = result
    return result
end

function compare_versions(a, b)
    local v1 = type(a) == "table" and a.version or a
    local v2 = type(b) == "table" and b.version or b

    local v1_parts = {}
    for part in string.gmatch(v1, "[^.]+") do
        table.insert(v1_parts, tonumber(part) or 0)
    end

    local v2_parts = {}
    for part in string.gmatch(v2, "[^.]+") do
        table.insert(v2_parts, tonumber(part) or 0)
    end

    for i = 1, math.max(#v1_parts, #v2_parts) do
        local v1_part = v1_parts[i] or 0
        local v2_part = v2_parts[i] or 0
        if v1_part > v2_part then
            return true
        elseif v1_part < v2_part then
            return false
        end
    end

    return false
end
