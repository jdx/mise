--- Return environment variables for the tool
--- @param ctx table Context information
--- @field ctx.path string SDK installation directory
--- @return table Environment variables

local file = require("file")

function PLUGIN:EnvKeys(ctx)
    local version_path = ctx.path

    -- The SDK extracts directly to the version path
    local bin_path = file.join_path(version_path, "bin")

    return {
        {
            key = "PATH",
            value = bin_path,
        },
        {
            key = "CLOUDSDK_ROOT_DIR",
            value = version_path,
        },
    }
end
