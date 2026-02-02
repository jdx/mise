--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType

--- Return all available versions provided by this plugin
function plugin:Available(_ctx: { args: { string }? }): { Types.AvailableResult }
	return {
		{
			version = "1.0.0",
		},
		{
			version = "1.0.1",
		},
	}
end
