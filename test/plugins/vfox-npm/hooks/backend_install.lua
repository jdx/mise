function PLUGIN:BackendInstall(ctx)
    local tool = ctx.tool
    local version = ctx.version
    local install_path = ctx.install_path
    
    -- Install the package directly using npm install
    local cmd = require("cmd")
    local npm_cmd = "npm install " .. tool .. "@" .. version .. " --no-package-lock --no-save --silent"
    local result = cmd.exec(npm_cmd, {cwd = install_path})
    
    -- If we get here, the command succeeded
    return {}
end
