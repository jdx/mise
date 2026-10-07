local os = require("os")

function PLUGIN:PostInstall(ctx)
    local sdkInfo = ctx.sdkInfo and ctx.sdkInfo[PLUGIN.name] or ctx
    local mainPath = sdkInfo.path
    local version = sdkInfo.version

    local extracted_dir = "emsdk-main"
    local extracted_path = mainPath .. "/" .. extracted_dir

    local f = io.open(extracted_path .. "/emsdk", "r")
    if f then
        f:close()
        if RUNTIME.osType == "windows" then
            os.execute('xcopy "' .. extracted_path .. '\\*" "' .. mainPath .. '\\" /E /I /Y 2>nul')
            os.execute('rmdir /S /Q "' .. extracted_path .. '" 2>nul')
        else
            os.execute('sh -c \'mv "' .. extracted_path .. '"/* "' .. mainPath .. '/" 2>/dev/null || true\'')
            os.execute('sh -c \'mv "' .. extracted_path .. '/.[!.]* "' .. mainPath .. '/" 2>/dev/null || true\'')
            os.execute('rmdir "' .. extracted_path .. '" 2>/dev/null || true')
        end
    end

    local emsdk_cmd
    if RUNTIME.osType == "windows" then
        emsdk_cmd = "emsdk.bat"
    else
        emsdk_cmd = "./emsdk"
    end

    local install_version = version
    local ret
    if RUNTIME.osType == "windows" then
        ret = os.execute('cmd /c "cd /d ' .. mainPath .. ' && ' .. emsdk_cmd .. ' install ' .. install_version .. '"')
    else
        os.execute('chmod +x "' .. mainPath .. '/emsdk"')
        ret = os.execute('cd "' .. mainPath .. '" && ' .. emsdk_cmd .. ' install ' .. install_version)
    end

    if ret ~= true and ret ~= 0 then
        error("Failed to install emscripten version " .. install_version)
    end

    if RUNTIME.osType == "windows" then
        ret = os.execute('cmd /c "cd /d ' .. mainPath .. ' && ' .. emsdk_cmd .. ' activate ' .. install_version .. '"')
    else
        ret = os.execute('cd "' .. mainPath .. '" && ' .. emsdk_cmd .. ' activate ' .. install_version)
    end

    if ret ~= true and ret ~= 0 then
        error("Failed to activate emscripten version " .. install_version)
    end
end
