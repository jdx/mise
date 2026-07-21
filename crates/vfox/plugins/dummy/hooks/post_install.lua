function PLUGIN:PostInstall(ctx)
	--- SDK installation root path
	local rootPath = ctx.rootPath
	local runtimeVersion = ctx.runtimeVersion

	-- Create the installation directory structure for dummy plugin
	os.execute("mkdir -p " .. rootPath .. "/bin")
	local version_file = io.open(rootPath .. "/VERSION", "w")
	if version_file then
		version_file:write(runtimeVersion)
		version_file:close()
	end

	-- Create a dummy executable
	local dummy_file = io.open(rootPath .. "/bin/dummy", "w")
	if dummy_file then
		dummy_file:write("#!/bin/sh\necho 'dummy version 1.0.0'\n")
		dummy_file:close()
		os.execute("chmod +x " .. rootPath .. "/bin/dummy")
	end
end
