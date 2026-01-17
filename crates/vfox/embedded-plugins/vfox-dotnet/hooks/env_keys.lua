--- Returns environment keys and paths for the installed .NET SDK
--- @param ctx table Context provided by vfox
--- @return table Environment keys
function PLUGIN:EnvKeys(ctx)
    local mainSdk = ctx.sdkInfo["dotnet"]
    local path = mainSdk.path

    return {
        {
            key = "PATH",
            value = path,
        },
        {
            key = "DOTNET_ROOT",
            value = path,
        },
    }
end
