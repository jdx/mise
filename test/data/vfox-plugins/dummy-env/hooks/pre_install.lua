function PLUGIN:PreInstall(ctx)
    local version = ctx.version
    return {
        version = version,
    }
end
