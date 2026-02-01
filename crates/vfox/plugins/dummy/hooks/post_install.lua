function PLUGIN:PostInstall(ctx)
	local cmd = require("cmd")
	--- SDK installation root path
	local rootPath = ctx.rootPath
	local _runtimeVersion = ctx.runtimeVersion

	-- Create the installation directory structure and dummy executable
	cmd.exec("mkdir -p " .. rootPath .. "/bin")
	cmd.exec("printf '%s\\n' '#!/bin/sh' \"echo 'dummy version 1.0.0'\" > " .. rootPath .. "/bin/dummy")
	cmd.exec("chmod +x " .. rootPath .. "/bin/dummy")
end
