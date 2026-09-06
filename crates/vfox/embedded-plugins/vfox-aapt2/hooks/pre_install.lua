local util = require("util")

--- Returns pre-installed information, such as version number, download address, etc.
--- @param ctx {version: string}  (User-input version)
--- @return table Version information
function PLUGIN:PreInstall(ctx)
	local version = ctx.version

	return {
		version = version,
		url = util.getDownloadUrl(version),
	}
end
