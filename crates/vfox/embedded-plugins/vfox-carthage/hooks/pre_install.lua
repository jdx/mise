--- Returns information about where to download the carthage .pkg
--- @param ctx table Context containing version info
--- @field ctx.version string The version to install
--- @return table Installation info including download URL
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    -- Carthage releases .pkg files on GitHub
    local url = string.format(
        "https://github.com/Carthage/Carthage/releases/download/%s/Carthage.pkg",
        version
    )

    return {
        version = version,
        url = url,
    }
end
