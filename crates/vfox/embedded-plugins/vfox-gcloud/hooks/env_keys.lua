--- Return environment variables for the tool
--- @param ctx {path: string}  Context information (SDK installation directory)
--- @return table Environment variables

local file = require("file")
local python = require("python")

function PLUGIN:EnvKeys(ctx)
    local version_path = ctx.path

    -- The SDK extracts directly to the version path
    local bin_path = file.join_path(version_path, "bin")

    local keys = {
        {
            key = "PATH",
            value = bin_path,
        },
        {
            key = "CLOUDSDK_ROOT_DIR",
            value = version_path,
        },
    }

    -- On macOS, gcloud provisions a matching interpreter itself when it cannot
    -- find one, by running `sudo installer` with output a mise-built
    -- environment cannot surface. Point it at an interpreter through
    -- CLOUDSDK_PYTHON, using the same lookup the install uses. Never override
    -- the interpreter the user chose themselves.
    local is_macos = RUNTIME.osType == "darwin" or RUNTIME.osType == "Darwin"
    local configured = os.getenv("CLOUDSDK_PYTHON")
    if is_macos and (not configured or configured == "") then
        local found = python.find(version_path)
        if found then
            table.insert(keys, {
                key = "CLOUDSDK_PYTHON",
                value = found,
            })
        end
    end

    return keys
end
