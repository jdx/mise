--- Returns environment keys and paths for the installed tool
--- @param ctx table Context provided by vfox
--- @return table Environment keys
function PLUGIN:EnvKeys(ctx)
    local mainSdk = ctx.sdkInfo["ag"]
    local path = mainSdk.path

    return {
        {
            key = "PATH",
            value = path .. "/bin",
        },
    }
end
