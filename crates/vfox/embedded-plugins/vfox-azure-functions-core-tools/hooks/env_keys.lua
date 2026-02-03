--- Sets environment variables for the installed version.
--- @param ctx {path: string}  (Installation path)
--- @return table Environment variables
function PLUGIN:EnvKeys(ctx)
    -- The func binary is at the root of the extracted zip
    return {
        {
            key = "PATH",
            value = ctx.path,
        },
    }
end
