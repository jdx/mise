--- Installs chromedriver by moving binary to bin directory
--- @param ctx table Context provided by vfox
function PLUGIN:PostInstall(ctx)
    local cmd = require("cmd")
    local file = require("file")

    local sdkInfo = ctx.sdkInfo["chromedriver"]
    local path = sdkInfo.path

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
    local binaryName = osType == "windows" and "chromedriver.exe" or "chromedriver"
    local srcDir = file.join_path(path, "chromedriver-" .. platform)
    local srcPath = file.join_path(srcDir, binaryName)

    -- Check if srcDir exists, if not try without subdirectory
    if not file.exists(srcPath) then
        -- Try direct path (files extracted directly)
        srcPath = file.join_path(path, binaryName)
    end

    local binDir = file.join_path(path, "bin")
    local destPath = file.join_path(binDir, binaryName)

    -- Copy chromedriver binary using the platform's native shell. Passing paths
    -- through environment variables avoids shell-quoting issues.
    if osType == "windows" then
        cmd.exec(
            "powershell.exe -NoLogo -NoProfile -NonInteractive -Command "
                .. "$null = New-Item -ItemType Directory -Force -Path $env:BIN_DIR; "
                .. "Copy-Item -LiteralPath $env:SRC_PATH -Destination $env:DEST_PATH -Force",
            {
                env = {
                    BIN_DIR = binDir,
                    SRC_PATH = srcPath,
                    DEST_PATH = destPath,
                },
            }
        )
    else
        local env = {
            BIN_DIR = binDir,
            SRC_PATH = srcPath,
            DEST_PATH = destPath,
        }
        cmd.exec('mkdir -p "$BIN_DIR"', { env = env })
        cmd.exec('cp -f "$SRC_PATH" "$DEST_PATH"', { env = env })
        cmd.exec('chmod +x "$DEST_PATH"', { env = env })
    end

    -- Verify installation
    if not file.exists(destPath) then
        error("Failed to install chromedriver - binary not found at " .. destPath)
    end
end
