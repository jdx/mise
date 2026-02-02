--- Returns environment variables for PostgreSQL
--- @param ctx table Context provided by vfox
--- @return table Environment configuration
function PLUGIN:EnvKeys(ctx)
	local sdkInfo = ctx.sdkInfo["postgres"]
	local installDir = sdkInfo.path

	local envs = {
		{
			key = "PATH",
			value = installDir .. "/bin",
		},
		{
			key = "PGDATA",
			value = installDir .. "/data",
		},
	}

	-- Add LD_LIBRARY_PATH on Linux
	if RUNTIME.osType == "linux" then
		table.insert(envs, {
			key = "LD_LIBRARY_PATH",
			value = installDir .. "/lib",
		})
	end

	return envs
end
