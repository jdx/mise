--- Compiles and installs PostgreSQL from source
--- @param ctx table Context provided by vfox
--- @field ctx.sdkInfo table SDK information with version and path
function PLUGIN:PostInstall(ctx)
    local sdkInfo = ctx.sdkInfo["postgres"]
    local version = sdkInfo.version
    local sdkPath = sdkInfo.path

    -- mise extracts tarball and strips top-level directory, so sdkPath IS the source directory

    -- Build configure options
    local configureOptions = "--prefix='" .. sdkPath .. "'"
    local envPrefix = "" -- Environment variables to prepend to configure command

    -- Add common options
    configureOptions = configureOptions .. " --with-openssl --with-zlib"

    -- Try to add UUID support (e2fs on Linux, BSD on macOS)
    local os_type = RUNTIME.osType
    local homebrew_prefix = os.getenv("HOMEBREW_PREFIX") or "/opt/homebrew"

    if os_type == "darwin" then
        -- Homebrew paths
        local openssl_path = homebrew_prefix .. "/opt/openssl"
        local icu_path = homebrew_prefix .. "/opt/icu4c"
        local ossp_uuid_path = homebrew_prefix .. "/opt/ossp-uuid"
        local util_linux_path = homebrew_prefix .. "/opt/util-linux"

        -- Build library and include paths
        local lib_paths = {}
        local include_paths = {}
        local f

        -- Check if OpenSSL exists in Homebrew
        f = io.open(openssl_path .. "/lib", "r")
        if f ~= nil then
            f:close()
            table.insert(lib_paths, openssl_path .. "/lib")
            table.insert(include_paths, openssl_path .. "/include")
        end

        -- Check if ICU exists in Homebrew (PostgreSQL 17+ requires ICU by default)
        f = io.open(icu_path .. "/lib", "r")
        if f ~= nil then
            f:close()
            table.insert(lib_paths, icu_path .. "/lib")
            table.insert(include_paths, icu_path .. "/include")
            -- Set PKG_CONFIG_PATH for ICU (prepend to configure command)
            local pkg_config_path = os.getenv("PKG_CONFIG_PATH") or ""
            if pkg_config_path ~= "" then
                pkg_config_path = icu_path .. "/lib/pkgconfig:" .. pkg_config_path
            else
                pkg_config_path = icu_path .. "/lib/pkgconfig"
            end
            envPrefix = "PKG_CONFIG_PATH='" .. pkg_config_path .. "' "
        else
            -- ICU not found, disable it
            io.stderr:write("Warning: ICU not found. Installing without ICU support.\n")
            io.stderr:write("  To enable ICU: brew install icu4c\n")
            configureOptions = configureOptions .. " --without-icu"
        end

        -- Check for UUID library: prefer ossp-uuid, then util-linux (e2fs), otherwise skip
        f = io.open(ossp_uuid_path .. "/lib", "r")
        if f ~= nil then
            f:close()
            configureOptions = configureOptions .. " --with-uuid=ossp"
            table.insert(lib_paths, ossp_uuid_path .. "/lib")
            table.insert(include_paths, ossp_uuid_path .. "/include")
        else
            f = io.open(util_linux_path .. "/lib", "r")
            if f ~= nil then
                f:close()
                configureOptions = configureOptions .. " --with-uuid=e2fs"
                table.insert(lib_paths, util_linux_path .. "/lib")
                table.insert(include_paths, util_linux_path .. "/include")
            else
                -- Neither UUID library available
                io.stderr:write("Warning: UUID library not found. Installing without UUID support.\n")
                io.stderr:write("  To enable UUID: brew install ossp-uuid\n")
            end
        end

        if #lib_paths > 0 then
            configureOptions = configureOptions .. " --with-libraries='" .. table.concat(lib_paths, ":") .. "'"
        end
        if #include_paths > 0 then
            configureOptions = configureOptions .. " --with-includes='" .. table.concat(include_paths, ":") .. "'"
        end
    else
        -- Linux: use e2fs UUID
        configureOptions = configureOptions .. " --with-uuid=e2fs"

        -- Check if ICU is available on Linux
        local icu_check = os.execute("pkg-config --exists icu-uc 2>/dev/null")
        if icu_check ~= 0 and icu_check ~= true then
            -- ICU not found, disable it
            io.stderr:write("Warning: ICU not found. Installing without ICU support.\n")
            io.stderr:write("  To enable ICU: sudo apt install libicu-dev (Debian/Ubuntu)\n")
            configureOptions = configureOptions .. " --without-icu"
        end
    end

    -- Allow user to override or extend configure options
    local extraOptions = os.getenv("POSTGRES_EXTRA_CONFIGURE_OPTIONS")
    if extraOptions ~= nil and extraOptions ~= "" then
        configureOptions = configureOptions .. " " .. extraOptions
    end

    local userOptions = os.getenv("POSTGRES_CONFIGURE_OPTIONS")
    if userOptions ~= nil and userOptions ~= "" then
        -- User provided full options, use those instead (but keep prefix)
        configureOptions = "--prefix='" .. sdkPath .. "' " .. userOptions
    end

    -- Run configure
    print("Configuring PostgreSQL with: " .. configureOptions)
    local configureCmd = string.format("cd '%s' && %s./configure %s", sdkPath, envPrefix, configureOptions)
    local status = os.execute(configureCmd)
    if status ~= 0 and status ~= true then
        error("Failed to configure PostgreSQL")
    end

    -- Build PostgreSQL
    print("Building PostgreSQL (this may take several minutes)...")
    local makeCmd =
        string.format("cd '%s' && make -j$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)", sdkPath)
    status = os.execute(makeCmd)
    if status ~= 0 and status ~= true then
        error("Failed to build PostgreSQL")
    end

    -- Install PostgreSQL
    print("Installing PostgreSQL...")
    local installCmd = string.format("cd '%s' && make install", sdkPath)
    status = os.execute(installCmd)
    if status ~= 0 and status ~= true then
        error("Failed to install PostgreSQL")
    end

    -- Build and install contrib modules
    print("Building contrib modules...")
    local contribCmd = string.format(
        "cd '%s/contrib' && make -j$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2) && make install",
        sdkPath
    )
    status = os.execute(contribCmd)
    if status ~= 0 and status ~= true then
        -- Contrib failure is not fatal
        print("Warning: Failed to build some contrib modules")
    end

    -- Create data directory
    local dataDir = sdkPath .. "/data"
    os.execute(string.format("mkdir -p '%s'", dataDir))

    -- Run initdb unless skipped
    local skipInitdb = os.getenv("POSTGRES_SKIP_INITDB")
    if skipInitdb ~= "1" and skipInitdb ~= "true" then
        print("Initializing database cluster...")
        local initdbCmd = string.format("'%s/bin/initdb' -D '%s' -U postgres", sdkPath, dataDir)
        status = os.execute(initdbCmd)
        if status ~= 0 and status ~= true then
            print("Warning: initdb failed. You may need to run it manually.")
        end
    else
        print("Skipping initdb (POSTGRES_SKIP_INITDB is set)")
    end

    -- Clean up source files to save space
    print("Cleaning up source files...")
    local cleanCmd = string.format(
        "cd '%s' && rm -rf src doc contrib config Makefile GNUmakefile configure* aclocal* 2>/dev/null",
        sdkPath
    )
    os.execute(cleanCmd)

    print("PostgreSQL installation complete!")
end
