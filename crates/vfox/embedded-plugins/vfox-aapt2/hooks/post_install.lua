local util = require("util")

--- Extension point, called after PreInstall, can perform additional operations,
--- such as file operations for the SDK installation directory
--- @param ctx table
--- @field ctx.rootPath string SDK installation directory
function PLUGIN:PostInstall(ctx)
	local rootPath = ctx.rootPath

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

	if not util.exec_ok(extract_cmd) then
		error("Failed to extract JAR file: " .. jar_path)
	end

	-- Verify the binary actually landed in the install directory before we
	-- delete the JAR or report success — extraction can silently produce no
	-- output if the archive layout is unexpected.
	local expected_bin = rootPath .. (RUNTIME.osType == "windows" and "\\aapt2.exe" or "/aapt2")
	local probe = io.open(expected_bin, "rb")
	if not probe then
		error("Extraction completed but expected aapt2 binary is missing at " .. expected_bin)
	end
	probe:close()

	-- Remove the JAR file after extraction
	os.remove(jar_path)

	-- Make the binary executable on Unix systems
	if RUNTIME.osType ~= "windows" then
		if not util.exec_ok(string.format('chmod +x "%s"', expected_bin)) then
			error("Failed to make aapt2 binary executable at " .. expected_bin)
		end
	end
end
