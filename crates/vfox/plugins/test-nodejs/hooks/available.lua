--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

--- Return all available versions provided by this plugin
function plugin:Available(_ctx: { args: { string }? }): { Types.AvailableResult }
	-- Return hardcoded test versions to avoid network calls
	return {
		{
			version = "20.3.0",
			note = "",
			addition = {
				{
					name = "npm",
					version = "9.6.7",
				},
			},
		},
		{
			version = "20.1.0",
			note = "",
			addition = {
				{
					name = "npm",
					version = "9.6.4",
				},
			},
		},
		{
			version = "20.0.0",
			note = "LTS",
			addition = {
				{
					name = "npm",
					version = "9.6.4",
				},
			},
		},
		{
			version = "19.0.0",
			note = "",
			addition = {
				{
					name = "npm",
					version = "9.0.0",
				},
			},
		},
		{
			version = "18.0.0",
			note = "LTS",
			addition = {
				{
					name = "npm",
					version = "8.0.0",
				},
			},
		},
	}
end
