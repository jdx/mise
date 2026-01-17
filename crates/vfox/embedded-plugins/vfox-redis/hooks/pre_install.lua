--- Returns download information for a specific Redis version
--- @param ctx table Context object with version field
--- @return table Download info with version, url, and optional note
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    return {
        version = version,
        url = "https://download.redis.io/releases/redis-" .. version .. ".tar.gz",
        note = "Downloading Redis " .. version .. " source..."
    }
end
