--- Installs bpkg by copying bash scripts to bin directory
--- @param ctx table Context provided by vfox
function PLUGIN:PostInstall(ctx)
    local cmd = require("cmd")

    local sdkInfo = ctx.sdkInfo["bpkg"]
    local path = sdkInfo.path
    local version = sdkInfo.version

    -- The tarball may extract to bpkg-{version}/ or directly to path
    -- Check which case we have
    local srcDir = path .. "/bpkg-" .. version
    local file = io.open(srcDir .. "/bpkg.sh", "r")
    if file then
        file:close()
    else
        -- Files are directly in path
        srcDir = path
    end

    -- Create bin directory
    cmd.exec("mkdir -p '" .. path .. "/bin'")

    -- bpkg uses symlinks to lib/ directory, so we need to:
    -- 1. Copy the lib directory
    -- 2. Copy the main bpkg.sh script
    -- 3. Copy the symlinks (they point to lib/)

    -- Copy lib directory
    cmd.exec("cp -r '" .. srcDir .. "/lib' '" .. path .. "/bin/'")

    -- Copy main script
    cmd.exec("cp -f '" .. srcDir .. "/bpkg.sh' '" .. path .. "/bin/'")

    -- Copy all symlinks (they'll still point to lib/ which we copied)
    local scripts = {
        "bpkg",
        "bpkg-env",
        "bpkg-getdeps",
        "bpkg-init",
        "bpkg-install",
        "bpkg-json",
        "bpkg-list",
        "bpkg-package",
        "bpkg-run",
        "bpkg-show",
        "bpkg-source",
        "bpkg-suggest",
        "bpkg-term",
        "bpkg-update",
        "bpkg-utils",
        "bpkg-realpath",
    }

    for _, script in ipairs(scripts) do
        local src = srcDir .. "/" .. script
        local dst = path .. "/bin/" .. script
        -- Copy symlink as symlink (-P preserves symlinks)
        cmd.exec("cp -P '" .. src .. "' '" .. dst .. "' 2>/dev/null || true")
    end

    -- Make all scripts executable
    cmd.exec("chmod +x '" .. path .. "/bin/bpkg.sh'")
    cmd.exec("find '" .. path .. "/bin/lib' -name '*.sh' -exec chmod +x {} \\;")

    -- Verify main bpkg script was installed
    local file = io.open(path .. "/bin/bpkg.sh", "r")
    if file then
        file:close()
    else
        error("Failed to install bpkg - main script not found at " .. path .. "/bin/bpkg.sh")
    end
end
