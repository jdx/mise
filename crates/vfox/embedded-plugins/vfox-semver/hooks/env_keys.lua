-- hooks/env_keys.lua
function PLUGIN:EnvKeys(ctx)
    local mainPath = ctx.path

    -- Add the bin directory to PATH where semver will be installed
    return {
        {
            key = "PATH",
            value = mainPath .. "/bin",
        },
    }
end
