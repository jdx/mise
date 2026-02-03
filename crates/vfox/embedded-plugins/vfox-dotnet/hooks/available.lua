--- Returns all available .NET SDK versions from Microsoft's official API
--- @param ctx table Context provided by vfox
--- @return table Available versions
function PLUGIN:Available(ctx)
    local http = require("http")
    local json = require("json")

    local result = {}
    local seen = {}

    -- Fetch the releases index to get all channels
    local indexResp, indexErr = http.get({
        url = "https://builds.dotnet.microsoft.com/dotnet/release-metadata/releases-index.json",
    })

    if indexErr ~= nil then
        error("Failed to fetch releases index: " .. indexErr)
    end

    if indexResp.status_code ~= 200 then
        error("Failed to fetch releases index, status: " .. indexResp.status_code)
    end

    local indexData = json.decode(indexResp.body)
    if indexData == nil or indexData["releases-index"] == nil then
        error("Invalid releases index format")
    end

    -- Process each channel
    for _, channel in ipairs(indexData["releases-index"]) do
        local channelVersion = channel["channel-version"]
        local releaseType = channel["release-type"] or ""
        local supportPhase = channel["support-phase"] or ""
        local releasesUrl = channel["releases.json"]

        -- Determine the note for this channel
        local channelNote = ""
        if supportPhase == "eol" then
            channelNote = "EOL"
        elseif releaseType == "lts" then
            channelNote = "LTS"
        elseif releaseType == "sts" then
            channelNote = "STS"
        end

        -- Fetch releases for this channel
        if releasesUrl ~= nil and releasesUrl ~= "" then
            local relResp, relErr = http.get({
                url = releasesUrl,
            })

            if relErr == nil and relResp.status_code == 200 then
                local relData = json.decode(relResp.body)
                if relData ~= nil and relData["releases"] ~= nil and type(relData["releases"]) == "table" then
                    for _, release in ipairs(relData["releases"]) do
                        -- Get SDK version from the release
                        if release["sdk"] ~= nil and release["sdk"]["version"] ~= nil then
                            local version = release["sdk"]["version"]
                            if not seen[version] then
                                seen[version] = true
                                local note = channelNote
                                -- Mark previews and RCs
                                if
                                    string.match(version, "preview")
                                    or string.match(version, "rc")
                                    or string.match(version, "alpha")
                                    or string.match(version, "beta")
                                then
                                    note = "Preview"
                                end
                                table.insert(result, {
                                    version = version,
                                    note = note,
                                })
                            end
                        end

                        -- Also check for additional SDKs array
                        if release["sdks"] ~= nil and type(release["sdks"]) == "table" then
                            for _, sdk in ipairs(release["sdks"]) do
                                if sdk["version"] ~= nil then
                                    local version = sdk["version"]
                                    if not seen[version] then
                                        seen[version] = true
                                        local note = channelNote
                                        if
                                            string.match(version, "preview")
                                            or string.match(version, "rc")
                                            or string.match(version, "alpha")
                                            or string.match(version, "beta")
                                        then
                                            note = "Preview"
                                        end
                                        table.insert(result, {
                                            version = version,
                                            note = note,
                                        })
                                    end
                                end
                            end
                        end
                    end
                end
            end
        end
    end

    -- Note: versions are returned in the order from the API
    -- mise handles sorting for display purposes

    return result
end
