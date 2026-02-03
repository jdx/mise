--- Extension point, called after PreInstall, compiles bfs from source
--- @param ctx table
--- @field ctx.rootPath string SDK installation directory
function PLUGIN:PostInstall(ctx)
    local rootPath = ctx.rootPath

    -- mise extracts the tarball and moves content directly to rootPath
    -- So rootPath IS the source directory

    -- Check if configure script exists (to verify we're in the right place)
    local check_cmd = string.format('test -f "%s/configure"', rootPath)
    local exists = os.execute(check_cmd)
    if not exists then
        error(
            "Could not find configure script in " .. rootPath .. ". The source may not have been extracted correctly."
        )
    end

    -- Build bfs using configure && make
    -- RELEASE=y enables optimizations
    local build_cmd = string.format(
        'cd "%s" && ./configure RELEASE=y && make -j$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)',
        rootPath
    )

    print("Compiling bfs from source...")
    local result = os.execute(build_cmd)
    if not result then
        error("Failed to compile bfs. Make sure you have a C compiler (gcc/clang) and make installed.")
    end

    -- bfs binary is built in bin/ subdirectory
    -- Make sure it's executable
    local binPath = rootPath .. "/bin"
    os.execute(string.format('chmod +x "%s/bfs"', binPath))

    print("bfs compiled successfully!")
end
