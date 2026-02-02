--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

--- Return environment variables for the tool
function plugin:EnvKeys(
	ctx: { path: string, runtimeVersion: string, sdkInfo: { [string]: Types.SdkInfo }? }
): { Types.EnvKeyResult }
	local mainPath = ctx.path
	return {
		{
			key = "PATH",
			value = mainPath .. "/bin",
		},
		{
			key = "NODE_HOME",
			value = mainPath,
		},
	}
end
