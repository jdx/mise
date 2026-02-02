--- Called after installation is complete to fix binary permissions.
--- @param ctx table
--- @field ctx.rootPath string Installation root path
function PLUGIN:PostInstall(ctx)
	local os_type = RUNTIME.osType

	-- On Unix systems, we need to make the binary executable
	if os_type ~= "windows" then
		local func_path = ctx.rootPath .. "/func"
		-- Use os.execute to chmod +x the binary
		local result = os.execute("chmod +x " .. func_path)
		if result ~= 0 and result ~= true then
			-- Some Lua versions return true on success, others return 0
			print("Warning: could not chmod +x " .. func_path)
		end

		-- Also chmod other potential executables
		local gozip_path = ctx.rootPath .. "/gozip"
		os.execute("chmod +x " .. gozip_path .. " 2>/dev/null")

		local createdump_path = ctx.rootPath .. "/createdump"
		os.execute("chmod +x " .. createdump_path .. " 2>/dev/null")
	end
end
