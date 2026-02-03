--- Returns information about where to download the bfs source tarball
--- @param ctx {version: string}  Context containing version info (The version to install)
--- @return table Installation info including download URL
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    -- bfs releases source tarballs on GitHub
    -- URL format: https://github.com/tavianator/bfs/archive/refs/tags/{version}.tar.gz
    local url = string.format("https://github.com/tavianator/bfs/archive/refs/tags/%s.tar.gz", version)

    return {
        version = version,
        url = url,
    }
end
