--- Get the available version list.
--- @param ctx table Empty table, no data provided. Always {}.
--- @return table Version list
function PLUGIN:Available(ctx)
	if os.getenv("TEST_VFOX_LOG") then
		local log = require("log")
		log.trace("log.trace msg")
		log.debug("log.debug msg")
		log.info("log.info msg")
		log.warn("log.warn msg")
		log.error("log.error msg")
		log.info("multi", "arg", 123)
		print("print msg")
		io.stderr:write("stderr msg\n")
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
