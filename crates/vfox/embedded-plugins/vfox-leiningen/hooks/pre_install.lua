--- Returns download information for a specific version
--- @param ctx table Context object with version field
--- @return table Download info with version and url fields
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    return {
        version = version,
        url = "https://github.com/technomancy/leiningen/releases/download/"
            .. version
            .. "/leiningen-"
            .. version
            .. "-standalone.jar",
    }
end
