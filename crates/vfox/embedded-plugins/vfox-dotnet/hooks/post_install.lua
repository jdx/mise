--- Installs .NET SDK using Microsoft's official installer script
--- @param ctx table Context provided by vfox
function PLUGIN:PostInstall(ctx)
    local sdkInfo = ctx.sdkInfo["dotnet"]
    local path = sdkInfo.path
    local version = sdkInfo.version

    -- Use correct path separator for OS
    local sep = RUNTIME.osType == "windows" and "\\" or "/"

    if RUNTIME.osType == "windows" then
        -- Windows: Use PowerShell script with -Command to avoid cmd.exe quote parsing issues
        -- Same pattern as vfox-aapt2: outer double quotes, inner single quotes
        local scriptPath = path .. sep .. "dotnet-install.ps1"
        local ps_cmd = string.format('powershell -ExecutionPolicy Bypass -Command "& \'%s\' -InstallDir \'%s\' -Version \'%s\' -NoPath"', scriptPath, path, version)
        local result = os.execute(ps_cmd)
        if not result then
            error("Failed to run dotnet-install.ps1")
        end
        -- Clean up installer script
        os.remove(scriptPath)
    else
        -- Linux/macOS: Use bash script
        local scriptPath = path .. sep .. "dotnet-install.sh"
        -- Make script executable
        os.execute("chmod +x '" .. scriptPath .. "'")
        -- Run the installer
        local result = os.execute("'" .. scriptPath .. "' --install-dir '" .. path .. "' --version '" .. version .. "' --no-path")
        if not result then
            error("Failed to run dotnet-install.sh")
        end
        -- Clean up installer script
        os.remove(scriptPath)
    end

    -- Verify installation
    local dotnetBin
    if RUNTIME.osType == "windows" then
        dotnetBin = path .. sep .. "dotnet.exe"
    else
        dotnetBin = path .. sep .. "dotnet"
    end

    local f = io.open(dotnetBin, "r")
    if f == nil then
        error("Installation failed: dotnet binary not found at " .. dotnetBin)
    end
    f:close()
end
