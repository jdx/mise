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

--- mise caches the result of EnvKeys; name the files it depends on so mise
--- runs the hook again when one changes. mise.cache is a mise addition that
--- the vfox CLI and older mise releases lack.
local function watch_files(paths)
    local ok, cache = pcall(require, "mise.cache")
    if ok then
        cache.watch_files(paths)
    end
end

--- Have mise run EnvKeys again every time, for a result that depends on
--- something it can't watch.
local function disable_cache()
    local ok, cache = pcall(require, "mise.cache")
    if ok then
        cache.disable()
    end
end

--- Whether a path is absolute: `/x`, `\x`, or a Windows drive path like `C:\x`.
local function is_absolute(path)
    return path:match("^[/\\]") ~= nil or path:match("^%a:[/\\]") ~= nil
end

--- The absolute path of the configured pyproject, or nil when there is none.
--- A relative path means relative to the project, so it is resolved against
--- the project root only. Without one there is no project to resolve it
--- against: the current directory is not it (mise may run this hook from
--- another directory, and runs the hook's commands from the home directory),
--- so skip virtualenv activation rather than check one directory and run
--- Poetry in another.
local function resolve_pyproject(pyproject, project_root)
    if is_absolute(pyproject) then
        return pyproject
    end
    if not project_root or project_root == "" then
        return nil
    end
    -- A backslash separates directories only on Windows; elsewhere it can be
    -- part of a directory name.
    local trailing = RUNTIME.osType == "windows" and "[/\\]$" or "/$"
    local sep = project_root:match(trailing) and "" or "/"
    return project_root .. sep .. pyproject
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

    pyproject = resolve_pyproject(pyproject, os.getenv("MISE_PROJECT_ROOT"))
    if not pyproject then
        return env_keys
    end
    watch_files({ pyproject })

    -- Check if pyproject.toml exists
    local f = io.open(pyproject, "r")
    if not f then
        return env_keys
    end
    f:close()

    -- resolve_pyproject returns an absolute path, so it always has a directory
    -- part, and every command below runs in that same directory.
    local pyproject_dir = pyproject:match("^(.*[/\\])")
    watch_files({ pyproject_dir .. "uv.lock", pyproject_dir .. "poetry.lock" })

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
        disable_cache()
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
            -- Poetry couldn't make one. Try again next time rather than keep
            -- this result, since nothing here would change when it can.
            disable_cache()
            return env_keys
        end

        -- Auto-install if enabled
        local auto_install = os.getenv("MISE_POETRY_AUTO_INSTALL")
        if auto_install == "1" or auto_install == "true" then
            os.execute("cd '" .. pyproject_dir .. "' && " .. path_prefix .. "'" .. poetry_bin .. "' install 2>&1")
        end
    end

    -- `poetry env use` switches virtualenvs by recording the choice in
    -- envs.toml, beside the virtualenvs Poetry manages.
    local watched = { venv_path }
    local venvs_dir = venv_path:match("^(.*)[/\\][^/\\]+$")
    if venvs_dir then
        table.insert(watched, venvs_dir .. "/envs.toml")
    end
    watch_files(watched)

    -- Set virtualenv environment variables
    table.insert(env_keys, { key = "POETRY_ACTIVE", value = "1" })
    table.insert(env_keys, { key = "VIRTUAL_ENV", value = venv_path })
    table.insert(
        env_keys,
        { key = "MISE_ADD_PATH", value = venv_path .. (RUNTIME.osType == "windows" and "\\Scripts" or "/bin") }
    )

    return env_keys
end
