--!strict
local Types = require("@lib/types")

local plugin = PLUGIN :: Types.PluginType
local env = require("env") :: Types.EnvModule

--- Get the available version list.
function plugin:Available(_ctx: { args: { string }? }): { Types.AvailableResult }
	if (env :: any)["TEST_VFOX_LOG"] then
		local log = require("log") :: Types.LogModule
		log.trace("log.trace msg")
		log.debug("log.debug msg")
		log.info("log.info msg")
		log.warn("log.warn msg")
		log.error("log.error msg")
		log.info("multi", "arg", 123)
		print("print msg")
	end
	return {
		{
			version = "1.0.0",
		},
		{
			version = "1.0.1",
		},
	}
end
