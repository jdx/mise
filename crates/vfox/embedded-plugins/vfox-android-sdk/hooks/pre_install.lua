--- Returns some pre-installed information, such as version number, download address, etc.
--- @param ctx {version: string}  (User-input version)
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    local http = require("http")
    local env = require("env")

    local version = ctx.version

    -- Determine OS (from global RUNTIME object)
    local os_type = RUNTIME.osType
    local android_sdk_os
    if os_type == "darwin" then
        android_sdk_os = "macosx"
    elseif os_type == "linux" then
        android_sdk_os = "linux"
    elseif os_type == "windows" then
        android_sdk_os = "windows"
    else
        error("Unsupported OS type: " .. os_type)
    end

    -- Determine architecture (from global RUNTIME object)
    local arch_type = RUNTIME.archType
    local android_sdk_arch
    if arch_type == "amd64" or arch_type == "x86_64" then
        android_sdk_arch = "x64"
    elseif arch_type == "arm64" or arch_type == "aarch64" then
        android_sdk_arch = "aarch64"
    else
        error("Unsupported architecture: " .. arch_type)
    end

    local base_url = env.ANDROID_SDK_MIRROR_URL or "https://dl.google.com/android/repository"
    local metadata_url = base_url .. "/repository2-3.xml"

    local resp = http.get({ url = metadata_url })
    if resp.status_code ~= 200 then
        error("Failed to fetch Android SDK metadata: HTTP " .. resp.status_code)
    end

    -- Parse the XML to find the matching package
    local package_pattern = 'remotePackage%s+path="cmdline%-tools;'
        .. version:gsub("%-", "%%-")
        .. '".-</remotePackage>'
    local package_block = resp.body:match(package_pattern)

    if not package_block then
        error("Version " .. version .. " not found in Android SDK repository")
    end

    -- Find archives block
    local archives_block = package_block:match("<archives>(.-)</archives>")
    if not archives_block then
        error("No archives found for version " .. version)
    end

    -- Find matching archive for our OS/arch
    local best_archive = nil

    for archive in archives_block:gmatch("<archive>(.-)</archive>") do
        local host_os = archive:match("<host%-os>([^<]+)</host%-os>")
        local host_arch = archive:match("<host%-arch>([^<]+)</host%-arch>")

        -- Match if OS matches and either no arch specified or arch matches
        if host_os == android_sdk_os then
            if host_arch == nil or host_arch == android_sdk_arch then
                best_archive = archive
                -- Prefer exact arch match
                if host_arch == android_sdk_arch then
                    break
                end
            end
        end
    end

    if not best_archive then
        error("No archive found for " .. android_sdk_os .. ":" .. android_sdk_arch)
    end

    -- Extract complete block (contains url, size, checksum)
    local complete_block = best_archive:match("<complete>(.-)</complete>")
    if not complete_block then
        error("No complete block found in archive")
    end

    -- Extract URL
    local url = complete_block:match("<url>([^<]+)</url>")
    if not url then
        error("No URL found in archive")
    end

    -- Make URL absolute if it's relative
    if not url:match("^https?://") then
        url = base_url .. "/" .. url
    end

    local result = {
        version = version,
        url = url,
    }

    -- Note: Android SDK only provides sha1 checksums, which vfox doesn't support yet
    -- Checksum verification is skipped for now

    return result
end
