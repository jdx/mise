function PLUGIN:PostInstall(ctx)
    --- SDK installation root path
    local rootPath = ctx.rootPath
    local runtimeVersion = ctx.runtimeVersion
    --- Other SDK information, the `addition` field returned in PreInstall, obtained by name
    --TODO
    --local sdkInfo = ctx.sdkInfo['dummy']
    --local path = sdkInfo.path
    --local version = sdkInfo.version
    --local name = sdkInfo.name
end
