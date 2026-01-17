--- Returns download information for the .NET installer script
--- @param ctx table Context provided by vfox (contains version)
--- @return table Version info with download URL
function PLUGIN:PreInstall(ctx)
    local version = ctx.version

    if version == nil or version == "" then
        error("You must provide a version number, eg: mise install dotnet@9.0.309")
    end

    -- We download the official dotnet-install script
    -- The actual SDK download is handled by the script in PostInstall
    local url
    if RUNTIME.osType == "windows" then
        url = "https://dot.net/v1/dotnet-install.ps1"
    else
        url = "https://dot.net/v1/dotnet-install.sh"
    end

    return {
        version = version,
        url = url,
    }
end
