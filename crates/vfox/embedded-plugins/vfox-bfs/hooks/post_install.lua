local util = require("util")

--- Extension point, called after PreInstall, compiles bfs from source
--- @param ctx table
--- @field ctx.rootPath string SDK installation directory
function PLUGIN:PostInstall(ctx)
    local rootPath = ctx.rootPath

    -- mise extracts the tarball and moves content directly to rootPath
    -- So rootPath IS the source directory

    -- Check if configure script exists (to verify we're in the right place)
    if not util.exec_ok(string.format('test -f "%s/configure"', rootPath)) then
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
    if not util.exec_ok(build_cmd) then
        error("Failed to compile bfs. Make sure you have a C compiler (gcc/clang) and make installed.")
    end

    -- bfs binary is built in bin/ subdirectory
    -- Make sure it's executable
    local binPath = rootPath .. "/bin"
    if not util.exec_ok(string.format('chmod +x "%s/bfs"', binPath)) then
        error("Failed to make bfs executable at " .. binPath .. "/bfs")
    end

    print("bfs compiled successfully!")
end
