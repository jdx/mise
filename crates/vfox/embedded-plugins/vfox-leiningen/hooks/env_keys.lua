--- Returns environment variables to set
--- @param ctx table Context object with path field (install directory)
--- @return table Array of environment variable definitions
function PLUGIN:EnvKeys(ctx)
    local mainPath = ctx.path

    return {
        {
            key = "PATH",
            value = mainPath .. "/bin",
        },
        {
            key = "LEIN_HOME",
            value = mainPath,
        },
    }
end
