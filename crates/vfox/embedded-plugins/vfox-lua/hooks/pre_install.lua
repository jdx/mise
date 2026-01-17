--- Returns pre-install information for Lua
--- @param ctx table Context provided by vfox
--- @return table Pre-install info
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    return {
        version = version,
        url = "https://www.lua.org/ftp/lua-" .. version .. ".tar.gz",
    }
end
