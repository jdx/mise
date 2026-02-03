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
        local f = io.open(pipfile_path, "r")
        if f then
            f:close()

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
    end

    return result
end
