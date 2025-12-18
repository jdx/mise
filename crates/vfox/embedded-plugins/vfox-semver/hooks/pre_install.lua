-- hooks/pre_install.lua
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    -- The semver tool is a single script file downloaded from the GitHub repo
    local url = "https://raw.githubusercontent.com/fsaintjacques/semver-tool/" .. version .. "/src/semver"

    return {
        version = version,
        url = url,
        note = "Downloading semver " .. version,
    }
end
