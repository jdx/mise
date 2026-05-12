--- Post-install hook to set up Leiningen
--- Downloads the lein script and organizes files properly
--- @param ctx table Context object with rootPath and sdkInfo
function PLUGIN:PostInstall(ctx)
    local rootPath = ctx.rootPath
    local http = require("http")

    -- Run a shell command and return true only on success. Lua 5.1's
    -- `os.execute` returns the numeric exit status, so `if not result`
    -- doesn't catch failures because a non-zero status is still truthy.
    local function exec_ok(cmd)
        local result = os.execute(cmd)
        return result == 0 or result == true
    end

    -- Prefer the version carried by sdkInfo (the actual SDK version mise
    -- resolved) over the basename of rootPath, which can differ from the
    -- requested version for custom install destinations.
    local version
    if ctx.sdkInfo and ctx.sdkInfo[PLUGIN.name] and ctx.sdkInfo[PLUGIN.name].version then
        version = ctx.sdkInfo[PLUGIN.name].version
    else
        version = rootPath:match("([^/\\]+)$")
    end
    if not version or version == "" then
        error("Could not determine Leiningen version from sdkInfo or rootPath: " .. rootPath)
    end

    -- Create directory structure
    local binDir = rootPath .. "/bin"
    local selfInstallDir = rootPath .. "/self-installs"

    if not exec_ok('mkdir -p "' .. binDir .. '"') then
        error("Failed to create bin directory: " .. binDir)
    end
    if not exec_ok('mkdir -p "' .. selfInstallDir .. '"') then
        error("Failed to create self-installs directory: " .. selfInstallDir)
    end

    -- Move the downloaded JAR to self-installs directory
    local jarName = "leiningen-" .. version .. "-standalone.jar"
    local sourceJar = rootPath .. "/" .. jarName
    local destJar = selfInstallDir .. "/" .. jarName

    -- The JAR might be at rootPath directly or we need to find it.
    -- Note: `find -exec ... \;` exits 0 even when no files matched, so
    -- exec_ok alone can't tell us the move succeeded — we verify destJar
    -- exists below.
    local moveCmd = string.format(
        'mv "%s" "%s" 2>/dev/null || find "%s" -maxdepth 1 -name "*.jar" -exec mv {} "%s" \\;',
        sourceJar,
        destJar,
        rootPath,
        destJar
    )
    if not exec_ok(moveCmd) then
        error("Failed to move Leiningen JAR into " .. selfInstallDir)
    end
    local destProbe = io.open(destJar, "rb")
    if not destProbe then
        error("Leiningen JAR missing after move; expected " .. destJar)
    end
    destProbe:close()

    -- Download the lein script. http.get returns a response table with
    -- status_code; ordinary non-2xx responses do not set err, so we must
    -- inspect the status code explicitly before treating the body as a
    -- valid shell script.
    local function fetch_lein(url)
        local resp, err = http.get({ url = url })
        if err ~= nil then
            return nil, err
        end
        if resp == nil or resp.status_code == nil then
            return nil, "no response from " .. url
        end
        if resp.status_code ~= 200 then
            return nil, string.format("HTTP %s from %s", tostring(resp.status_code), url)
        end
        if not resp.body or resp.body == "" then
            return nil, "empty body from " .. url
        end
        return resp, nil
    end

    print("Downloading lein script...")
    local versioned_url = "https://raw.githubusercontent.com/technomancy/leiningen/" .. version .. "/bin/lein"
    local resp, err = fetch_lein(versioned_url)
    if err ~= nil then
        local stable_url = "https://raw.githubusercontent.com/technomancy/leiningen/stable/bin/lein"
        resp, err = fetch_lein(stable_url)
        if err ~= nil then
            error("Failed to download lein script: " .. err)
        end
    end

    -- Write the lein script
    local leinPath = binDir .. "/lein"
    local f = io.open(leinPath, "w")
    if f then
        f:write(resp.body)
        f:close()
    else
        error("Failed to write lein script")
    end

    -- Make it executable
    if not exec_ok('chmod +x "' .. leinPath .. '"') then
        error("Failed to make lein script executable at " .. leinPath)
    end

    -- Clean up any remaining files in root that aren't needed
    exec_ok('rm -f "' .. rootPath .. '"/*.jar 2>/dev/null')

    print("Leiningen " .. version .. " installed successfully!")
end
