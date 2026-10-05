--- Return environment variables for the tool
--- @param ctx {path: string}  (The installation path of the tool version)
--- @field ctx.version string The version
--- @return table Environment variables
function PLUGIN:EnvKeys(ctx)
    local file = require("file")
    local semver = require("semver")

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

    -- Build tools are installed independently through sdkmanager. Prefer the newest
    -- installed version so commands such as apksigner use the current toolchain.
    -- Set build_tools = false on the tool to keep build-tools off PATH.
    if ctx.options.build_tools ~= false then
        local build_tools_root = file.join_path(install_path, "build-tools")
        local latest_version = nil
        local latest_path = nil

        if file.exists(build_tools_root) then
            for _, candidate_path in ipairs(file.list(build_tools_root)) do
                local stat = file.stat(candidate_path)
                local candidate_version = candidate_path:match("[^/\\\\]+$")

                if stat and stat.is_dir and candidate_version then
                    if not latest_version or semver.compare(candidate_version, latest_version) > 0 then
                        latest_version = candidate_version
                        latest_path = candidate_path
                    end
                end
            end
        end

        if latest_path then
            table.insert(env_vars, {
                key = "PATH",
                value = latest_path,
            })
        end
    end

    return env_vars
end
