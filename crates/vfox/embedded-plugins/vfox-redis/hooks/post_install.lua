--- Compiles Redis from source after extraction
--- @param ctx table Context object with rootPath field
function PLUGIN:PostInstall(ctx)
    local rootPath = ctx.rootPath

    -- Redis doesn't need configure, just make with PREFIX
    local build_cmd = string.format(
        'cd "%s" && make PREFIX="%s" install',
        rootPath,
        rootPath
    )

    print("Compiling Redis from source...")
    local result = os.execute(build_cmd)

    if not result or result ~= 0 and result ~= true then
        error("Failed to compile Redis. Make sure you have a C compiler (gcc/clang) and make installed.")
    end

    -- Clean up source files to save space (keep only bin/)
    local cleanup_cmd = string.format(
        'cd "%s" && rm -rf src deps tests runtest runtest-cluster runtest-sentinel runtest-moduleapi sentinel.conf redis.conf Makefile 2>/dev/null',
        rootPath
    )
    os.execute(cleanup_cmd)

    print("Redis compiled successfully!")
end
