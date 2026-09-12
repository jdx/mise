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

    local env_vars = {
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

    -- Add tools installed with sdkmanager to PATH, if they exist
    local optional_bin_paths = { "platform-tools", "emulator" }
    for _, relative_optional_bin_path in ipairs(optional_bin_paths) do
        local optional_bin_path = file.join_path(install_path, relative_optional_bin_path)
        if file.exists(optional_bin_path) then
            table.insert(env_vars, {
                key = "PATH",
                value = optional_bin_path,
            })
        end
    end

    return env_vars
end
