-- hooks/post_install.lua
function PLUGIN:PostInstall(ctx)
    local sdkInfo = ctx.sdkInfo["semver"]
    local path = sdkInfo.path

    -- Create bin directory if it doesn't exist
    os.execute("mkdir -p " .. path .. "/bin")

    -- Move the downloaded semver file to bin directory
    local srcFile = path .. "/semver"
    local destFile = path .. "/bin/semver"

    -- Move and make executable
    local result = os.execute("mv " .. srcFile .. " " .. destFile .. " && chmod +x " .. destFile)

    if result ~= 0 then
        error("Failed to install semver binary")
    end

    -- Test that it works
    local testResult = os.execute(destFile .. " --version > /dev/null 2>&1")
    if testResult ~= 0 then
        error("semver installation appears to be broken")
    end
end
