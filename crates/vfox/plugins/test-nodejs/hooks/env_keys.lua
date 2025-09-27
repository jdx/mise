--- Return environment variables for the tool
--- @param ctx table See /vfox/ctx.md#ctx-hooks for more information on ctx
--- @return table Environment variables
function PLUGIN:EnvKeys(ctx)
    local mainPath = ctx.path
    return {
        {
            key = "PATH",
            value = mainPath .. "/bin"
        },
        {
            key = "NODE_HOME",
            value = mainPath
        }
    }
end