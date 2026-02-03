local http = require("http")

--- Get the available version list from Maven Central.
--- @param ctx table Empty table, no data provided. Always {}.
--- @return table Version list
function PLUGIN:Available(ctx)
    local result = {}

    local resp = http.get({
        url = "https://repo1.maven.org/maven2/org/asciidoctor/asciidoctorj/",
    })

    if resp.status_code ~= 200 then
        error("Failed to fetch versions from Maven Central: " .. resp.status_code)
    end

    -- Parse version directories from Maven Central HTML
    -- Pattern matches: <a href="2.5.13/" title="2.5.13/">
    for version in resp.body:gmatch('<a href="([%d%.]+)/"') do
        -- Skip versions without bin distribution (older than 1.5.0)
        local major = tonumber(version:match("^(%d+)"))
        local minor = tonumber(version:match("^%d+%.(%d+)"))
        if major and minor then
            if major > 1 or (major == 1 and minor >= 5) then
                table.insert(result, {
                    version = version,
                })
            end
        end
    end

    -- Sort versions (newest first)
    table.sort(result, function(a, b)
        return compare_versions(a.version, b.version)
    end)

    return result
end

--- Compare two version strings
--- @param v1 string
--- @param v2 string
--- @return boolean true if v1 > v2
function compare_versions(v1, v2)
    local function parse(v)
        local parts = {}
        for num in v:gmatch("(%d+)") do
            table.insert(parts, tonumber(num))
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
