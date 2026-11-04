--- Called after the tool is installed
--- @param ctx table Context information
--- @field ctx.rootPath string The installation directory

local file = require("file")
local cmd = require("cmd")
local os = require("os")
local log = require("log")
local components = require("components")
local python = require("python")

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

local function log_invalid(invalid)
    for _, name in ipairs(invalid) do
        log.info("Skipping invalid component name: " .. name)
    end
end

local function file_components()
    local components_file = find_default_components_file()
    if not components_file then
        return {}
    end
    local contents = file.read(components_file)
    if not contents or contents == "" then
        return {}
    end
    log.info("Reading default Cloud SDK components from " .. components_file)
    local list, invalid = components.from_file_contents(contents)
    log_invalid(invalid)
    return list
end

--- Install the components requested by the `components` tool option and any
--- `.default-cloud-sdk-components` file. Only missing ones are installed, so
--- this is cheap when mise reruns PostInstall to add a component. A failure is
--- an error for requested components (mise must not report them installed)
--- and a warning for the default file.
local function install_components(gcloud_bin, sdk_path, options)
    local requested, invalid = components.from_options(options)
    log_invalid(invalid)

    local wanted, seen = {}, {}
    for _, id in ipairs(requested) do
        seen[id] = true
        table.insert(wanted, id)
    end
    for _, id in ipairs(file_components()) do
        if not seen[id] then
            seen[id] = true
            table.insert(wanted, id)
        end
    end

    local missing = components.missing(sdk_path, wanted)
    if #missing == 0 then
        return
    end

    log.info("Installing Cloud SDK components: " .. table.concat(missing, " "))
    local command = string.format('"%s" --quiet components install %s', gcloud_bin, table.concat(missing, " "))
    local ok, err = pcall(cmd.exec, command)
    if ok then
        log.info("Cloud SDK components installed successfully")
        return
    end
    local still_missing = components.missing(sdk_path, requested)
    if #still_missing > 0 then
        error("Failed to install Cloud SDK components " .. table.concat(still_missing, ", ") .. ": " .. tostring(err))
    end
    log.error("Failed to install default Cloud SDK components: " .. tostring(err))
end

--- Run gcloud's installer on a freshly extracted SDK.
local function run_install_script(sdk_path, install_script, version)
    -- Build install command arguments
    local args = {
        "--usage-reporting",
        "false",
        "--path-update",
        "false",
        "--quiet",
    }

    -- Only the Linux x86_64 archive bundles Python. Other platforms must use
    -- CLOUDSDK_PYTHON or let the installer provision a supported interpreter.
    local is_linux = RUNTIME.osType == "linux" or RUNTIME.osType == "Linux"
    local is_macos = RUNTIME.osType == "darwin" or RUNTIME.osType == "Darwin"
    local is_x86_64 = RUNTIME.archType == "amd64" or RUNTIME.archType == "x86_64"
    local env_prefix = ""
    if version ~= "" and version_gte(version, "352.0.0") and is_linux and is_x86_64 then
        table.insert(args, "--install-python")
        table.insert(args, "false")
    elseif is_macos then
        -- On macOS, gcloud provisions Python itself unless it finds the exact
        -- minor version it wants, by running `sudo installer`. The installer's
        -- output is captured during a mise install and macOS sudo prompts have
        -- no timeout, so that turns the install into a silent, unbounded hang.
        -- Point gcloud at a matching interpreter when there is one; otherwise
        -- turn its Python provisioning off rather than risk the prompt.
        local python_path, required = python.find(sdk_path)
        if python_path then
            env_prefix = string.format('CLOUDSDK_PYTHON="%s" ', python_path)
        elseif version ~= "" and version_gte(version, "352.0.0") then
            table.insert(args, "--install-python")
            table.insert(args, "false")
            local detail
            if required then
                detail = string.format(
                    "Python %s was not found. Install it (for example `brew install python@%s`) or point "
                        .. "CLOUDSDK_PYTHON at it",
                    required,
                    required
                )
            else
                detail = "The Python version gcloud requires could not be determined. Point CLOUDSDK_PYTHON "
                    .. "at the interpreter gcloud should use"
            end
            log.warn(
                detail
                    .. ", then reinstall gcloud. gcloud's own Python setup was skipped because it installs "
                    .. "Python system-wide with sudo, and would wait indefinitely on a password prompt that "
                    .. "is not visible here."
            )
        end
    end

    -- Run the install script
    local cmd_str
    if RUNTIME.osType == "windows" or RUNTIME.osType == "Windows" then
        cmd_str = '"' .. install_script .. '" ' .. table.concat(args, " ")
    else
        cmd_str = env_prefix .. 'sh "' .. install_script .. '" ' .. table.concat(args, " ")
    end

    local ok, err = pcall(cmd.exec, cmd_str)
    if not ok then
        error("Failed to run gcloud install script: " .. tostring(err))
    end
end

function PLUGIN:PostInstall(ctx)
    local sdkInfo = ctx.sdkInfo[PLUGIN.name]
    local root_path = sdkInfo.path
    local version = sdkInfo.version or ""

    -- The SDK extracts directly to the root path
    local sdk_path = root_path
    local install_script
    if RUNTIME.osType == "windows" or RUNTIME.osType == "Windows" then
        install_script = file.join_path(sdk_path, "install.bat")
    else
        install_script = file.join_path(sdk_path, "install.sh")
    end

    if not file.exists(install_script) then
        -- Some versions might not have an install script, skip silently
        return
    end

    -- mise reruns PostInstall on an existing SDK to add components, so only
    -- run gcloud's installer the first time.
    local installed_marker = file.join_path(sdk_path, ".mise-install-script-done")
    if not file.exists(installed_marker) then
        run_install_script(sdk_path, install_script, version)
        local marker = io.open(installed_marker, "w")
        if marker then
            marker:close()
        end
    end

    -- Install default SDK components
    local gcloud_bin = file.join_path(sdk_path, "bin", "gcloud")
    if RUNTIME.osType == "windows" or RUNTIME.osType == "Windows" then
        gcloud_bin = gcloud_bin .. ".cmd"
    end
    install_components(gcloud_bin, sdk_path, ctx.options)
end
