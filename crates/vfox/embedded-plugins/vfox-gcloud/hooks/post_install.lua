--- Called after the tool is installed
--- @param ctx table Context information
--- @field ctx.rootPath string The installation directory

local file = require("file")
local cmd = require("cmd")
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

--- Read the exact Python minor version gcloud's macOS installer wants, which is
--- recorded in the extracted SDK. Returns e.g. "3.14", or nil if it cannot be read.
local function required_python_version(sdk_path)
    local manager = file.join_path(sdk_path, "lib", "googlecloudsdk", "core", "updater", "python_manager.py")
    if not file.exists(manager) then
        return nil
    end
    local contents = file.read(manager)
    if not contents then
        return nil
    end
    return string.match(contents, "PYTHON_VERSION%s*=%s*['\"](%d+%.%d+)['\"]")
end

--- Return the absolute path of `candidate` if it runs and reports exactly
--- `version` (e.g. "3.14"), otherwise nil. gcloud accepts only an exact match.
local function resolve_python(candidate, version)
    local command = string.format(
        '"%s" -c "import sys; print(sys.executable); print(\'.\'.join(str(n) for n in sys.version_info[:2]))"',
        candidate
    )
    local ok, output = pcall(cmd.exec, command)
    if not ok or type(output) ~= "string" then
        return nil
    end
    local lines = strings.split(strings.trim_space(output), "\n")
    if #lines < 2 or strings.trim_space(lines[2]) ~= version then
        return nil
    end
    local executable = strings.trim_space(lines[1])
    if executable == "" then
        return candidate
    end
    return executable
end

--- Find an interpreter that is exactly `version`, preferring the locations
--- gcloud itself looks in before falling back to PATH (which includes
--- mise-managed tools).
local function find_python(version)
    local candidates = {}
    local configured = os.getenv("CLOUDSDK_PYTHON")
    if configured and configured ~= "" then
        table.insert(candidates, configured)
    end
    table.insert(candidates, "/Library/Frameworks/Python.framework/Versions/" .. version .. "/bin/python3")
    table.insert(candidates, "/opt/homebrew/bin/python" .. version)
    table.insert(candidates, "/usr/local/bin/python" .. version)
    table.insert(candidates, "python" .. version)

    for _, candidate in ipairs(candidates) do
        local python = resolve_python(candidate, version)
        if python then
            return python
        end
    end
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

    local command = string.format('"%s" --quiet components install %s', gcloud_bin, table.concat(components, " "))
    local ok, err = pcall(cmd.exec, command)
    if not ok then
        log.error("Failed to install default Cloud SDK components: " .. tostring(err))
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
        local python_version = required_python_version(sdk_path)
        local python = python_version and find_python(python_version)
        if python then
            env_prefix = string.format('CLOUDSDK_PYTHON="%s" ', python)
        elseif version ~= "" and version_gte(version, "352.0.0") then
            table.insert(args, "--install-python")
            table.insert(args, "false")
            local detail
            if python_version then
                detail = string.format(
                    "Python %s was not found. Install it (for example `brew install python@%s`) or point "
                        .. "CLOUDSDK_PYTHON at it",
                    python_version,
                    python_version
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

    -- Install default SDK components
    local gcloud_bin = file.join_path(sdk_path, "bin", "gcloud")
    if RUNTIME.osType == "windows" or RUNTIME.osType == "Windows" then
        gcloud_bin = gcloud_bin .. ".cmd"
    end
    install_default_components(gcloud_bin)
end
