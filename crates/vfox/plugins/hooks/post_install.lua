--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

function plugin:PostInstall(ctx: { rootPath: string, runtimeVersion: string, sdkInfo: { [string]: Types.SdkInfo } })
	--- SDK installation root path
	local _rootPath = ctx.rootPath
	local _runtimeVersion = ctx.runtimeVersion
	--- Other SDK information, the `addition` field returned in PreInstall, obtained by name
	--TODO
	--local sdkInfo = ctx.sdkInfo['dummy']
	--local path = sdkInfo.path
	--local version = sdkInfo.version
	--local name = sdkInfo.name
end
