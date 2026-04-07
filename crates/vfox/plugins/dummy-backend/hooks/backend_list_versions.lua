function PLUGIN:BackendListVersions(ctx)
    local cmd = require("cmd")
    local ok, result = pcall(cmd.exec, "printenv MY_TEST_VAR")
    if ok and result then
        local trimmed = result:gsub("%s+$", "")
        return {versions = {trimmed}}
    end
    return {versions = {"fallback"}}
end
