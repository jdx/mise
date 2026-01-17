--- Returns environment variables to set for Redis
--- @param ctx table Context object with path field
--- @return table Array of environment variable key-value pairs
function PLUGIN:EnvKeys(ctx)
    local mainPath = ctx.path

    return {
        {
            key = "PATH",
            value = mainPath .. "/bin"
        }
    }
end
