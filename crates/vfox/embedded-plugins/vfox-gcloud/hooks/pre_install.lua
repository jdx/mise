--- Return the pre-installation information including download URL
--- @param ctx {version: string}  Context information (Version to install)
--- @return table Version information with download URL

local http = require("http")

-- GCS bucket configuration
local GCS_BUCKET = "cloud-sdk-release"
local BASE_URL = "https://storage.googleapis.com/" .. GCS_BUCKET

--- Get OS name for download URL
local function get_os_name()
    local os_type = RUNTIME.osType
    if os_type == "darwin" or os_type == "Darwin" then
        return "darwin"
    elseif os_type == "linux" or os_type == "Linux" then
        return "linux"
    elseif os_type == "windows" or os_type == "Windows" then
        return "windows"
    else
        error("Unsupported operating system: " .. os_type)
    end
end

--- Get architecture name for download URL
local function get_arch_name()
    local arch_type = RUNTIME.archType
    local os_name = get_os_name()

    if arch_type == "amd64" or arch_type == "x86_64" then
        return "x86_64"
    elseif arch_type == "arm64" or arch_type == "aarch64" then
        -- macOS uses arm64, Linux uses aarch64
        if os_name == "darwin" then
            return "arm"
        else
            return "arm"
        end
    elseif arch_type == "386" or arch_type == "x86" then
        return "x86"
    else
        error("Unsupported architecture: " .. arch_type)
    end
end

--- Get file extension based on OS
local function get_extension()
    local os_name = get_os_name()
    if os_name == "windows" then
        return ".zip"
    else
        return ".tar.gz"
    end
end

--- Build the download filename
local function build_filename(version)
    local os_name = get_os_name()
    local arch_name = get_arch_name()
    local ext = get_extension()

    -- Format: google-cloud-sdk-{version}-{os}-{arch}.tar.gz
    -- For bundled Python (recommended): google-cloud-sdk-{version}-{os}-{arch}-bundled-python.tar.gz
    return "google-cloud-sdk-" .. version .. "-" .. os_name .. "-" .. arch_name .. ext
end

--- Fetch SHA256 checksum for the file
local function fetch_checksum(filename)
    -- Google provides checksums at a .sha256 file
    local checksum_url = BASE_URL .. "/" .. filename .. ".sha256"

    local resp, err = http.get({
        url = checksum_url,
    })

    if err ~= nil or resp.status_code ~= 200 then
        -- Checksum file might not exist for all versions
        return nil
    end

    -- The file contains just the hash
    local hash = string.match(resp.body, "^(%x+)")
    return hash
end

function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    local filename = build_filename(version)
    local url = BASE_URL .. "/" .. filename

    -- Verify the file exists
    local resp, err = http.head({
        url = url,
    })

    if err ~= nil or resp.status_code == 404 then
        error("Version " .. version .. " not found for this platform. URL: " .. url)
    end

    -- Try to fetch checksum
    local checksum = fetch_checksum(filename)

    local result = {
        version = version,
        url = url,
    }

    if checksum then
        result.sha256 = checksum
    end

    return result
end
