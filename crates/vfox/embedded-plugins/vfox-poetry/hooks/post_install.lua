--- Post-installation hook for Poetry
--- Runs the installer script with the correct version

local function shell_quote(value)
    return "'" .. tostring(value):gsub("'", "'\\''") .. "'"
end

--- Installs Poetry on Windows without relying on bash, sed, chmod or rm
local function install_windows(install_path, version)
    local http = require("http")

    local resp, err = http.get({ url = "https://install.python-poetry.org" })
    if err ~= nil or resp.status_code ~= 200 then
        error("Failed to download the Poetry installer: " .. (err or ("HTTP " .. resp.status_code)))
    end

    -- Unlike the unix path, keep the installer's symlinks=False: on Windows that gives the
    -- venv a launcher redirector, while symlinks would leave a python.exe that cannot find
    -- its DLLs unless the base Python is on PATH.
    local installer = resp.body

    local script_path = install_path .. "\\install-poetry.py"
    local f = io.open(script_path, "wb")
    if not f then
        error("Failed to create the Poetry installer script")
    end
    f:write(installer)
    f:close()

    local result = os.execute(
        string.format('set "POETRY_HOME=%s"&& python "%s" --version "%s"', install_path, script_path, version)
    )
    os.remove(script_path)
    if result ~= 0 and result ~= true then
        error("Poetry installation failed")
    end

    -- Poetry >= 2.0.0 uses virtualenvs.use-poetry-python false; 1.2.x uses prefer-active-python
    local major, minor = version:match("^(%d+)%.(%d+)")
    major, minor = tonumber(major), tonumber(minor)
    local poetry = install_path .. "\\bin\\poetry.exe"
    local setting
    if major and major >= 2 then
        setting = "virtualenvs.use-poetry-python false"
    elseif major == 1 and minor and minor >= 2 then
        setting = "virtualenvs.prefer-active-python true"
    end
    if setting then
        result = os.execute(string.format('"%s" config %s', poetry, setting))
        if result ~= 0 and result ~= true then
            error("Failed to configure Poetry")
        end
    end
end

function PLUGIN:PostInstall(ctx)
    local install_path = ctx.rootPath

    -- Get version from sdkInfo
    local version = ctx.sdkInfo["poetry"].version

    if RUNTIME.osType == "windows" then
        return install_windows(install_path, version)
    end

    -- Run the Poetry installer via bash script
    local script = string.format(
        [[
#!/bin/bash
set -e

# Run the Poetry installer
# The installer builds its venv with symlinks=False, which copies the python binary.
# That breaks precompiled Pythons (e.g. missing libpython*.dylib on macOS), so patch
# it to create a symlinked venv instead.
installer="$(curl -fsSL https://install.python-poetry.org)"
patched="$(printf '%%s\n' "$installer" | sed 's/symlinks=False/symlinks=True/')"
if [ "$installer" = "$patched" ]; then
    echo "warning: could not patch the Poetry installer to use symlinks" >&2
fi
printf '%%s\n' "$patched" | POETRY_HOME="%s" python3 - --version "%s"

# Configure poetry for mise compatibility
# For Poetry >= 2.0.0, use virtualenvs.use-poetry-python false
# For Poetry >= 1.2.0 and < 2.0.0, use virtualenvs.prefer-active-python true

version_ge() {
    printf '%%s\n%%s\n' "$2" "$1" | sort --check=quiet --version-sort
}

if version_ge "%s" "2.0.0"; then
    "%s/bin/poetry" config virtualenvs.use-poetry-python false
elif version_ge "%s" "1.2.0"; then
    "%s/bin/poetry" config virtualenvs.prefer-active-python true
fi
]],
        install_path,
        version,
        version,
        install_path,
        version,
        install_path
    )

    -- Write and execute the script
    local script_path = install_path .. "/install_poetry.sh"
    local f = io.open(script_path, "w")
    if f then
        f:write(script)
        f:close()
        local result = os.execute("chmod +x " .. shell_quote(script_path) .. " && " .. shell_quote(script_path))
        os.remove(script_path)
        if result ~= 0 and result ~= true then
            error("Poetry installation failed")
        end
    else
        error("Failed to create installation script")
    end
end
