--- Called after the tool is installed
--- @param ctx table Context information
--- @field ctx.rootPath string The installation directory

local file = require("file")

--- Compare version strings
--- Returns true if v1 >= v2
local function version_gte(v1, v2)
    local function parse_version(v)
        local parts = {}
        for part in string.gmatch(v, "([^%.]+)") do
            table.insert(parts, tonumber(part) or 0)
        end
        return parts
    end

    local va = parse_version(v1)
    local vb = parse_version(v2)

    for i = 1, math.max(#va, #vb) do
        local na = va[i] or 0
        local nb = vb[i] or 0
        if na > nb then
            return true
        elseif na < nb then
            return false
        end
    end
    return true
end

function PLUGIN:PostInstall(ctx)
    local root_path = ctx.rootPath
    local version = ctx.version or ""

    -- The SDK extracts directly to the root path
    local sdk_path = root_path
    local install_script = file.join_path(sdk_path, "install.sh")

    -- Check if install script exists
    if not file.exists(install_script) then
        -- On Windows, use install.bat
        if RUNTIME.osType == "windows" or RUNTIME.osType == "Windows" then
            install_script = file.join_path(sdk_path, "install.bat")
        end
    end

    if not file.exists(install_script) then
        -- Some versions might not have an install script, skip silently
        return
    end

    -- Build install command arguments
    local args = {
        "--usage-reporting", "false",
        "--path-update", "false",
        "--quiet",
    }

    -- For versions >= 352.0.0, disable Python installation
    -- (gcloud bundles its own Python in newer versions)
    if version ~= "" and version_gte(version, "352.0.0") then
        table.insert(args, "--install-python")
        table.insert(args, "false")
    end

    -- Run the install script
    local cmd_str
    if RUNTIME.osType == "windows" or RUNTIME.osType == "Windows" then
        cmd_str = '"' .. install_script .. '" ' .. table.concat(args, " ")
    else
        cmd_str = 'sh "' .. install_script .. '" ' .. table.concat(args, " ")
    end

    local status = os.execute(cmd_str)
    if status ~= 0 and status ~= true then
        error("Failed to run gcloud install script")
    end
end
