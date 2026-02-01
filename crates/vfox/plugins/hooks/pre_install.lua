--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

--- Returns some pre-installed information, such as version number, download address, local files, etc.
function plugin:PreInstall(ctx: { version: string, runtimeVersion: string }): Types.PreInstallResult
	local version = ctx.version

	return {
		version = version,
	}
end
