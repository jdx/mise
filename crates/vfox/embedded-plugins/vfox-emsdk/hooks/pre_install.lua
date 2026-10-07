function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    return {
        version = version,
        url = "https://github.com/emscripten-core/emsdk/archive/refs/heads/main.tar.gz",
        note = "Downloading emsdk tool, then will install emscripten " .. version
    }
end
