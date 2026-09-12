--- Post-installation hook that creates a venv and installs pipenv.
--- @param ctx table
--- @field ctx.rootPath string SDK installation root path
--- @field ctx.sdkInfo table SDK info containing version
function PLUGIN:PostInstall(ctx)
    local rootPath = ctx.rootPath

    -- Extract version from rootPath (e.g., /path/to/installs/pipenv/2024.0.1)
    -- The version is the last component of the path
    local version = rootPath:match("([^/\\]+)$")
    if not version then
        error("Could not extract version from rootPath: " .. rootPath)
    end

    -- Find Python interpreter
    local python_cmd = nil
    local python_candidates = { "python3", "python" }

    for _, candidate in ipairs(python_candidates) do
        local check = os.execute(candidate .. " --version >/dev/null 2>&1")
        if check == 0 or check == true then
            -- Verify it's Python 3.7+
            local handle =
                io.popen(candidate .. ' -c "import sys; print(sys.version_info.major, sys.version_info.minor)"')
            if handle then
                local output = handle:read("*a")
                handle:close()
                local major, minor = output:match("(%d+)%s+(%d+)")
                if major and minor then
                    major = tonumber(major)
                    minor = tonumber(minor)
                    if major >= 3 and minor >= 7 then
                        python_cmd = candidate
                        break
                    end
                end
            end
        end
    end

    if not python_cmd then
        error("Python 3.7+ is required but not found in PATH")
    end

    -- Create virtual environment
    local venv_cmd = python_cmd .. ' -m venv --copies "' .. rootPath .. '"'
    local result = os.execute(venv_cmd)
    if result ~= 0 and result ~= true then
        error("Failed to create virtual environment")
    end

    -- Determine the path separator and bin directory based on OS
    local bin_dir = "bin"
    local path_sep = "/"
    local script_ext = ""
    local activate_script = "activate"

    if RUNTIME.osType == "windows" then
        bin_dir = "Scripts"
        path_sep = "\\"
        script_ext = ".bat"
        activate_script = "activate.bat"
    end

    local venv_bin = rootPath .. path_sep .. bin_dir
    local pip_cmd = venv_bin .. path_sep .. "pip"

    -- Install pipenv inside virtual environment
    local install_cmd = '"' .. pip_cmd .. '" install --quiet pipenv==' .. version
    result = os.execute(install_cmd)
    if result ~= 0 and result ~= true then
        error("Failed to install pipenv==" .. version)
    end

    -- Create wrapper scripts directory
    local wrapper_dir = rootPath .. path_sep .. "wrapper_bin"
    os.execute('mkdir -p "' .. wrapper_dir .. '"')

    -- Create wrapper scripts for pipenv executables
    local executables = { "pipenv", "pipenv-resolver" }

    if RUNTIME.osType == "windows" then
        -- Windows batch wrapper
        for _, exe in ipairs(executables) do
            local wrapper_path = wrapper_dir .. path_sep .. exe .. ".cmd"
            local wrapper_file = io.open(wrapper_path, "w")
            if wrapper_file then
                wrapper_file:write("@echo off\r\n")
                wrapper_file:write('call "' .. venv_bin .. path_sep .. activate_script .. '"\r\n')
                wrapper_file:write("set PIPENV_IGNORE_VIRTUALENVS=1\r\n")
                wrapper_file:write('"' .. venv_bin .. path_sep .. exe .. '" %*\r\n')
                wrapper_file:close()
            end
        end
    else
        -- Unix shell wrapper
        for _, exe in ipairs(executables) do
            local wrapper_path = wrapper_dir .. path_sep .. exe
            local wrapper_file = io.open(wrapper_path, "w")
            if wrapper_file then
                wrapper_file:write("#!/usr/bin/env bash\n")
                wrapper_file:write('source "' .. venv_bin .. "/" .. activate_script .. '"\n')
                wrapper_file:write('PIPENV_IGNORE_VIRTUALENVS=1 "' .. venv_bin .. "/" .. exe .. '" "$@"\n')
                wrapper_file:close()
                os.execute('chmod +x "' .. wrapper_path .. '"')
            end
        end
    end
end
