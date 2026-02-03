local http = require("http")
local json = require("json")

--- Returns pre-installed information, such as version number.
--- For pipenv, we install via pip so we just validate the version exists.
--- @param ctx {version: string}  (User-input version)
--- @return table Version information
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    -- Validate version exists on PyPI
    local resp, err = http.get({
        url = "https://pypi.org/pypi/pipenv/" .. version .. "/json",
    })

    if err ~= nil then
        error("Failed to validate pipenv version: " .. err)
    end

    if resp.status_code == 404 then
        error("Pipenv version " .. version .. " not found on PyPI")
    end

    if resp.status_code ~= 200 then
        error("Failed to validate pipenv version: HTTP " .. resp.status_code)
    end

    -- No download URL needed - we install via pip in PostInstall
    return {
        version = version,
    }
end
