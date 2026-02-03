--- Called after the tool is installed.
--- Used to set up the correct directory structure for Android SDK.
--- @param ctx table
--- @field ctx.rootPath string The installation root path
--- @field ctx.sdkInfo table SDK information including version
function PLUGIN:PostInstall(ctx)
    local file = require("file")

    local root_path = ctx.rootPath

    -- Get the version from sdkInfo
    local version = nil
    for _, info in pairs(ctx.sdkInfo) do
        version = info.version
        break
    end

    if not version then
        error("Could not determine version from sdkInfo")
    end

    -- vfox extracts cmdline-tools contents directly to rootPath
    -- But Android SDK expects: ANDROID_HOME/cmdline-tools/VERSION/bin/sdkmanager
    -- So we need to reorganize: move rootPath/* to rootPath/cmdline-tools/VERSION/

    local temp_path = root_path .. "-temp"
    local target_path = file.join_path(root_path, "cmdline-tools", version)

    -- Move current rootPath to temp location
    os.execute("mv " .. root_path .. " " .. temp_path)

    -- Recreate rootPath with proper structure
    os.execute("mkdir -p " .. target_path)

    -- Move contents from temp to target
    os.execute("mv " .. temp_path .. "/* " .. target_path .. "/")

    -- Clean up temp
    os.execute("rm -rf " .. temp_path)

    -- Verify installation
    local sdkmanager_path = file.join_path(target_path, "bin", "sdkmanager")
    if not file.exists(sdkmanager_path) then
        error("Installation verification failed: sdkmanager not found at " .. sdkmanager_path)
    end

    -- Make sure binaries are executable (for Unix systems)
    os.execute("chmod +x " .. file.join_path(target_path, "bin", "*") .. " 2>/dev/null || true")
end
