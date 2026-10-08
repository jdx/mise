local http = require("http")

--- Quote a string as a single POSIX shell word
local function shell_quote(value)
    return "'" .. string.gsub(value, "'", "'\\''") .. "'"
end

--- Homebrew prefix: honor HOMEBREW_PREFIX, else pick the one that exists
--- (/opt/homebrew on Apple Silicon, /usr/local on Intel)
local function homebrew_prefix_dir()
    local prefix = os.getenv("HOMEBREW_PREFIX")
    if prefix ~= nil and prefix ~= "" then
        return prefix
    end
    for _, candidate in ipairs({ "/opt/homebrew", "/usr/local" }) do
        local f = io.open(candidate .. "/Cellar", "r")
        if f ~= nil then
            f:close()
            return candidate
        end
    end
    return "/opt/homebrew"
end

--- Compiles and installs PHP from source
--- @param ctx table Context provided by vfox
--- @field ctx.sdkInfo table SDK information with version and path
function PLUGIN:PostInstall(ctx)
    local sdkInfo = ctx.sdkInfo["php"]
    local version = sdkInfo.version
    local sdkPath = sdkInfo.path

    -- mise extracts tarball to sdkPath, with top-level directory stripped
    -- So sdkPath IS the source directory (php-src-php-X.Y.Z contents)

    local os_type = RUNTIME.osType
    local homebrew_prefix = homebrew_prefix_dir()

    -- Build environment and configure options
    local envPrefix = ""
    local configureOptions = "--prefix=" .. shell_quote(sdkPath)

    -- Common configure options
    local commonOptions = [[
        --enable-bcmath
        --enable-calendar
        --enable-dba
        --enable-exif
        --enable-fpm
        --enable-ftp
        --enable-gd
        --enable-intl
        --enable-mbregex
        --enable-mbstring
        --enable-mysqlnd
        --enable-pcntl
        --enable-shmop
        --enable-soap
        --enable-sockets
        --enable-sysvmsg
        --enable-sysvsem
        --enable-sysvshm
        --with-curl
        --with-mhash
        --with-mysqli=mysqlnd
        --with-pdo-mysql=mysqlnd
        --with-zlib
        --with-pear
        --without-pcre-jit
        --without-snmp
    ]]

    -- Clean up whitespace in common options (before inserting paths, which may contain spaces)
    commonOptions = string.gsub(commonOptions, "%s+", " ")
    configureOptions = configureOptions
        .. " "
        .. commonOptions
        .. " --sysconfdir="
        .. shell_quote(sdkPath)
        .. " --with-config-file-path="
        .. shell_quote(sdkPath)
        .. " --with-config-file-scan-dir="
        .. shell_quote(sdkPath .. "/conf.d")

    if os_type == "darwin" then
        configureOptions, envPrefix = configure_macos(configureOptions, homebrew_prefix)
    else
        configureOptions = configure_linux(configureOptions)
    end

    -- Allow user to override configure options
    local extraOptions = os.getenv("PHP_EXTRA_CONFIGURE_OPTIONS")
    if extraOptions ~= nil and extraOptions ~= "" then
        configureOptions = configureOptions .. " " .. extraOptions
    end

    local userOptions = os.getenv("PHP_CONFIGURE_OPTIONS")
    if userOptions ~= nil and userOptions ~= "" then
        -- User provided full options, use those instead (but keep prefix)
        configureOptions = "--prefix=" .. shell_quote(sdkPath) .. " " .. userOptions
    end

    -- Run buildconf
    print("Running buildconf...")
    local buildconfCmd = string.format("cd %s && %s./buildconf --force", shell_quote(sdkPath), envPrefix)
    local status = os.execute(buildconfCmd)
    if status ~= 0 and status ~= true then
        error("Failed to run buildconf")
    end

    -- Run configure
    print("Configuring PHP with options...")
    local configureCmd = string.format("cd %s && %s./configure %s", shell_quote(sdkPath), envPrefix, configureOptions)
    status = os.execute(configureCmd)
    if status ~= 0 and status ~= true then
        error("Failed to configure PHP")
    end

    -- Build PHP
    print("Building PHP (this may take several minutes)...")
    local makeCmd = string.format(
        "cd %s && %smake -j$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)",
        shell_quote(sdkPath),
        envPrefix
    )
    status = os.execute(makeCmd)
    if status ~= 0 and status ~= true then
        error("Failed to build PHP")
    end

    -- Install PHP
    print("Installing PHP...")
    local installCmd = string.format("cd %s && %smake install", shell_quote(sdkPath), envPrefix)
    status = os.execute(installCmd)
    if status ~= 0 and status ~= true then
        error("Failed to install PHP")
    end

    -- Create conf.d directory
    os.execute(string.format("mkdir -p %s", shell_quote(sdkPath .. "/conf.d")))
    local confFile = io.open(sdkPath .. "/conf.d/php.ini", "w")
    if confFile then
        confFile:write("# Add system-wide PHP configuration options here\n")
        confFile:close()
    end

    -- Install Composer
    install_composer(sdkPath)

    -- Clean up source files to save space
    print("Cleaning up source files...")
    local cleanCmd = string.format(
        "cd %s && rm -rf Zend ext sapi main TSRM build configure* aclocal* Makefile* 2>/dev/null",
        shell_quote(sdkPath)
    )
    os.execute(cleanCmd)

    print("PHP installation complete!")
