--- Post-install hook to set up Leiningen
--- Downloads the lein script and organizes files properly
--- @param ctx table Context object with rootPath field
function PLUGIN:PostInstall(ctx)
    local rootPath = ctx.rootPath
    local http = require("http")

    -- Extract version from rootPath (e.g., /path/to/installs/leiningen/2.12.0)
    local version = rootPath:match("([^/\\]+)$")
    if not version then
        error("Could not extract version from rootPath: " .. rootPath)
    end

    -- Create directory structure
    local binDir = rootPath .. "/bin"
    local selfInstallDir = rootPath .. "/self-installs"

    os.execute('mkdir -p "' .. binDir .. '"')
    os.execute('mkdir -p "' .. selfInstallDir .. '"')

    -- Move the downloaded JAR to self-installs directory
    local jarName = "leiningen-" .. version .. "-standalone.jar"
    local sourceJar = rootPath .. "/" .. jarName
    local destJar = selfInstallDir .. "/" .. jarName

    -- The JAR might be at rootPath directly or we need to find it
    local moveCmd = string.format(
        'mv "%s" "%s" 2>/dev/null || find "%s" -name "*.jar" -exec mv {} "%s" \\;',
        sourceJar,
        destJar,
        rootPath,
        destJar
    )
    os.execute(moveCmd)

    -- Download the lein script
    print("Downloading lein script...")
    local resp, err = http.get({
        url = "https://raw.githubusercontent.com/technomancy/leiningen/" .. version .. "/bin/lein",
    })

    if err ~= nil then
        -- Try stable branch as fallback
        resp, err = http.get({
            url = "https://raw.githubusercontent.com/technomancy/leiningen/stable/bin/lein",
        })
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
    os.execute('chmod +x "' .. leinPath .. '"')

    -- Clean up any remaining files in root that aren't needed
    os.execute('rm -f "' .. rootPath .. '"/*.jar 2>/dev/null')

    print("Leiningen " .. version .. " installed successfully!")
end
