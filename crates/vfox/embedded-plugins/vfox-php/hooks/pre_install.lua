--- Returns pre-install information for PHP
--- @param ctx table Context provided by vfox
--- @return table Pre-install info
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    -- Download from GitHub php-src releases
    return {
        version = version,
        url = "https://github.com/php/php-src/archive/php-" .. version .. ".tar.gz",
    }
end
