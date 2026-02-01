--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType
local file = require("file") :: Types.FileModule

--- Parse the legacy file found by vfox to determine the version of the tool.
function plugin:ParseLegacyFile(ctx: { filepath: string, filename: string? }): Types.ParseLegacyFileResult
	local filename = ctx.filename
	if filename == nil then
		error("ctx.filename is nil")
	end
	if filename ~= ".dummy-version" then
		error("Expected filename to be .dummy-version, got " .. filename)
	end
	local filepath = ctx.filepath
	local content = file.read(filepath)
	content = content:gsub("%s+", "")
	if content == "" then
		return {}
	end

	return {
		version = content,
	}
end
