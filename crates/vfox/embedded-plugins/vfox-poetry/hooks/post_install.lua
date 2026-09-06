--- Post-installation hook for Poetry
--- Runs the installer script with the correct version

function PLUGIN:PostInstall(ctx)
    local install_path = ctx.rootPath

    -- Get version from sdkInfo
    local version = ctx.sdkInfo["poetry"].version

    -- Run the Poetry installer via bash script
    local script = string.format(
        [[
#!/bin/bash
set -e

# Run the Poetry installer
curl -sSL https://install.python-poetry.org | POETRY_HOME="%s" python3 - --version "%s"

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
        local result = os.execute("chmod +x " .. script_path .. " && " .. script_path)
        os.execute("rm -f " .. script_path)
        if result ~= 0 and result ~= true then
            error("Poetry installation failed")
        end
    else
        error("Failed to create installation script")
    end
end
