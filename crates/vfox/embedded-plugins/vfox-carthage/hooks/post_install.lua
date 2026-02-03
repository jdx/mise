--- Extension point, called after PreInstall, extracts carthage from .pkg
--- @param ctx table
--- @field ctx.rootPath string SDK installation directory
function PLUGIN:PostInstall(ctx)
    local rootPath = ctx.rootPath

    -- Check if we're on macOS (carthage is macOS only)
    if RUNTIME.osType ~= "darwin" then
        error("Carthage is only available for macOS")
    end

    -- Find the downloaded .pkg file
    local find_cmd = string.format('ls "%s"/*.pkg 2>/dev/null | head -1', rootPath)
    local handle = io.popen(find_cmd)
    if not handle then
        error("Failed to find .pkg file")
    end
    local pkg_path = handle:read("*l")
    handle:close()

    if not pkg_path or pkg_path == "" then
        error("Could not find Carthage.pkg in " .. rootPath)
    end

    -- Create extraction directory
    local expand_dir = rootPath .. "/expanded"
    os.execute(string.format('mkdir -p "%s"', expand_dir))

    -- Expand the .pkg file using pkgutil
    local expand_cmd = string.format('pkgutil --expand "%s" "%s"', pkg_path, expand_dir)
    local result = os.execute(expand_cmd)
    if not result then
        error("Failed to expand .pkg file")
    end

    -- Find the payload and extract it
    -- The payload is typically in CarthageApp.pkg/Payload
    local payload_dir = expand_dir .. "/CarthageApp.pkg"
    local extract_cmd = string.format('cd "%s" && cat Payload | gunzip | cpio -id 2>/dev/null', payload_dir)
    result = os.execute(extract_cmd)
    if not result then
        error("Failed to extract payload from .pkg")
    end

    -- Create bin directory and move carthage binary
    local binDir = rootPath .. "/bin"
    os.execute(string.format('mkdir -p "%s"', binDir))

    local move_cmd = string.format('mv "%s/usr/local/bin/carthage" "%s/carthage"', payload_dir, binDir)
    result = os.execute(move_cmd)
    if not result then
        error("Failed to move carthage binary to bin directory")
    end

    -- Make executable and remove quarantine
    os.execute(string.format('chmod +x "%s/carthage"', binDir))
    os.execute(string.format('xattr -d com.apple.quarantine "%s/carthage" 2>/dev/null || true', binDir))

    -- Clean up
    os.execute(string.format('rm -rf "%s"', expand_dir))
    os.execute(string.format('rm -f "%s"', pkg_path))

    print("Carthage installed successfully!")
end
