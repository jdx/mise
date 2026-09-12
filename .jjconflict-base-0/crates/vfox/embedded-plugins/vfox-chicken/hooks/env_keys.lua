--- Returns environment variables to set for CHICKEN
--- Sets PATH to include the bin directory and library paths

function PLUGIN:EnvKeys(ctx)
    local file = require("file")
    local main_path = ctx.path
    local lib_path = file.join_path(main_path, "lib")

    return {
        {
            key = "PATH",
            value = file.join_path(main_path, "bin"),
        },
        {
            key = "CHICKEN_HOME",
            value = main_path,
        },
        {
            key = "LD_LIBRARY_PATH",
            value = lib_path,
        },
        {
            key = "DYLD_LIBRARY_PATH",
            value = lib_path,
        },
    }
end
