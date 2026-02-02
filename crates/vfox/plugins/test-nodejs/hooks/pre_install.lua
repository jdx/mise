--- Return the URL to download the tool
--- @param ctx table See /vfox/ctx.md#ctx-hooks for more information on ctx
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    -- Return simple test data without runtime checks
    return {
        version = version,
        url = "file:///fake/nodejs/node-v" .. version .. ".tar.gz",
        sha256 = "fakehash" .. version,
    }
end