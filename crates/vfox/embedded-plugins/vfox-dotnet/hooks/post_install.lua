--- Installs .NET SDK using Microsoft's official installer script
--- @param ctx table Context provided by vfox
function PLUGIN:PostInstall(ctx)
    local cmd = require("cmd")

    local sdkInfo = ctx.sdkInfo["dotnet"]
    local path = sdkInfo.path
    local version = sdkInfo.version

    -- Use correct path separator for OS
    local sep = RUNTIME.osType == "windows" and "\\" or "/"

    if RUNTIME.osType == "windows" then
        -- Windows: Use PowerShell script
        local scriptPath = path .. sep .. "dotnet-install.ps1"
        cmd.exec("powershell -ExecutionPolicy Bypass -File " .. scriptPath .. " -InstallDir " .. path .. " -Version " .. version .. " -NoPath")
        -- Clean up installer script
        os.remove(scriptPath)
    else
        -- Linux/macOS: Use bash script
        local scriptPath = path .. sep .. "dotnet-install.sh"
        -- Make script executable
        cmd.exec("chmod +x '" .. scriptPath .. "'")
        -- Run the installer
        cmd.exec("'" .. scriptPath .. "' --install-dir '" .. path .. "' --version '" .. version .. "' --no-path")
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
