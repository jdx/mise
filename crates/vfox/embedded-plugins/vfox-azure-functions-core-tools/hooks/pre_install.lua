local http = require("http")

--- Returns pre-installed information, such as version number, download address, etc.
--- @param ctx {version: string}  (User-input version)
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    local version = ctx.version
    local os_type = RUNTIME.osType
    local arch_type = RUNTIME.archType

    -- Map OS type to Azure naming convention
    local os_name
    if os_type == "darwin" then
        os_name = "osx"
    elseif os_type == "windows" then
        os_name = "win"
    else
        os_name = "linux"
    end

    -- Map architecture
    local arch
    if arch_type == "amd64" or arch_type == "x86_64" then
        arch = "x64"
    elseif arch_type == "arm64" or arch_type == "aarch64" then
        arch = "arm64"
    elseif arch_type == "386" or arch_type == "i386" or arch_type == "i686" then
        arch = "x86"
    else
        arch = arch_type
    end

    local base_url = "https://github.com/Azure/azure-functions-core-tools/releases/download/"
    local filename = "Azure.Functions.Cli." .. os_name .. "-" .. arch .. "." .. version .. ".zip"
    local download_url = base_url .. version .. "/" .. filename

    -- Verify URL exists
    local resp = http.head({ url = download_url })
    if resp.status_code ~= 200 and resp.status_code ~= 302 then
        error("Download URL not found: " .. download_url .. " (status: " .. resp.status_code .. ")")
    end

    return {
        version = version,
        url = download_url,
    }
end
