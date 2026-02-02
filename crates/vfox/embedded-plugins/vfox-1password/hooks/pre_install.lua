local http = require("http")

--- Returns pre-installed information, such as version number, download address, etc.
--- @param ctx {version: string}  (User-input version)
--- @return table Version information
function PLUGIN:PreInstall(ctx)
	local version = ctx.version
	local os_type = RUNTIME.osType
	local arch_type = RUNTIME.archType

	-- Determine CLI version (v1 or v2) and base path
	local cli_path
	if version:match("^1%.") then
		cli_path = "op"
	else
		cli_path = "op2"
	end

	-- Map OS type
	local os_name = os_type
	if os_type == "darwin" then
		os_name = "darwin"
	elseif os_type == "windows" then
		os_name = "windows"
	end

	-- Map architecture
	local arch = arch_type
	if arch_type == "x86_64" then
		arch = "amd64"
	elseif arch_type == "i386" or arch_type == "i686" then
		arch = "386"
	elseif arch_type == "aarch64" then
		arch = "arm64"
	end

	local base_url = "https://cache.agilebits.com/dist/1P/" .. cli_path .. "/pkg/v" .. version .. "/"
	local filename = "op_" .. os_name .. "_" .. arch .. "_v" .. version .. ".zip"
	local download_url = base_url .. filename

	-- Verify URL exists
	local resp = http.head({ url = download_url })
	if resp.status_code ~= 200 then
		error("Download URL not found: " .. download_url .. " (status: " .. resp.status_code .. ")")
	end

	return {
		version = version,
		url = download_url,
	}
end
