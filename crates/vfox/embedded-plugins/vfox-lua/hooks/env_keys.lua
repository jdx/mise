--- Returns environment variables for Lua
--- @param ctx table Context provided by vfox
--- @return table Environment configuration
function PLUGIN:EnvKeys(ctx)
    local sdkInfo = ctx.sdkInfo["lua"]
    local version = sdkInfo.version
    local installDir = sdkInfo.path

    -- Extract major.minor version for Lua paths
    local shortVersion = string.match(version, "^(%d+%.%d+)")

    local envs = {
        {
            key = "PATH",
            value = installDir .. "/bin",
        },
    }

    -- Add LuaRocks bin to PATH if it exists
    local luarocksBin = installDir .. "/luarocks/bin"
    local f = io.open(luarocksBin, "r")
    if f ~= nil then
        f:close()
        table.insert(envs, {
            key = "PATH",
            value = luarocksBin,
        })
    end

    -- Set LUA_INIT for package paths (similar to asdf-lua)
    if shortVersion then
        local packagePath = string.format(
            "package.path = package.path .. ';%s/share/lua/%s/?.lua;%s/share/lua/%s/?/init.lua;%s/luarocks/share/lua/%s/?.lua;%s/luarocks/share/lua/%s/?/init.lua'",
            installDir, shortVersion,
            installDir, shortVersion,
            installDir, shortVersion,
            installDir, shortVersion
        )
        local packageCpath = string.format(
            "package.cpath = package.cpath .. ';%s/lib/lua/%s/?.so;%s/luarocks/lib/lua/%s/?.so'",
            installDir, shortVersion,
            installDir, shortVersion
        )

        table.insert(envs, {
            key = "LUA_INIT",
            value = packagePath .. "\n" .. packageCpath,
        })
    end

    return envs
end
