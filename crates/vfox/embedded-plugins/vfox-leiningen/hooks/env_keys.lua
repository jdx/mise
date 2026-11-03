--- Returns true when version is older than 2.9.8, the first lein script that honors LEIN_JAR
local function predates_lein_jar(version)
    local major, minor, patch = version:match("^(%d+)%.(%d+)%.(%d+)")
    if not major then
        return false
    end
    major, minor, patch = tonumber(major), tonumber(minor), tonumber(patch)
    if major ~= 2 then
        return major < 2
    end
    return minor < 9 or (minor == 9 and patch < 8)
end

--- Returns environment variables to set
--- @param ctx table Context object with path field (install directory)
--- @return table Array of environment variable definitions
function PLUGIN:EnvKeys(ctx)
    local mainPath = ctx.path
    local version = mainPath:match("([^/\\]+)$")

    local envs = {
        {
            key = "PATH",
            value = mainPath .. "/bin",
        },
    }

    if predates_lein_jar(version) then
        -- Older lein scripts always look for the jar under $LEIN_HOME/self-installs
        table.insert(envs, { key = "LEIN_HOME", value = mainPath })
    else
        -- LEIN_HOME is where users keep ~/.lein settings, so leave it alone and
        -- point lein at the standalone jar installed by this plugin instead.
        table.insert(envs, {
            key = "LEIN_JAR",
            value = mainPath .. "/self-installs/leiningen-" .. version .. "-standalone.jar",
        })
    end

    return envs
end
