local file = require("file")

local function path_exists(path)
    return path ~= nil and path ~= "" and file.exists(path)
end

local function shell_quote(value)
    local str = tostring(value or "")
    return "'" .. str:gsub("'", "'\"'\"'") .. "'"
end

local function validate_openssl_prefix(prefix)
    if not path_exists(prefix) then
        return false
    end

    if not path_exists(prefix .. "/include/openssl/ssl.h") then
        return false
    end

    for _, lib_path in ipairs({ "/lib/libssl.dylib", "/lib/libssl.a", "/lib/libssl.so" }) do
        if path_exists(prefix .. lib_path) then
            return true
        end
    end

    return false
end

local function pkg_config_openssl_prefix()
    local handle = io.popen("pkg-config --variable=prefix openssl 2>/dev/null")
    if handle == nil then
        return nil
    end

    local prefix = handle:read("*l")
    local close_ok, _, close_code = handle:close()
    if close_ok ~= true and close_ok ~= 0 then
        return nil
    end
    if close_code ~= nil and close_code ~= 0 then
        return nil
    end

    if prefix ~= nil then
        prefix = prefix:match("^%s*(.-)%s*$")
    end

    if prefix ~= nil and prefix ~= "" and prefix:match("^/") then
        return prefix
    end

    return nil
end

local function find_nix_openssl_prefix()
    local nix_ssl_cert = os.getenv("NIX_SSL_CERT_FILE")
    if nix_ssl_cert ~= nil and nix_ssl_cert ~= "" then
        local nix_prefix = nix_ssl_cert:match("(/nix/store/[^/]+%-openssl[^/]*)")
        if validate_openssl_prefix(nix_prefix) then
            return nix_prefix
        end
    end

    local nix_paths = { "/nix/var/nix/profiles/default" }
    local home = os.getenv("HOME")
    if home ~= nil and home ~= "" then
        table.insert(nix_paths, 1, home .. "/.nix-profile")
    end

    for _, path in ipairs(nix_paths) do
        if validate_openssl_prefix(path) then
            return path
        end
    end

    return nil
end

local function find_openssl_prefix(homebrew_prefix)
    for _, env_var in ipairs({ "OPENSSL_ROOT_DIR", "OPENSSL_DIR" }) do
        local override_path = os.getenv(env_var)
        if validate_openssl_prefix(override_path) then
            return override_path
        end
    end

    local pkg_path = pkg_config_openssl_prefix()
    if validate_openssl_prefix(pkg_path) then
        return pkg_path
    end

    local nix_path = find_nix_openssl_prefix()
    if nix_path ~= nil then
        return nix_path
    end

    local brew = homebrew_prefix or "/opt/homebrew"
    for _, path in ipairs({ brew .. "/opt/openssl@3", brew .. "/opt/openssl" }) do
        if validate_openssl_prefix(path) then
            return path
        end
    end

    for _, path in ipairs({
        "/usr/local/opt/openssl@3",
        "/usr/local/opt/openssl",
        "/opt/local/libexec/openssl3",
        "/opt/local",
        "/usr/local/ssl",
    }) do
        if validate_openssl_prefix(path) then
            return path
        end
    end

    return nil
end

