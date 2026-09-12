--- Returns environment keys for Poetry
--- Poetry installs its binary to $POETRY_HOME/bin
--- Also handles virtualenv activation based on pyproject.toml

--- Helper function to get mise Python bin directory
--- Returns the PATH prefix to use, or empty string if not found
local function get_mise_python_path_prefix()
    local handle = io.popen("mise which python3 2>/dev/null")
    if not handle then
        return ""
    end
    local python_path = handle:read("*l")
    handle:close()

    if not python_path or python_path == "" then
        return ""
    end

    -- Extract the bin directory from the python path
    local bin_dir = python_path:match("(.*/)")
    if bin_dir then
        return "PATH='" .. bin_dir .. ":$PATH' "
    end
    return ""
end

function PLUGIN:EnvKeys(ctx)
    local file = require("file")
    local bin_path = file.join_path(ctx.path, "bin")

    local env_keys = {
        {
            key = "PATH",
            value = bin_path,
        },
        {
            key = "POETRY_HOME",
            value = ctx.path,
        },
    }

    -- Check for pyproject option from tool configuration
    local pyproject = ctx.options and ctx.options.pyproject
    if not pyproject or pyproject == "" then
        return env_keys
    end

    -- Resolve relative path against project root
    local project_root = os.getenv("MISE_PROJECT_ROOT")
    if project_root and pyproject:sub(1, 1) ~= "/" then
        pyproject = project_root .. "/" .. pyproject
    end

    -- Check if pyproject.toml exists
    local f = io.open(pyproject, "r")
    if not f then
        return env_keys
    end
    f:close()

    local pyproject_dir = pyproject:match("(.*/)")
    if not pyproject_dir then
        pyproject_dir = "."
    end

    -- Check for uv.lock - if present, let uv manage the venv
    local uv_lock = io.open(pyproject_dir .. "uv.lock", "r")
    if uv_lock then
        uv_lock:close()
        return env_keys
    end

    -- Check MISE_POETRY_VENV_AUTO setting
    local venv_auto = os.getenv("MISE_POETRY_VENV_AUTO")
    if venv_auto == "1" or venv_auto == "true" then
        -- Only activate if poetry.lock exists
        local lock_file = io.open(pyproject_dir .. "poetry.lock", "r")
        if not lock_file then
            return env_keys
        end
        lock_file:close()
    end

    -- Get mise Python path prefix to ensure poetry uses the correct Python
    local path_prefix = get_mise_python_path_prefix()

    -- Get the virtualenv path from poetry
    local poetry_bin = ctx.path .. "/bin/poetry"
    local handle = io.popen(
        "cd '" .. pyproject_dir .. "' && " .. path_prefix .. "'" .. poetry_bin .. "' env info --path 2>/dev/null"
    )
    if not handle then
        return env_keys
    end

    local venv_path = handle:read("*l")
    handle:close()

    if not venv_path or venv_path == "" then
        -- Try to create the virtualenv with mise's Python in PATH
        os.execute("cd '" .. pyproject_dir .. "' && " .. path_prefix .. "'" .. poetry_bin .. "' run true 2>/dev/null")

        -- Try again to get the path
        handle = io.popen(
            "cd '" .. pyproject_dir .. "' && " .. path_prefix .. "'" .. poetry_bin .. "' env info --path 2>/dev/null"
        )
        if handle then
            venv_path = handle:read("*l")
            handle:close()
        end

        if not venv_path or venv_path == "" then
            return env_keys
        end

        -- Auto-install if enabled
        local auto_install = os.getenv("MISE_POETRY_AUTO_INSTALL")
        if auto_install == "1" or auto_install == "true" then
            os.execute("cd '" .. pyproject_dir .. "' && " .. path_prefix .. "'" .. poetry_bin .. "' install 2>&1")
        end
    end

    -- Set virtualenv environment variables
    table.insert(env_keys, { key = "POETRY_ACTIVE", value = "1" })
    table.insert(env_keys, { key = "VIRTUAL_ENV", value = venv_path })
    table.insert(env_keys, { key = "MISE_ADD_PATH", value = venv_path .. "/bin" })

    return env_keys
end
