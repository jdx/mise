local http = require("http")

local util = {}

util.MAVEN_REPO = "https://dl.google.com/android/maven2/com/android/tools/build"
util.GROUP_INDEX_URL = util.MAVEN_REPO .. "/group-index.xml"

function util.getOsName()
    local os_type = RUNTIME.osType
    if os_type == "darwin" then
        return "osx"
    elseif os_type == "linux" then
        return "linux"
    elseif os_type == "windows" then
        return "windows"
    else
        error("Unsupported OS: " .. os_type)
    end
end

function util.getDownloadUrl(version)
    local os_name = util.getOsName()
    return string.format("%s/aapt2/%s/aapt2-%s-%s.jar", util.MAVEN_REPO, version, version, os_name)
end

function util.parseVersions()
    local resp, err = http.get({
        url = util.GROUP_INDEX_URL
    })
    if err ~= nil or resp.status_code ~= 200 then
        error("Failed to fetch version list: " .. (err or "HTTP " .. resp.status_code))
    end

    local result = {}
    -- Parse versions from XML: <aapt2 versions="8.9.0-alpha04,8.9.0-alpha03,..."/>
    local versions_str = string.match(resp.body, '<aapt2 versions="([^"]*)"')
    if versions_str then
        for version in string.gmatch(versions_str, "([^,]+)") do
            table.insert(result, {
                version = version,
                note = "",
            })
        end
    end

    return result
end

function util.compareVersions(a, b)
    -- Extract major.minor.patch and optional suffix
    local function parseVersion(v)
        local major, minor, patch, suffix = string.match(v, "^(%d+)%.(%d+)%.(%d+)%-?(.*)$")
        if not major then
            major, minor, patch = string.match(v, "^(%d+)%.(%d+)%.(%d+)$")
            suffix = ""
        end
        return tonumber(major) or 0, tonumber(minor) or 0, tonumber(patch) or 0, suffix or ""
    end

    local a1, a2, a3, a4 = parseVersion(a)
    local b1, b2, b3, b4 = parseVersion(b)

    if a1 ~= b1 then return a1 > b1 end
    if a2 ~= b2 then return a2 > b2 end
    if a3 ~= b3 then return a3 > b3 end
    -- For suffix: stable (empty) > rc > beta > alpha
    if a4 == "" and b4 ~= "" then return true end
    if a4 ~= "" and b4 == "" then return false end
    return a4 > b4
end

return util
