local util = require("util")

--- Extension point, called after PreInstall, can perform additional operations,
--- such as file operations for the SDK installation directory
--- @param ctx table
--- @field ctx.rootPath string SDK installation directory
function PLUGIN:PostInstall(ctx)
	local rootPath = ctx.rootPath
	local os_name = util.getOsName()

	-- Find the JAR file in the install directory
	local find_cmd
	local jar_path
	if RUNTIME.osType == "windows" then
		find_cmd = string.format('dir /b "%s\\aapt2-*.jar" 2>nul', rootPath)
	else
		find_cmd = string.format('ls "%s"/aapt2-*.jar 2>/dev/null | head -1', rootPath)
	end

	local handle = io.popen(find_cmd)
	if handle then
		jar_path = handle:read("*l")
		handle:close()
	end

	if not jar_path or jar_path == "" then
		-- JAR not found, maybe already extracted
		return
	end

	-- On Windows, need to prepend the rootPath
	if RUNTIME.osType == "windows" and not string.match(jar_path, "^[A-Za-z]:") then
		jar_path = rootPath .. "\\" .. jar_path
	end

	-- Extract the JAR file (it's a ZIP)
	local extract_cmd
	if RUNTIME.osType == "windows" then
		extract_cmd = string.format(
			"powershell -Command \"Expand-Archive -Path '%s' -DestinationPath '%s' -Force\"",
			jar_path,
			rootPath
		)
	else
		extract_cmd = string.format('unzip -o "%s" -d "%s"', jar_path, rootPath)
	end

	local result = os.execute(extract_cmd)
	if not result then
		error("Failed to extract JAR file: " .. jar_path)
	end

	-- Remove the JAR file after extraction
	os.remove(jar_path)

	-- Make the binary executable on Unix systems
	if RUNTIME.osType ~= "windows" then
		local aapt2_path = rootPath .. "/aapt2"
		os.execute(string.format('chmod +x "%s"', aapt2_path))
	end
end
