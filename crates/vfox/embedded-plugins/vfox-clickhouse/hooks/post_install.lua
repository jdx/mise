--- Extension point, called after PreInstall, sets up clickhouse binary
--- @param ctx table
--- @field ctx.rootPath string SDK installation directory
function PLUGIN:PostInstall(ctx)
    local rootPath = ctx.rootPath
    local binDir = rootPath .. "/bin"

    -- Create bin directory
    os.execute(string.format('mkdir -p "%s"', binDir))

    if RUNTIME.osType == "darwin" then
        -- macOS: single binary file was downloaded
        -- Find the downloaded binary (clickhouse-macos or clickhouse-macos-aarch64)
        local find_cmd = string.format('ls "%s"/clickhouse-macos* 2>/dev/null | head -1', rootPath)
        local handle = io.popen(find_cmd)
        if not handle then
            error("Failed to find clickhouse binary")
        end
        local binary_path = handle:read("*l")
        handle:close()

        if not binary_path or binary_path == "" then
            error("Could not find clickhouse-macos binary in " .. rootPath)
        end

        -- Move and rename to bin/clickhouse
        local move_cmd = string.format('mv "%s" "%s/clickhouse"', binary_path, binDir)
        local result = os.execute(move_cmd)
        if not result then
            error("Failed to move clickhouse binary")
        end

        -- Make executable and remove quarantine
        os.execute(string.format('chmod +x "%s/clickhouse"', binDir))
        os.execute(string.format('xattr -d com.apple.quarantine "%s/clickhouse" 2>/dev/null || true', binDir))
    else
        -- Linux: tarball was extracted
        -- The tarball extracts to usr/bin/clickhouse
        local src_binary = rootPath .. "/usr/bin/clickhouse"

        -- Check if binary exists at expected location
        local check_cmd = string.format('test -f "%s"', src_binary)
        if not os.execute(check_cmd) then
            -- Try alternate location
            src_binary = rootPath .. "/clickhouse"
            check_cmd = string.format('test -f "%s"', src_binary)
            if not os.execute(check_cmd) then
                error("Could not find clickhouse binary after extraction")
            end
        end

        -- Move binary to bin/
        local move_cmd = string.format('mv "%s" "%s/clickhouse"', src_binary, binDir)
        local result = os.execute(move_cmd)
        if not result then
            -- Try copying instead
            move_cmd = string.format('cp "%s" "%s/clickhouse"', src_binary, binDir)
            result = os.execute(move_cmd)
            if not result then
                error("Failed to copy clickhouse binary to bin/")
            end
        end

        -- Make executable
        os.execute(string.format('chmod +x "%s/clickhouse"', binDir))

        -- Clean up extracted directories
        os.execute(
            string.format('rm -rf "%s/usr" "%s/install" "%s/etc" 2>/dev/null || true', rootPath, rootPath, rootPath)
        )
    end

    -- Create symlinks for clickhouse subcommands
    local symlinks = {
        "clickhouse-client",
        "clickhouse-server",
        "clickhouse-local",
        "clickhouse-benchmark",
        "clickhouse-compressor",
        "clickhouse-format",
        "clickhouse-obfuscator",
    }

    for _, link in ipairs(symlinks) do
        os.execute(string.format('ln -sf clickhouse "%s/%s" 2>/dev/null || true', binDir, link))
    end

    print("ClickHouse installed successfully!")
end
