--- Post-installation hook

local file = require("file")
local http = require("http")

function PLUGIN:PostInstall(ctx)
    -- Get install path - it should be in sdkInfo
    local install_path = nil
    local version = nil

    -- Try to get path from sdkInfo
    if ctx.sdkInfo and ctx.sdkInfo.yarn then
        install_path = ctx.sdkInfo.yarn.path
        version = ctx.sdkInfo.yarn.version
    end

    -- Fallback to environment variable
    if not install_path then
        install_path = os.getenv("MISE_INSTALL_PATH")
    end
    if not version then
        version = os.getenv("MISE_INSTALL_VERSION") or ctx.version
    end

    if not install_path or not version then
        -- For v1, mise handles everything, so this is OK
        return {}
    end

    local major_version = tonumber(version:match("^(%d+)")) or 0

    if major_version >= 6 then
        -- Yarn ZPM (v6+) - mise extracted the platform package (a single
        -- native binary); move it to bin/ where EnvKeys expects it
        local bin_dir = file.join_path(install_path, "bin")
        os.execute('mkdir -p "' .. bin_dir .. '"')
        local binary = file.join_path(install_path, "yarn")
        if not file.exists(binary) then
            error("Yarn " .. version .. " package did not contain a yarn binary at " .. binary)
        end
        local target = file.join_path(bin_dir, "yarn")
        -- os.execute returns true (5.2+) or 0 (5.1) on success
        local function ok(r)
            return r == true or r == 0
        end
        if not ok(os.execute('mv "' .. binary .. '" "' .. target .. '"')) then
            error("Failed to move " .. binary .. " to " .. target)
        end
        if not ok(os.execute('chmod +x "' .. target .. '"')) then
            error("Failed to make " .. target .. " executable")
        end
    elseif major_version >= 2 then
        -- Yarn Berry (v2.x+) - download single JS file
        local yarn_url = "https://repo.yarnpkg.com/" .. version .. "/packages/yarnpkg-cli/bin/yarn.js"

        -- Detect Windows
        local is_windows = package.config:sub(1, 1) == "\\"

        -- Create bin directory. mkdir is a shell built-in on both cmd and sh,
        -- so it does not depend on any external binary being on PATH. Use
        -- file.join_path so the path uses the platform separator (mixing "/"
        -- into a Windows path can confuse cmd's mkdir). Skip if it already
        -- exists so a re-install does not print a spurious mkdir error.
        local bin_dir = file.join_path(install_path, "bin")
        if not file.exists(bin_dir) then
            if is_windows then
                os.execute('mkdir "' .. bin_dir .. '" 2>NUL')
            else
                os.execute('mkdir -p "' .. bin_dir .. '"')
            end
        end

        -- Download yarn.js via mise's built-in HTTP client rather than shelling
        -- out to curl/wget. The shell approach is unreliable on Windows: under
        -- mise's sanitized os.execute environment curl/wget are not guaranteed
        -- to be on PATH, and with stderr redirected the real error is lost.
        -- http.try_download_file uses mise's own client (with retry) and returns
        -- err (nil on success). Unlike download_file it reports failure
        -- as a return value instead of raising, so we can add the URL to the
        -- message. It must be called directly: it is an async host function,
        -- and wrapping it in pcall fails with "attempt to yield across a
        -- metamethod/C-call boundary".
        local yarn_js_file = file.join_path(bin_dir, "yarn.js")
        local _, err = http.try_download_file({ url = yarn_url, headers = {} }, yarn_js_file)
        if err ~= nil then
            error("Failed to download Yarn v2+ from " .. yarn_url .. ": " .. tostring(err))
        end

        -- Create wrapper script
        if is_windows then
            -- Create yarn.cmd wrapper for Windows
            local yarn_cmd = file.join_path(bin_dir, "yarn.cmd")
            local cmd_file = io.open(yarn_cmd, "w")
            if cmd_file then
                cmd_file:write("@echo off\n")
                cmd_file:write('node "%~dp0yarn.js" %*\n')
                cmd_file:close()
            end

            -- Also create yarn without extension for Git Bash
            local yarn_sh = file.join_path(bin_dir, "yarn")
            local sh_file = io.open(yarn_sh, "w")
            if sh_file then
                sh_file:write("#!/bin/sh\n")
                sh_file:write('exec node "$(dirname "$0")/yarn.js" "$@"\n')
                sh_file:close()
            end
        else
            -- Create shell wrapper for Unix
            local yarn_file = file.join_path(bin_dir, "yarn")
            local wrapper_file = io.open(yarn_file, "w")
            if wrapper_file then
                wrapper_file:write("#!/bin/sh\n")
                wrapper_file:write('exec node "$(dirname "$0")/yarn.js" "$@"\n')
                wrapper_file:close()
            end
            -- Make executable
            os.execute('chmod +x "' .. yarn_file .. '"')
        end
    end

    return {}
end

return PLUGIN
