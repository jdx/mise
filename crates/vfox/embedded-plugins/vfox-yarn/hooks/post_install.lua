--- Post-installation hook

local function download_file(url, output_path)
    -- Detect Windows
    local is_windows = package.config:sub(1,1) == '\\'
    local stderr_redirect = is_windows and " 2>NUL" or " 2>/dev/null"
    
    -- Try curl first (more likely to be available on Windows via Git Bash)
    local curl_cmd = "curl -sSL -o " .. output_path .. " " .. url .. stderr_redirect
    local wget_cmd = "wget -q -O " .. output_path .. " " .. url .. stderr_redirect
    
    if os.execute(curl_cmd) == 0 then
        return true
    elseif os.execute(wget_cmd) == 0 then
        return true
    end
    return false
end

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
    
    local major_version = string.sub(version, 1, 1)
    
    if major_version ~= "1" then
        -- Yarn Berry (v2.x+) - download single JS file
        local yarn_url = "https://repo.yarnpkg.com/" .. version .. "/packages/yarnpkg-cli/bin/yarn.js"
        
        -- Detect Windows
        local is_windows = package.config:sub(1,1) == '\\'
        
        -- Create bin directory (cross-platform)
        local bin_dir = install_path .. "/bin"
        if is_windows then
            os.execute('mkdir "' .. bin_dir .. '" 2>NUL')
        else
            os.execute("mkdir -p " .. bin_dir)
        end
        
        -- Download yarn.js
        local yarn_js_file = bin_dir .. "/yarn.js"
        if not download_file(yarn_url, yarn_js_file) then
            error("Failed to download Yarn v2+")
        end
        
        -- Create wrapper script
        if is_windows then
            -- Create yarn.cmd wrapper for Windows
            local yarn_cmd = bin_dir .. "/yarn.cmd"
            local cmd_file = io.open(yarn_cmd, "w")
            if cmd_file then
                cmd_file:write("@echo off\n")
                cmd_file:write('node "%~dp0yarn.js" %*\n')
                cmd_file:close()
            end
            
            -- Also create yarn without extension for Git Bash
            local yarn_sh = bin_dir .. "/yarn"
            local sh_file = io.open(yarn_sh, "w")
            if sh_file then
                sh_file:write("#!/bin/sh\n")
                sh_file:write('exec node "$(dirname "$0")/yarn.js" "$@"\n')
                sh_file:close()
            end
        else
            -- Create shell wrapper for Unix
            local yarn_file = bin_dir .. "/yarn"
            local wrapper_file = io.open(yarn_file, "w")
            if wrapper_file then
                wrapper_file:write("#!/bin/sh\n")
                wrapper_file:write('exec node "$(dirname "$0")/yarn.js" "$@"\n')
                wrapper_file:close()
            end
            -- Make executable
            os.execute("chmod +x " .. yarn_file)
        end
    end
    
    return {}
end

return PLUGIN