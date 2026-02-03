--- Returns a list of available versions of CHICKEN
--- Fetches from foldling.org/dust/ and parses the tarball filenames

function PLUGIN:Available(ctx)
    local http = require("http")
    local resp, err = http.get({
        url = "https://foldling.org/dust/",
    })
    if err ~= nil then
        error("Failed to fetch version list: " .. err)
    end
    if resp.status_code ~= 200 then
        error("Failed to fetch version list: HTTP " .. resp.status_code)
    end

    local versions = {}
    local seen = {}

    -- Parse HTML for chicken tarballs: chicken-VERSION-ARCH-OS-VARIANT.tar.gz
    -- Only match versioned releases, not "master"
    for version in resp.body:gmatch("chicken%-([0-9]+%.[0-9]+%.[0-9]+)%-") do
        if not seen[version] then
            seen[version] = true
            table.insert(versions, {
                version = version,
            })
        end
    end

    -- Sort versions in descending order (newest first)
    table.sort(versions, function(a, b)
        local function parse_version(v)
            local parts = {}
            for num in v:gmatch("(%d+)") do
                table.insert(parts, tonumber(num))
            end
            return parts
        end
        local va = parse_version(a.version)
        local vb = parse_version(b.version)
        for i = 1, math.max(#va, #vb) do
            local na = va[i] or 0
            local nb = vb[i] or 0
            if na ~= nb then
                return na > nb
            end
        end
        return false
    end)

    return versions
end
