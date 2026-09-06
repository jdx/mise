--- Returns version information for installation
--- @param ctx table Context provided by vfox (contains version)
--- @return table Version info with download URL
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    if version == nil or version == "" then
        error("You must provide a version number, eg: vfox install bpkg@1.1.3")
    end

    -- bpkg releases don't have a 'v' prefix in the tarball URL
    local url = "https://github.com/bpkg/bpkg/archive/refs/tags/" .. version .. ".tar.gz"

    return {
        version = version,
        url = url,
    }
end
