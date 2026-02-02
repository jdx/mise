--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType
local runtime = RUNTIME :: Types.RuntimeType

--- Each SDK may have different environment variable configurations.
function plugin:EnvKeys(
	ctx: { path: string, runtimeVersion: string, sdkInfo: { [string]: Types.SdkInfo }? }
): { Types.EnvKeyResult }
	--- this variable is same as ctx.sdkInfo['plugin-name'].path
	local version_path = ctx.path
	if runtime.osType == "windows" then
		return {
			{
				key = "PATH",
				value = version_path,
			},
		}
	else
		return {
			{
				key = "PATH",
				value = version_path .. "/bin",
			},
		}
	end
end