--- Compiles and installs PostgreSQL from source
--- @param ctx PostInstallCtx Context provided by vfox
function PLUGIN:PostInstall(ctx)
    local sdkInfo = ctx.sdkInfo["postgres"]
    local version = sdkInfo.version
    local sdkPath = sdkInfo.path

    -- mise extracts tarball and strips top-level directory, so sdkPath IS the source directory

    -- Build configure options
    local configureOptions = "--prefix=" .. shell_quote(sdkPath)
    local envPrefix = "" -- Environment variables to prepend to configure command

    -- Add common options
    configureOptions = configureOptions .. " --with-openssl --with-zlib"

    -- Try to add UUID support (e2fs on Linux, BSD on macOS)
    local os_type = RUNTIME.osType
    local homebrew_prefix = os.getenv("HOMEBREW_PREFIX") or "/opt/homebrew"

    if os_type == "darwin" then
        local openssl_path = find_openssl_prefix(homebrew_prefix)
        -- Homebrew paths
        local icu_path = homebrew_prefix .. "/opt/icu4c"
        local ossp_uuid_path = homebrew_prefix .. "/opt/ossp-uuid"
        local util_linux_path = homebrew_prefix .. "/opt/util-linux"

        -- Build library and include paths
        local lib_paths = {}
        local include_paths = {}

        -- Check if OpenSSL exists in supported locations
        if openssl_path ~= nil then
            table.insert(lib_paths, openssl_path .. "/lib")
            table.insert(include_paths, openssl_path .. "/include")
        else
            io.stderr:write("Warning: OpenSSL not found in known locations.\n")
            io.stderr:write("  Set OPENSSL_ROOT_DIR or OPENSSL_DIR to your OpenSSL installation prefix.\n")
        end

        -- Check if ICU exists in Homebrew (PostgreSQL 17+ requires ICU by default)
        if path_exists(icu_path .. "/lib") then
            table.insert(lib_paths, icu_path .. "/lib")
            table.insert(include_paths, icu_path .. "/include")
            -- Set PKG_CONFIG_PATH for ICU (prepend to configure command)
            local pkg_config_path = os.getenv("PKG_CONFIG_PATH") or ""
            if pkg_config_path ~= "" then
                pkg_config_path = icu_path .. "/lib/pkgconfig:" .. pkg_config_path
            else
                pkg_config_path = icu_path .. "/lib/pkgconfig"
            end
            envPrefix = "PKG_CONFIG_PATH=" .. shell_quote(pkg_config_path) .. " "
        else
            -- ICU not found, disable it
            io.stderr:write("Warning: ICU not found. Installing without ICU support.\n")
            io.stderr:write("  To enable ICU: brew install icu4c\n")
            configureOptions = configureOptions .. " --without-icu"
        end

        -- Check for UUID library: prefer ossp-uuid, then util-linux (e2fs), otherwise skip
        if path_exists(ossp_uuid_path .. "/lib") then
            configureOptions = configureOptions .. " --with-uuid=ossp"
            table.insert(lib_paths, ossp_uuid_path .. "/lib")
            table.insert(include_paths, ossp_uuid_path .. "/include")
        else
            if path_exists(util_linux_path .. "/lib") then
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
            configureOptions = configureOptions .. " --with-libraries=" .. shell_quote(table.concat(lib_paths, ":"))
        end
        if #include_paths > 0 then
            configureOptions = configureOptions .. " --with-includes=" .. shell_quote(table.concat(include_paths, ":"))
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
        configureOptions = "--prefix=" .. shell_quote(sdkPath) .. " " .. userOptions
    end

    -- Run configure
    print("Configuring PostgreSQL with: " .. configureOptions)
    local configureCmd = string.format("cd %s && %s./configure %s", shell_quote(sdkPath), envPrefix, configureOptions)
    local status = os.execute(configureCmd)
    if status ~= 0 and status ~= true then
        error("Failed to configure PostgreSQL")
    end

    -- Build PostgreSQL
    print("Building PostgreSQL (this may take several minutes)...")
    local makeCmd = string.format(
        "cd %s && make -j$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)",
        shell_quote(sdkPath)
    )
    status = os.execute(makeCmd)
    if status ~= 0 and status ~= true then
        error("Failed to build PostgreSQL")
    end

    -- Install PostgreSQL
    print("Installing PostgreSQL...")
    local installCmd = string.format("cd %s && make install", shell_quote(sdkPath))
    status = os.execute(installCmd)
    if status ~= 0 and status ~= true then
        error("Failed to install PostgreSQL")
    end

    -- Build and install contrib modules
    print("Building contrib modules...")
    local contribCmd = string.format(
        "cd %s && make -j$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2) && make install",
        shell_quote(sdkPath .. "/contrib")
    )
    status = os.execute(contribCmd)
    if status ~= 0 and status ~= true then
        -- Contrib failure is not fatal
        print("Warning: Failed to build some contrib modules")
    end

    -- Create data directory
    local dataDir = sdkPath .. "/data"
    os.execute(string.format("mkdir -p %s", shell_quote(dataDir)))

    -- Run initdb unless skipped
    local skipInitdb = os.getenv("POSTGRES_SKIP_INITDB")
    if skipInitdb ~= "1" and skipInitdb ~= "true" then
        print("Initializing database cluster...")
        local initdbCmd =
            string.format("%s -D %s -U postgres", shell_quote(sdkPath .. "/bin/initdb"), shell_quote(dataDir))
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
        "cd %s && rm -rf src doc contrib config Makefile GNUmakefile configure* aclocal* 2>/dev/null",
        shell_quote(sdkPath)
    )
    os.execute(cleanCmd)

    print("PostgreSQL installation complete!")
end
