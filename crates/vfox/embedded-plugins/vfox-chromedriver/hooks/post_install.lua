--- Installs chromedriver by moving binary to bin directory
--- @param ctx table Context provided by vfox
function PLUGIN:PostInstall(ctx)
    local cmd = require("cmd")

    local sdkInfo = ctx.sdkInfo["chromedriver"]
    local path = sdkInfo.path
    local version = sdkInfo.version

    -- Create bin directory
    cmd.exec("mkdir -p '" .. path .. "/bin'")

    -- Determine platform suffix for the extracted directory using RUNTIME global
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
    end

    -- The zip extracts to chromedriver-{platform}/
    local srcDir = path .. "/chromedriver-" .. platform

    -- Check if srcDir exists, if not try without subdirectory
    local file = io.open(srcDir .. "/chromedriver", "r")
    if file then
        file:close()
    else
        -- Try direct path (files extracted directly)
        srcDir = path
    end

    -- Copy chromedriver binary
    if osType == "windows" then
        cmd.exec("cp -f '" .. srcDir .. "/chromedriver.exe' '" .. path .. "/bin/'")
    else
        cmd.exec("cp -f '" .. srcDir .. "/chromedriver' '" .. path .. "/bin/'")
        cmd.exec("chmod +x '" .. path .. "/bin/chromedriver'")
    end

    -- Verify installation
    local binaryName = osType == "windows" and "chromedriver.exe" or "chromedriver"
    local verifyFile = io.open(path .. "/bin/" .. binaryName, "r")
    if verifyFile then
        verifyFile:close()
    else
        error("Failed to install chromedriver - binary not found at " .. path .. "/bin/" .. binaryName)
    end
end
