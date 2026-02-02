--- Called after extraction to compile Chez Scheme from source.
--- @param ctx table
--- @field ctx.rootPath string Installation root path
function PLUGIN:PostInstall(ctx)
	local os_type = RUNTIME.osType

	-- Windows uses the .exe installer, not source compilation
	if os_type == "windows" then
		print("Windows installation requires the ChezScheme.exe installer from GitHub releases")
		print("Please download and run it manually from:")
		print("https://github.com/cisco/ChezScheme/releases")
		return
	end

	local root_path = ctx.rootPath

	-- The tarball extracts with contents at root level (no top-level directory to strip)
	-- We need to run configure and make install
	print("Compiling Chez Scheme from source...")

	-- Configure with install prefix
	local configure_cmd = "cd " .. root_path .. " && ./configure --installprefix=" .. root_path
	print("Running: " .. configure_cmd)
	local result = os.execute(configure_cmd)
	if result ~= 0 and result ~= true then
		error("Configure failed")
	end

	-- Build
	local make_cmd = "cd " .. root_path .. " && make"
	print("Running: make")
	result = os.execute(make_cmd)
	if result ~= 0 and result ~= true then
		error("Make failed")
	end

	-- Install to the prefix directory
	local install_cmd = "cd " .. root_path .. " && make install"
	print("Running: make install")
	result = os.execute(install_cmd)
	if result ~= 0 and result ~= true then
		error("Make install failed")
	end

	-- Clean up build artifacts to save space (optional)
	os.execute(
		"cd "
			.. root_path
			.. " && rm -rf boot c examples mats s unicode workarea Makefile configure LOG NOTICE 2>/dev/null"
	)

	print("Chez Scheme compilation complete!")
end