end

--- Configure options for macOS with Homebrew
function configure_macos(configureOptions, homebrew_prefix)
    local envPrefix = ""
    local pkg_config_paths = {}

    -- Required packages
    -- Note: bzip2 doesn't have pkgconfig, freetype/libpng depend on it
    -- So we don't add freetype/libpng to PKG_CONFIG_PATH, instead rely on path-based detection
    local required_packages = {
        { name = "bison", path_only = true },
        { name = "re2c", path_only = true },
        { name = "icu4c", pkg_config = true },
        { name = "krb5", pkg_config = true },
        { name = "libedit", pkg_config = true },
        { name = "libxml2", pkg_config = true },
        { name = "openssl@3", pkg_config = true },
        { name = "zlib", pkg_config = true },
        { name = "libzip", pkg_config = true },
        { name = "oniguruma", pkg_config = true },
        { name = "sqlite", pkg_config = true },
        { name = "curl", pkg_config = true },
    }

    -- Check for versioned icu4c (icu4c@76, icu4c@77, icu4c@78, etc.)
    local icu_path = nil
    for v = 80, 70, -1 do
        local test_path = homebrew_prefix .. "/opt/icu4c@" .. v
        local f = io.open(test_path .. "/lib", "r")
        if f ~= nil then
            f:close()
            icu_path = test_path
            break
        end
    end
    if icu_path == nil then
        local f = io.open(homebrew_prefix .. "/opt/icu4c/lib", "r")
        if f ~= nil then
            f:close()
            icu_path = homebrew_prefix .. "/opt/icu4c"
        end
    end

    for _, pkg in ipairs(required_packages) do
        local pkg_path
        if pkg.name == "icu4c" then
            pkg_path = icu_path
        else
            pkg_path = homebrew_prefix .. "/opt/" .. pkg.name
        end

        if pkg_path ~= nil then
            -- Check for /bin for path_only packages, /lib for others
            local check_dir = pkg.path_only and "/bin" or "/lib"
            local f = io.open(pkg_path .. check_dir, "r")
            if f ~= nil then
                f:close()
                if pkg.pkg_config then
                    table.insert(pkg_config_paths, pkg_path .. "/lib/pkgconfig")
                end
                if pkg.path_only then
                    envPrefix = envPrefix .. "export PATH=" .. shell_quote(pkg_path .. "/bin") .. ':"$PATH" && '
                end
            else
                io.stderr:write("Warning: " .. pkg.name .. " not found at " .. pkg_path .. check_dir .. "\n")
            end
        else
            io.stderr:write("Warning: " .. pkg.name .. " not found\n")
        end
    end

    -- Build PKG_CONFIG_PATH
    if #pkg_config_paths > 0 then
        local existing_pkg = os.getenv("PKG_CONFIG_PATH") or ""
        local new_pkg = table.concat(pkg_config_paths, ":")
        if existing_pkg ~= "" then
            new_pkg = new_pkg .. ":" .. existing_pkg
        end
        envPrefix = envPrefix .. "export PKG_CONFIG_PATH=" .. shell_quote(new_pkg) .. " && "
    end

    -- Set FREETYPE2 flags to bypass pkg-config (bzip2 doesn't have .pc file)
    local freetype_path = homebrew_prefix .. "/opt/freetype"
    local f = io.open(freetype_path .. "/lib", "r")
    if f ~= nil then
        f:close()
        envPrefix = envPrefix
            .. "export FREETYPE2_CFLAGS="
            .. shell_quote("-I" .. freetype_path .. "/include/freetype2")
            .. " && "
        envPrefix = envPrefix
            .. "export FREETYPE2_LIBS="
            .. shell_quote("-L" .. freetype_path .. "/lib -lfreetype")
            .. " && "
    end

    -- Optional packages with configure flags
    local optional_packages = {
        { name = "gmp", flag = "--with-gmp" },
        { name = "libsodium", flag = "--with-sodium" },
        { name = "freetype", flag = "--with-freetype" },
        { name = "gettext", flag = "--with-gettext" },
        { name = "jpeg", flag = "--with-jpeg", path_name = "jpeg-turbo" },
        { name = "webp", flag = "--with-webp" },
        { name = "libpng", flag = "--with-png" },
        { name = "readline", flag = "--with-readline" },
        { name = "bzip2", flag = "--with-bz2" },
        { name = "libiconv", flag = "--with-iconv" },
        { name = "libpq", flag = "--with-pdo-pgsql" },
        { name = "openssl@3", flag = "--with-openssl" },
    }

    for _, pkg in ipairs(optional_packages) do
        local pkg_path = homebrew_prefix .. "/opt/" .. (pkg.path_name or pkg.name)
        local f = io.open(pkg_path .. "/lib", "r")
        if f ~= nil then
            f:close()
            configureOptions = configureOptions .. " " .. pkg.flag .. "=" .. shell_quote(pkg_path)
        else
            io.stderr:write("Info: " .. pkg.name .. " not found, skipping " .. pkg.flag .. "\n")
        end
    end

    -- Add external-gd only if libgd itself is installed (image libraries alone
    -- are not enough; without it PHP uses its bundled GD)
    local gd_lib = io.open(homebrew_prefix .. "/opt/gd/lib", "r")
    if gd_lib ~= nil then
        gd_lib:close()
        configureOptions = configureOptions .. " --with-external-gd"
    end

    return configureOptions, envPrefix
end

--- Configure options for Linux
function configure_linux(configureOptions)
    -- On Linux, most libraries are in standard paths
    configureOptions = configureOptions .. " --with-openssl --with-curl --with-readline --with-gettext"

    -- Only use external GD when libgd itself is present; otherwise PHP uses its bundled GD
    local gd_check = os.execute("pkg-config --exists gdlib 2>/dev/null")
    if gd_check == 0 or gd_check == true then
        configureOptions = configureOptions .. " --with-external-gd"
    end

    -- Check for PostgreSQL
    local pgsql_check = os.execute("pg_config --version 2>/dev/null")
    if pgsql_check == 0 or pgsql_check == true then
        configureOptions = configureOptions .. " --with-pdo-pgsql"
    end

    -- Check for libzip
    local zip_check = os.execute("pkg-config --exists libzip 2>/dev/null")
    if zip_check == 0 or zip_check == true then
        configureOptions = configureOptions .. " --with-zip"
    end

    return configureOptions
end

--- Install Composer
local function fetch_https_body(url)
    -- mise exposes try_get for errors-as-values; standalone vfox's get already
    -- uses that convention. Avoid pcall because mise's async HTTP can yield.
    local response, err
    if http.try_get ~= nil then
        response, err = http.try_get({ url = url })
    else
        response, err = http.get({ url = url })
    end
    if err ~= nil then
        return nil, err
    end
    if response == nil then
        return nil, "empty response"
    end
    if response.status_code ~= 200 then
        return nil, "HTTP " .. tostring(response.status_code)
    end

    return response.body
end

local function download_https_file(url, path)
    local body, err = fetch_https_body(url)
    if body == nil then
        return nil, err
    end

    local file, open_err = io.open(path, "wb")
    if file == nil then
        return nil, open_err
    end
    local wrote, write_err = file:write(body)
    local closed, close_err = file:close()
    if wrote == nil or closed == nil then
        os.remove(path)
        return nil, write_err or close_err
    end

    return true
end

function install_composer(sdkPath)
    print("Installing Composer...")

    local php_bin = shell_quote(sdkPath .. "/bin/php")

    local installer_path = sdkPath .. "/composer-setup.php"
    local downloaded, download_err = download_https_file("https://getcomposer.org/installer", installer_path)
    if downloaded == nil then
        io.stderr:write("Warning: Failed to download Composer installer: " .. tostring(download_err) .. "\n")
        return
    end

    local expected_signature, signature_err = fetch_https_body("https://composer.github.io/installer.sig")
    if expected_signature == nil then
        os.remove(installer_path)
        error("Failed to download Composer installer signature: " .. tostring(signature_err))
    end
    expected_signature = string.gsub(expected_signature, "%s+", "")
    -- The signature is a SHA-384 hex digest; reject anything else since it is
    -- embedded in a shell command below
    if #expected_signature ~= 96 or string.find(expected_signature, "^%x+$") == nil then
        os.remove(installer_path)
        error("Composer installer signature is not a valid SHA-384 digest")
    end

    local php_path_literal = string.gsub(installer_path, "[\\']", "\\%0")
    local verify_cmd = string.format(
        "%s -r %s",
        php_bin,
        shell_quote(
            "if (hash_file('sha384', '" .. php_path_literal .. "') !== '" .. expected_signature .. "') { exit(1); }"
        )
    )
    local status = os.execute(verify_cmd)
    if status ~= 0 and status ~= true then
        os.remove(sdkPath .. "/composer-setup.php")
        error("Composer installer signature verification failed")
    end

    local install_cmd = string.format(
        "%s %s --install-dir=%s --filename=composer",
        php_bin,
        shell_quote(installer_path),
        shell_quote(sdkPath .. "/bin")
    )
    status = os.execute(install_cmd)
    if status ~= 0 and status ~= true then
        io.stderr:write("Warning: Failed to install Composer\n")
    end

    -- Cleanup
    os.remove(sdkPath .. "/composer-setup.php")
end
