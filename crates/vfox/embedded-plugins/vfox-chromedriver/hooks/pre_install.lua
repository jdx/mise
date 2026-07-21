--- Returns version information for installation
--- @param ctx table Context provided by vfox (contains version)
--- @return table Version info with download URL
function PLUGIN:PreInstall(ctx)
    local http = require("http")
    local json = require("json")

    local version = ctx.version

    if version == nil or version == "" then
        error("You must provide a version number, eg: vfox install chromedriver@131.0.6778.0")
    end

    -- Determine platform using RUNTIME global
    local osType = RUNTIME.osType
    local archType = RUNTIME.archType
    local platform = ""

    if osType == "darwin" then
        if archType == "arm64" then
            platform = "mac-arm64"
        else
            platform = "mac-x64"
        end
    elseif osType == "linux" then
        platform = "linux64"
    elseif osType == "windows" then
        if archType == "amd64" or archType == "x86_64" then
            platform = "win64"
        else
            platform = "win32"
        end
    else
        error("Unsupported OS: " .. osType)
    end

    -- Fetch the versions JSON to get the download URL
    local resp, err = http.get({
        url = "https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json",
        headers = {
            ["Accept"] = "application/json",
        },
    })

    if err ~= nil then
        error("Failed to fetch versions: " .. err)
    end

    if resp.status_code ~= 200 then
        error("Failed to fetch versions, status: " .. resp.status_code)
    end

    local data = json.decode(resp.body)
    if data == nil or data.versions == nil then
        error("Failed to parse versions JSON")
    end

    -- Find the version and platform URL
    local downloadUrl = nil
    for _, v in ipairs(data.versions) do
        if v.version == version and v.downloads and v.downloads.chromedriver then
            for _, d in ipairs(v.downloads.chromedriver) do
                if d.platform == platform then
                    downloadUrl = d.url
                    break
                end
            end
            break
        end
    end

    if downloadUrl == nil then
        error("Could not find chromedriver " .. version .. " for platform " .. platform)
    end

    return {
        version = version,
        url = downloadUrl,
    }
end
