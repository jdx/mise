--- Returns information about the version to install
--- Constructs the download URL based on OS and architecture

function PLUGIN:PreInstall(ctx)
    local http = require("http")
    local json = require("json")

    local version = ctx.version
    local os_type = RUNTIME.osType
    local arch_type = RUNTIME.archType

    -- Map OS/arch to Neovim asset naming
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
        error("Unsupported OS: " .. os_type)
    end

    -- Determine the tag to fetch
    local tag = version
    if version ~= "nightly" and version ~= "stable" then
        tag = "v" .. version
    end

    -- Fetch the specific release
    local resp, err = http.get({
        url = "https://api.github.com/repos/neovim/neovim/releases/tags/" .. tag,
        headers = {
            ["Accept"] = "application/vnd.github.v3+json",
        },
    })

    if err ~= nil then
        error("Failed to fetch release: " .. err)
    end
    if resp.status_code ~= 200 then
        error("Failed to fetch release " .. tag .. ": HTTP " .. resp.status_code)
    end

    local release = json.decode(resp.body)

    -- Find the right asset and its checksum file
    local asset_name = "nvim-" .. platform .. ext
    local checksum_name = asset_name .. ".sha256sum"
    local download_url = nil
    local checksum_url = nil

    for _, asset in ipairs(release.assets) do
        if asset.name == asset_name then
            download_url = asset.browser_download_url
        elseif asset.name == checksum_name then
            checksum_url = asset.browser_download_url
        end
    end

    if download_url == nil then
        error("Could not find asset " .. asset_name .. " for release " .. tag)
    end

    -- Fetch the sha256 checksum if available
    local sha256 = nil
    if checksum_url ~= nil then
        local checksum_resp, checksum_err = http.get({
            url = checksum_url,
        })
        if checksum_err == nil and checksum_resp.status_code == 200 then
            -- Format is: "checksum  filename\n"
            -- Extract just the checksum (first field)
            sha256 = checksum_resp.body:match("^(%x+)")
        end
    end

    return {
        version = version,
        url = download_url,
        sha256 = sha256,
    }
end
