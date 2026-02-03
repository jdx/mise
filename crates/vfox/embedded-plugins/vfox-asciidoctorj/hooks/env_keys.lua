--- Sets environment variables for the installed version.
--- @param ctx {path: string}  (Installation path)
--- @return table Environment variables
function PLUGIN:EnvKeys(ctx)
    -- mise flattens the zip structure, so bin is directly in ctx.path
    return {
        {
            key = "PATH",
            value = ctx.path .. "/bin",
        },
    }
end
