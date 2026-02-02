--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

--- Return the URL to download the tool
function plugin:PreInstall(ctx: { version: string, runtimeVersion: string }): Types.PreInstallResult
	local version = ctx.version

	-- Return simple test data without runtime checks
	return {
		version = version,
		url = "file:///fake/nodejs/node-v" .. version .. ".tar.gz",
		sha256 = "fakehash" .. version,
	}
end
