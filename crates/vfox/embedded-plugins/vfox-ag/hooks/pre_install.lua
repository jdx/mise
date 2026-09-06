--- Returns download information for a specific version
--- @param ctx table Context provided by vfox (contains version)
--- @return table Version info with download URL
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    if version == nil or version == "" then
        error("You must provide a version number, eg: vfox install ag@2.2.0")
    end

    -- Download source tarball from GitHub
    local url = "https://github.com/ggreer/the_silver_searcher/archive/refs/tags/" .. version .. ".tar.gz"

    return {
        version = version,
        url = url,
    }
end
