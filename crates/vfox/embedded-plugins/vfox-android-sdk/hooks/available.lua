--- Return all available versions provided by this plugin
--- @param ctx table Empty table used as context, for future extension
--- @return table Descriptions of available versions and accompanying tool descriptions
function PLUGIN:Available(ctx)
    local http = require("http")
    local env = require("env")

    local base_url = env.ANDROID_SDK_MIRROR_URL or "https://dl.google.com/android/repository"
    local metadata_url = base_url .. "/repository2-3.xml"

    local resp = http.get({ url = metadata_url })
    if resp.status_code ~= 200 then
        error("Failed to fetch Android SDK metadata: HTTP " .. resp.status_code)
    end

    local versions = {}
    local seen = {}

    -- Parse XML to find cmdline-tools packages
    -- Look for remotePackage elements with path="cmdline-tools;VERSION"
    for path_attr in resp.body:gmatch('remotePackage%s+path="([^"]+)"') do
        -- Match cmdline-tools;VERSION pattern, excluding "latest"
        local version = path_attr:match("^cmdline%-tools;(.+)$")
        if version and version ~= "latest" and not seen[version] then
            seen[version] = true
            table.insert(versions, {
                version = version,
                note = "",
            })
        end
    end

    -- Sort versions (simple string sort, works for numeric versions)
    table.sort(versions, function(a, b)
        return a.version < b.version
    end)

    return versions
end
