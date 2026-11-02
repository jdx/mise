--- Python discovery for gcloud. gcloud accepts an interpreter whose minor
--- version exactly matches the one its macOS installer wants, so a candidate
--- is only used after being executed and checked.

local file = require("file")
local cmd = require("cmd")
local strings = require("strings")

local M = {}

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

--- Return the path of the interpreter mise installs for `version`. mise strips
--- its shim directories from `PATH` while plugin hooks run, so mise-managed
--- interpreters can only be found by probing their install directory directly.
local function mise_python(version)
    local data = os.getenv("XDG_DATA_HOME") or file.join_path(os.getenv("HOME") or "", ".local", "share")
    local data_dir = os.getenv("MISE_DATA_DIR") or file.join_path(data, "mise")
    return file.join_path(data_dir, "installs", "python", version, "bin", "python")
end

--- Find an interpreter that is exactly `version`, preferring the locations
--- gcloud itself looks in before mise's install directory, then PATH.
local function find_python(version)
    local candidates = {}
    local configured = os.getenv("CLOUDSDK_PYTHON")
    if configured and configured ~= "" then
        table.insert(candidates, configured)
    end
    table.insert(candidates, "/Library/Frameworks/Python.framework/Versions/" .. version .. "/bin/python3")
    table.insert(candidates, "/opt/homebrew/bin/python" .. version)
    table.insert(candidates, "/usr/local/bin/python" .. version)
    table.insert(candidates, mise_python(version))
    table.insert(candidates, "python" .. version)

    for _, candidate in ipairs(candidates) do
        local python = resolve_python(candidate, version)
        if python then
            return python
        end
    end
end

--- Find the interpreter the SDK wants on macOS: the first candidate that runs
--- and reports exactly the required version. Returns the interpreter path and
--- the required version, or nil and the required version when there is no
--- match, or nil and nil when the required version cannot be read.
function M.find(sdk_path)
    local version = required_python_version(sdk_path)
    if not version then
        return nil, nil
    end
    return find_python(version), version
end

--- Where the interpreter chosen for an SDK is remembered. It lives inside the
--- SDK directory, so a new gcloud version starts with no cache. The first line
--- is the Python version the SDK required, the second the interpreter path.
local function cache_path(sdk_path)
    return file.join_path(sdk_path, ".mise-cloudsdk-python")
end

--- Return the cached interpreter if it was chosen for `version` and still exists.
local function read_cache(sdk_path, version)
    local handle = io.open(cache_path(sdk_path), "r")
    if not handle then
        return nil
    end
    local contents = handle:read("*a")
    handle:close()
    local cached_version, cached = string.match(contents or "", "^%s*([^\n]+)\n%s*([^\n]+)")
    if cached_version == version and cached and file.exists(strings.trim_space(cached)) then
        return strings.trim_space(cached)
    end
end

local function write_cache(sdk_path, version, python)
    local handle = io.open(cache_path(sdk_path), "w")
    if handle then
        handle:write(version, "\n", python, "\n")
        handle:close()
    end
end

--- Like `find`, for callers that run on every shell prompt (`EnvKeys`). The
--- interpreter found by a full probe is remembered next to the SDK together
--- with the Python version the SDK required, and later calls only re-read that
--- requirement and check that the interpreter still exists, falling back to
--- the probe when either changed (`gcloud components update` can raise the
--- requirement in place). A miss is never cached, so installing the right
--- Python later is picked up on the next call.
function M.find_cached(sdk_path)
    local version = required_python_version(sdk_path)
    if not version then
        return nil
    end
    local cached = read_cache(sdk_path, version)
    if cached then
        return cached
    end
    local found = find_python(version)
    if found then
        pcall(write_cache, sdk_path, version, found)
    end
    return found
end

return M
