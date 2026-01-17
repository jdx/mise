--- vfox-dotnet plugin metadata
PLUGIN = {}

PLUGIN.name = "dotnet"
PLUGIN.version = "0.1.0"
PLUGIN.homepage = "https://github.com/mise-plugins/vfox-dotnet"
PLUGIN.license = "MIT"
PLUGIN.description = ".NET SDK version manager - dynamically fetches versions from Microsoft"

PLUGIN.minRuntimeVersion = "0.3.0"

--- Legacy version files to check
PLUGIN.legacyFilenames = {
    "global.json",
}

PLUGIN.notes = {
    "Installs the .NET SDK using Microsoft's official installer",
    "Set DOTNET_CLI_TELEMETRY_OPTOUT=1 to disable telemetry",
}
