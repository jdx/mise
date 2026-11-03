--- Each SDK may have different environment variable configurations.
--- This allows plugins to define custom environment variables (including PATH settings)
--- @param ctx {path: string}  Context information (SDK installation directory)
function PLUGIN:EnvKeys(ctx)
    local version_path = ctx.path
    local result = {}

    -- Add wrapper_bin to PATH for pipenv command
    if RUNTIME.osType == "windows" then
        table.insert(result, {
            key = "PATH",
            value = version_path .. "\\wrapper_bin",
        })
    else
        table.insert(result, {
            key = "PATH",
            value = version_path .. "/wrapper_bin",
        })
    end

    -- Check for Pipfile in current directory to auto-activate virtualenv
    local cwd = os.getenv("PWD")
    if not cwd then
        -- Fallback: try to get cwd via shell command
        local handle = io.popen("pwd 2>/dev/null")
        if handle then
            cwd = handle:read("*a"):gsub("%s+$", "")
            handle:close()
        end
    end

    if cwd and cwd ~= "" then
        local pipfile_path = cwd .. "/Pipfile"
        -- mise caches this result, so name what it depends on: the Pipfile, the
        -- lock `pipenv install` writes, and the virtualenv once found.
        local watched = { pipfile_path, cwd .. "/Pipfile.lock" }
        local f = io.open(pipfile_path, "r")
        if f then
            f:close()

            -- `pipenv install` or `pipenv sync` can create the virtualenv without
            -- changing the lock. It goes in the project's .venv, or in a directory
            -- named after the project in WORKON_HOME, which gains an entry then.
            table.insert(watched, cwd .. "/.venv")
            local workon_home = os.getenv("WORKON_HOME")
            if not workon_home or workon_home == "" then
                local home = os.getenv("HOME") or os.getenv("USERPROFILE") or ""
                if RUNTIME.osType == "windows" then
                    workon_home = home .. "\\.virtualenvs"
                else
                    local data_home = os.getenv("XDG_DATA_HOME")
                    if not data_home or data_home == "" then
                        data_home = home .. "/.local/share"
                    end
                    workon_home = data_home .. "/virtualenvs"
                end
            end
            table.insert(watched, workon_home)

            -- Pipfile exists, try to get the virtualenv path
            local pipenv_cmd = version_path
            if RUNTIME.osType == "windows" then
                pipenv_cmd = pipenv_cmd .. "\\wrapper_bin\\pipenv"
            else
                pipenv_cmd = pipenv_cmd .. "/wrapper_bin/pipenv"
            end

            local venv_handle =
                io.popen('PIPENV_PIPFILE="' .. pipfile_path .. '" "' .. pipenv_cmd .. '" --venv 2>/dev/null')
            if venv_handle then
                local venv_path = venv_handle:read("*a"):gsub("%s+$", "")
                venv_handle:close()

                if venv_path and venv_path ~= "" then
                    -- Verify the venv exists
                    local venv_bin = venv_path .. "/bin"
                    local test_file = io.open(venv_bin .. "/python", "r")
                    if test_file then
                        test_file:close()
                        table.insert(watched, venv_path)

                        -- Add virtualenv activation
                        table.insert(result, {
                            key = "VIRTUAL_ENV",
                            value = venv_path,
                        })
                        table.insert(result, {
                            key = "PIPENV_ACTIVE",
                            value = "1",
                        })
                        table.insert(result, {
                            key = "PATH",
                            value = venv_bin,
                        })
                    end
                end
            end
        end

        -- mise.cache is a mise addition; the vfox CLI and older mise lack it.
        local ok, cache = pcall(require, "mise.cache")
        if ok then
            cache.watch_files(watched)
        end
    end

    return result
end
