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
    local parent_path = file.join_path(root_path, "cmdline-tools")
    local target_path = file.join_path(root_path, "cmdline-tools", version)
    local is_windows = RUNTIME.osType == "windows"

    if is_windows then
        local cmd = require("cmd")
        local system_root = os.getenv("SystemRoot") or "C:\\Windows"
        local function run_powershell(command, env)
            cmd.exec("powershell.exe -NoLogo -NoProfile -NonInteractive -Command " .. command, {
                cwd = system_root,
                env = env,
            })
        end

        -- Remove stale temp from a previous failed install
        if file.exists(temp_path) then
            run_powershell("Remove-Item -LiteralPath $env:TEMP_PATH -Recurse -Force", {
                TEMP_PATH = temp_path,
            })
        end

        -- Move current rootPath to a temporary location
        run_powershell("Move-Item -LiteralPath $env:ROOT_PATH -Destination $env:TEMP_PATH", {
            ROOT_PATH = root_path,
            TEMP_PATH = temp_path,
        })

        -- Recreate parent directory structure
        run_powershell("New-Item -ItemType Directory -Force -Path $env:PARENT_PATH", {
            PARENT_PATH = parent_path,
        })

        -- Move temp to target (renames the directory)
        run_powershell("Move-Item -LiteralPath $env:TEMP_PATH -Destination $env:TARGET_PATH", {
            TEMP_PATH = temp_path,
            TARGET_PATH = target_path,
        })
    else
        -- Remove stale temp from a previous failed install
        os.execute('rm -rf "' .. temp_path .. '"')

        -- Move current rootPath to temp location
        os.execute('mv "' .. root_path .. '" "' .. temp_path .. '"')

        -- Recreate parent directory structure
        os.execute('mkdir -p "' .. parent_path .. '"')

        -- Move temp to target (renames the directory)
        os.execute('mv "' .. temp_path .. '" "' .. target_path .. '"')
    end

    -- Verify installation
    local sdkmanager_name = is_windows and "sdkmanager.bat" or "sdkmanager"
    local sdkmanager_path = file.join_path(target_path, "bin", sdkmanager_name)
    if not file.exists(sdkmanager_path) then
        error("Installation verification failed: sdkmanager not found at " .. sdkmanager_path)
    end

    -- Make sure binaries are executable (for Unix systems)
    if not is_windows then
        os.execute('chmod +x "' .. file.join_path(target_path, "bin") .. '"/* 2>/dev/null || true')
    end
end
