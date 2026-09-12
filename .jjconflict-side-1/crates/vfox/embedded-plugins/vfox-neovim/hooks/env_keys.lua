--- Sets environment variables for the installed Neovim version

function PLUGIN:EnvKeys(ctx)
    local main_path = ctx.path
    return {
        {
            key = "PATH",
            value = main_path .. "/bin",
        },
    }
end
