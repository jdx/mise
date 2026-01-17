--- Compiles and installs Lua from source
--- @param ctx table Context provided by vfox
--- @field ctx.sdkInfo table SDK information with version and path
function PLUGIN:PostInstall(ctx)
    local http = require("http")
    local json = require("json")

    local sdkInfo = ctx.sdkInfo["lua"]
    local version = sdkInfo.version
    local sdkPath = sdkInfo.path

    -- mise extracts tarball and strips top-level directory, so sdkPath IS the source directory

    -- Determine OS-specific make target
    local os_type = RUNTIME.osType
    local make_target = "guess"

    if os_type == "darwin" then
        make_target = "macosx"
    elseif os_type == "linux" then
        -- For Lua < 5.4, use "linux", otherwise "guess"
        local major, minor = string.match(version, "^(%d+)%.(%d+)")
        if major and minor then
            local ver_num = tonumber(major) * 100 + tonumber(minor)
            if ver_num < 504 then
                make_target = "linux"
            end
        end
    end

    -- Build Lua
    local major = tonumber(string.match(version, "^(%d+)"))
    local buildCmd

    if major and major >= 5 then
        -- Lua 5.x: use make local target which creates install/ subdirectory
        buildCmd = string.format(
            "cd '%s' && make %s && make local",
            sdkPath, make_target
        )
    else
        -- Older versions
        buildCmd = string.format(
            "cd '%s' && make && make install INSTALL_ROOT=install",
            sdkPath
        )
    end

    local status = os.execute(buildCmd)
    if status ~= 0 and status ~= true then
        error("Failed to build Lua: make failed")
    end

    -- After make local, files are in install/ subdirectory
    -- Move them to the root of sdkPath (overwriting source files is fine)
    local moveCmd = string.format(
        "cd '%s' && mv install/* . 2>/dev/null || cp -r install/* . 2>/dev/null",
        sdkPath
    )
    os.execute(moveCmd)

    -- Install LuaRocks for Lua 5.x
    if major and major >= 5 then
        -- Get latest LuaRocks version from GitHub
        local luarocksVersion = "3.11.1" -- Default fallback

        local resp, err = http.get({
            url = "https://api.github.com/repos/luarocks/luarocks/tags?per_page=1",
        })

        if err == nil and resp.status_code == 200 then
            local data = json.decode(resp.body)
            if data ~= nil and type(data) == "table" and #data > 0 then
                local tag = data[1]["name"]
                if tag then
                    -- Remove 'v' prefix if present
                    luarocksVersion = string.gsub(tag, "^v", "")
                end
            end
        end

        -- Download and install LuaRocks
        local luarocksUrl = "https://luarocks.org/releases/luarocks-" .. luarocksVersion .. ".tar.gz"
        local luarocksArchive = sdkPath .. "/luarocks.tar.gz"

        local downloadCmd = string.format("curl -sL '%s' -o '%s'", luarocksUrl, luarocksArchive)
        status = os.execute(downloadCmd)
        if status ~= 0 and status ~= true then
            -- LuaRocks installation is optional, don't fail
            return
        end

        local extractCmd = string.format("cd '%s' && tar xzf luarocks.tar.gz", sdkPath)
        status = os.execute(extractCmd)
        if status ~= 0 and status ~= true then
            return
        end

        local luarocksDir = sdkPath .. "/luarocks-" .. luarocksVersion
        local configureCmd = string.format(
            "cd '%s' && ./configure --with-lua='%s' --with-lua-include='%s/include' --with-lua-lib='%s/lib' --prefix='%s/luarocks' 2>/dev/null",
            luarocksDir, sdkPath, sdkPath, sdkPath, sdkPath
        )
        status = os.execute(configureCmd)
        if status ~= 0 and status ~= true then
            -- Clean up and return without luarocks
            os.execute(string.format("rm -rf '%s/luarocks.tar.gz' '%s/luarocks-'*", sdkPath, sdkPath))
            return
        end

        local bootstrapCmd = string.format("cd '%s' && make bootstrap 2>&1", luarocksDir)
        os.execute(bootstrapCmd)

        -- Clean up LuaRocks source
        os.execute(string.format("rm -rf '%s/luarocks.tar.gz' '%s/luarocks-'*", sdkPath, sdkPath))
    end

    -- Clean up Lua source files (keep only bin, lib, include, man, share, luarocks)
    local cleanCmd = string.format(
        "cd '%s' && rm -rf src doc Makefile README install 2>/dev/null",
        sdkPath
    )
    os.execute(cleanCmd)
end
