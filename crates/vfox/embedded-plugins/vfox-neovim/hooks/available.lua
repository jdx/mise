--- Returns a list of available Neovim versions
--- Fetches from GitHub releases API

-- Helper function to get checksum for a release
local function get_release_checksum(release, http)
    -- Determine platform-specific asset name
    local os_type = RUNTIME.osType
    local arch_type = RUNTIME.archType
    local platform, ext

    if os_type == "darwin" then
        ext = ".tar.gz"
        if arch_type == "arm64" then
            platform = "macos-arm64"
        else
            platform = "macos-x86_64"
        end
    elseif os_type == "linux" then
        ext = ".tar.gz"
        if arch_type == "arm64" then
            platform = "linux-arm64"
        else
            platform = "linux-x86_64"
        end
    elseif os_type == "windows" then
        ext = ".zip"
        if arch_type == "arm64" then
            platform = "win-arm64"
        else
            platform = "win64"
        end
    else
        return nil
    end

    local checksum_name = "nvim-" .. platform .. ext .. ".sha256sum"
    local checksum_url = nil

    for _, asset in ipairs(release.assets or {}) do
        if asset.name == checksum_name then
            checksum_url = asset.browser_download_url
            break
        end
    end

    if checksum_url == nil then
        return nil
    end

    local checksum_resp, checksum_err = http.get({ url = checksum_url })
    if checksum_err ~= nil or checksum_resp.status_code ~= 200 then
        return nil
    end

    -- Format is: "checksum  filename\n"
    return checksum_resp.body:match("^(%x+)")
end

function PLUGIN:Available(ctx)
    local http = require("http")
    local json = require("json")

    local resp, err = http.get({
        url = "https://api.github.com/repos/neovim/neovim/releases",
        headers = {
            ["Accept"] = "application/vnd.github.v3+json",
        },
    })

    if err ~= nil then
        error("Failed to fetch releases: " .. err)
    end
    if resp.status_code ~= 200 then
        error("Failed to fetch releases: HTTP " .. resp.status_code)
    end

    local releases = json.decode(resp.body)
    local result = {}

    for _, release in ipairs(releases) do
        if not release.draft then
            local tag = release.tag_name
            local version = tag
            local rolling = false
            local checksum = nil

            -- Handle special release tags
            if tag == "nightly" or tag == "stable" then
                -- Both nightly and stable are rolling releases (they point to moving targets)
                rolling = true
                -- Fetch checksum for rolling releases to detect updates
                checksum = get_release_checksum(release, http)
            else
                -- Strip 'v' prefix for versioned releases
                version = tag:gsub("^v", "")
            end

            table.insert(result, {
                version = version,
                note = release.name or "",
                rolling = rolling,
                checksum = checksum,
            })
        end
    end

    return result
end
