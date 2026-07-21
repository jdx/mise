--- Returns some pre-installed information, such as version number, download address, local files, etc.
--- If checksum is provided, vfox will automatically check it for you.
--- @param ctx {version: string} Context information (version = User-input version)
--- @return table Version information
function PLUGIN:PreInstall(ctx)
	local version = ctx.version

	return {
		version = version,
	}
end
