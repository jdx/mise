--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType
local cmd = require("cmd") :: Types.CmdModule

function plugin:PostInstall(ctx: { rootPath: string, runtimeVersion: string, sdkInfo: { [string]: Types.SdkInfo } })
	--- SDK installation root path
	local rootPath = ctx.rootPath
	local _runtimeVersion = ctx.runtimeVersion

	-- Create the installation directory structure and dummy executable
	cmd.exec("mkdir -p " .. rootPath .. "/bin")
	cmd.exec("printf '%s\\n' '#!/bin/sh' \"echo 'dummy version 1.0.0'\" > " .. rootPath .. "/bin/dummy")
	cmd.exec("chmod +x " .. rootPath .. "/bin/dummy")
end
