--- Called after the tool is installed
--- @param ctx table Context information
--- @field ctx.rootPath string The installation directory

local file = require("file")
local os = require("os")
local log = require("log")
local strings = require("strings")

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

local function get_home_dir()
    return os.getenv("HOME") or os.getenv("USERPROFILE") or ""
end

local function find_default_components_file()
    local filename = ".default-cloud-sdk-components"
    local home = get_home_dir()
    local cloudsdk_config = os.getenv("CLOUDSDK_CONFIG")
    if not cloudsdk_config or cloudsdk_config == "" then
        if RUNTIME.osType == "windows" or RUNTIME.osType == "Windows" then
            cloudsdk_config = file.join_path(os.getenv("APPDATA") or home, "gcloud")
        else
            cloudsdk_config = file.join_path(home, ".config", "gcloud")
        end
    end

    for _, dir in ipairs({ cloudsdk_config, home }) do
        local path = file.join_path(dir, filename)
        if file.exists(path) then
            return path
        end
    end
end

local function install_default_components(gcloud_bin)
    local components_file = find_default_components_file()
    if not components_file then
        return
    end

    log.info("Installing default Cloud SDK components from " .. components_file)

    local contents = file.read(components_file)
    if not contents or contents == "" then
        return
    end

    local components = {}
    for _, line in ipairs(strings.split(contents, "\n")) do
        local trimmed = strings.trim_space(line)
        if trimmed ~= "" and not string.find(trimmed, "^#") then
            if string.match(trimmed, "^[%w%-_%.]+$") then
                table.insert(components, trimmed)
            else
                log.info("Skipping invalid component name: " .. trimmed)
            end
        end
    end

    if #components == 0 then
        return
    end

    local cmd = string.format('"%s" --quiet components install %s', gcloud_bin, table.concat(components, " "))
    local status = os.execute(cmd)
    if status ~= 0 and status ~= true then
        log.error("Failed to install default Cloud SDK components")
        return
    end
    log.info("Default Cloud SDK components installed successfully")
end

function PLUGIN:PostInstall(ctx)
    local sdkInfo = ctx.sdkInfo[PLUGIN.name]
    local root_path = sdkInfo.path
    local version = sdkInfo.version or ""

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
        "--usage-reporting",
        "false",
        "--path-update",
        "false",
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

    -- Install default SDK components
    local gcloud_bin = file.join_path(sdk_path, "bin", "gcloud")
    if RUNTIME.osType == "windows" or RUNTIME.osType == "Windows" then
        gcloud_bin = gcloud_bin .. ".cmd"
    end
    install_default_components(gcloud_bin)
end
