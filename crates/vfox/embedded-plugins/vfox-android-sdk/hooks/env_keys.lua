--- Return environment variables for the tool
--- @param ctx {path: string}  (The installation path of the tool version)
--- @field ctx.version string The version
--- @return table Environment variables
function PLUGIN:EnvKeys(ctx)
    local file = require("file")

    local install_path = ctx.path
    local version = ctx.version

    -- Structure is: install_path/cmdline-tools/VERSION/bin
    local bin_path = file.join_path(install_path, "cmdline-tools", version, "bin")

    return {
        {
            key = "PATH",
            value = bin_path,
        },
        {
            key = "ANDROID_HOME",
            value = install_path,
        },
        {
            key = "ANDROID_SDK_ROOT",
            value = install_path,
        },
    }
end
