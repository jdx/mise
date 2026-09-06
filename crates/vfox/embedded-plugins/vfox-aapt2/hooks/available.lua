local util = require("util")

--- Return all available versions provided by this plugin
--- @param ctx table Empty table used as context, for future extension
--- @return table Descriptions of available versions and accompanying tool descriptions
function PLUGIN:Available(ctx)
	local versions = util.parseVersions()

	table.sort(versions, function(a, b)
		return util.compareVersions(a.version, b.version)
	end)

	return versions
end
