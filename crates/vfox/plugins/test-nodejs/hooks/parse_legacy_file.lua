--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType
local file = require("file") :: Types.FileModule

--- Parse the legacy file to extract the version
function plugin:ParseLegacyFile(ctx: { filepath: string }): Types.ParseLegacyFileResult
	local filepath = ctx.filepath

	if not file.exists(filepath) then
		return {}
	end

	local content = file.read(filepath)

	-- Remove any leading/trailing whitespace and 'v' prefix
	content = content:gsub("%s+", "")
	content = content:gsub("^v", "")

	if content == "" then
		return {}
	end

	return {
		version = content,
	}
end
